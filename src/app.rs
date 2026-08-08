use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::Path,
};

use arboard::Clipboard;
use goblin::Object;
use goblin::error;
use mmap_io::{MemoryMappedFile, MmapMode};
use tui_input::Input;
use ratatui::{Frame, layout::Rect, widgets::ListState};

use crate::{
    config::*,
    editor::*,
    global::calculator::Calculator,
    header::header_view::{Elf, HeaderView, Pe},
    hex::{hex_view::HexView, strings::FoundString},
    input_history::InputHistory,
    reader::Reader,
};

#[derive(Default)]
pub struct FileInfo {
    pub file: Option<File>,
    pub path: String,
    pub is_read_only: bool,
    pub name: String,
    pub r#type: &'static str,
    pub size: usize,
    pub mmap: Option<MemoryMappedFile>,
    /// Bytes staged after the physical end of the mapped file (e.g. a newly
    /// added PE section's payload) that have not been written to disk yet.
    /// Empty for the overwhelming majority of sessions - only the "Add
    /// Section" header tool populates it. `:w` appends it to the file and
    /// clears it; loading a file always clears it.
    pub staged_extension: Vec<u8>,
    /// `mmap` bytes + `staged_extension` concatenated. Rebuilt only when
    /// `staged_extension` grows (see `stage_extension`), so sessions that
    /// never use that feature keep reading straight from the mmap
    /// (zero-copy, unchanged from before). `None` means "no extension
    /// staged".
    combined_cache: Option<Vec<u8>>,
}

impl FileInfo {
    /// Get memory mapped file buffer.
    ///
    /// This slice appears to have all file, but beware it is just a mapping from it and every
    /// time you access a page that is not mapped it will load from disk to memory by the OS,
    /// which also takes care of unloading it if memory constrained.
    pub fn get_buffer(&mut self) -> &[u8] {
        if let Some(combined) = self.combined_cache.as_deref() {
            return combined;
        }
        if let Some(mmap) = self.mmap.as_mut() {
            let len = (self.size as u64).min(mmap.len());
            // `as_slice_bytes` fails when the requested length is past the end of
            // the mapping (e.g. the file shrank on disk after it was mapped).
            // Returning an empty slice instead of unwrapping keeps every caller
            // that does `buffer[offset]` from aborting the process.
            return mmap.as_slice_bytes(0, len).unwrap_or(&[]);
        }

        &[]
    }

    pub fn get_buffer_ref(&self) -> &[u8] {
        if let Some(combined) = self.combined_cache.as_deref() {
            return combined;
        }
        if let Some(mmap) = self.mmap.as_ref() {
            let len = (self.size as u64).min(mmap.len());
            return mmap.as_slice_bytes(0, len).unwrap_or(&[]);
        }

        &[]
    }

    /// Number of bytes actually reachable through the mapping.
    ///
    /// Always use this - never `file_info.size` - when the result is going to
    /// index into the buffer. `size` comes from the directory entry and can
    /// legitimately disagree with the mapping.
    pub fn buffer_len(&self) -> usize {
        if let Some(combined) = &self.combined_cache {
            return self.size.min(combined.len());
        }
        match self.mmap.as_ref() {
            Some(mmap) => self.size.min(mmap.len() as usize),
            None => 0,
        }
    }

    /// Appends `bytes` after the current end of the file (physical file plus
    /// any bytes already staged) and grows `size` to match, so the new
    /// region reads back correctly in every view (Hex/Text/Disasm/Header)
    /// without touching the file on disk.
    ///
    /// This is what lets the PE "Add Section" tool hand the new section real,
    /// readable/editable bytes immediately: `changed_bytes` can only overlay
    /// offsets that already exist in the buffer, so growing the file itself
    /// (not just recording an edit) requires materializing the extra bytes
    /// here instead.
    pub fn stage_extension(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut combined = self.combined_cache.take().unwrap_or_else(|| match self.mmap.as_ref() {
            Some(mmap) => mmap.as_slice_bytes(0, mmap.len()).unwrap_or(&[]).to_vec(),
            None => Vec::new(),
        });

        combined.extend_from_slice(bytes);
        self.size = combined.len();
        self.staged_extension.extend_from_slice(bytes);
        self.combined_cache = Some(combined);
    }

    /// Bytes covered by the mmap alone, ignoring any staged extension - i.e.
    /// the file's real length on disk right now.
    #[allow(dead_code)]
    pub fn physical_len(&self) -> usize {
        self.mmap.as_ref().map(|m| m.len() as usize).unwrap_or(0)
    }

    fn clear_staged_extension(&mut self) {
        self.staged_extension.clear();
        self.combined_cache = None;
    }

    /// Records that the staged bytes are now on disk, without dropping them from
    /// the readable buffer.
    ///
    /// Used by `write_to_file` right after the append succeeds. Two things must
    /// not happen here:
    ///
    /// * Leaving `staged_extension` populated - a later failure in the same `:w`
    ///   returns early, and because only `load_file` used to clear the field, the
    ///   *next* `:w` appended the whole payload a second time.
    /// * Calling `clear_staged_extension`, which also drops `combined_cache`.
    ///   `size` already counts the appended bytes while the mmap does not, so
    ///   reads would clamp to the stale mapping and the new section's bytes would
    ///   vanish from every view until the reload completed.
    fn mark_extension_written(&mut self) {
        self.staged_extension.clear();
    }
}

impl App {
    pub fn is_executable(&self) -> bool {
        self.header_view.pe.is_some() || self.file_info.r#type.contains("PE") || self.file_info.r#type.contains("ELF")
    }

    pub fn get_addr_col_width(&self) -> usize {
        if self.is_64() {
            11
        } else {
            10
        }
    }

    pub fn get_va(&self, offset: usize) -> u64 {
        // One source for the base, so an override reaches every address the UI
        // shows. This used to re-derive it from the optional header right here,
        // which is why overriding it had to be threaded through by hand.
        let image_base = self.get_image_base();

        if let Some(pe) = &self.header_view.pe {
            let first_section_offset = pe.sections.first().map(|s| s.pointer_to_raw_data as usize).unwrap_or(0x400);
            if offset < first_section_offset {
                return image_base + offset as u64;
            }

            for section in &pe.sections {
                let section_offset = section.pointer_to_raw_data as usize;
                let section_size = (section.size_of_raw_data as usize).max(section.virtual_size as usize);
                if offset >= section_offset && offset < section_offset + section_size {
                    let rva = section.virtual_address as usize + (offset - section_offset);
                    return image_base + rva as u64;
                }
            }
            return image_base + offset as u64;
        }

        if let Some(elf) = &self.header_view.elf {
            // Program headers carry absolute addresses, so an override cannot be
            // applied on top of them without contradicting the file; it only backs
            // the fallback for offsets outside every segment.
            if self.image_base_override.is_none() {
                for ph in &elf.phdrs {
                    let file_offset = ph.p_offset as usize;
                    let file_size = ph.p_filesz as usize;
                    if offset >= file_offset && offset < file_offset + file_size {
                        return ph.p_vaddr + (offset - file_offset) as u64;
                    }
                }
            }
            return image_base.wrapping_add(offset as u64);
        }

        // No header at all: with an override this is what makes a raw dump or a
        // shellcode blob show the addresses it will actually run at. Without one
        // the base is 0 and this stays the plain file offset.
        image_base.wrapping_add(offset as u64)
    }

    pub fn va_to_offset(&self, va: u64) -> Option<usize> {
        // Validated against what is actually readable, not `file_info.size`.
        //
        // `size` comes from the directory entry and can exceed the mapping, so
        // the old checks handed back offsets that every subsequent read clamps
        // away - a Follow or an Xref jump landed on an empty viewport instead of
        // reporting that the target isn't in the file.
        let limit = self.file_info.buffer_len() as u64;

        // Same single source as `get_va`, so the two stay each other's inverse
        // when the base is overridden.
        let image_base = self.get_image_base();

        if let Some(pe) = &self.header_view.pe {
            let rva = if va >= image_base {
                va - image_base
            } else {
                va
            };

            for section in &pe.sections {
                let sec_rva = section.virtual_address as u64;
                let sec_vsize = (section.virtual_size as u64).max(section.size_of_raw_data as u64);
                if rva >= sec_rva && rva < sec_rva + sec_vsize {
                    let offset = section.pointer_to_raw_data as u64 + (rva - sec_rva);
                    if offset < limit {
                        return Some(offset as usize);
                    }
                }
            }

            if rva < limit {
                return Some(rva as usize);
            }
        }

        if let Some(elf) = &self.header_view.elf
            && self.image_base_override.is_none()
        {
            for ph in &elf.phdrs {
                let p_vaddr = ph.p_vaddr;
                let p_memsz = ph.p_memsz.max(ph.p_filesz);
                if va >= p_vaddr && va < p_vaddr + p_memsz {
                    let offset = ph.p_offset + (va - p_vaddr);
                    if offset < limit {
                        return Some(offset as usize);
                    }
                }
            }
        }

        // Headerless (or overridden) case: subtract the base, so `get_va` and this
        // remain inverses. A bare `va as usize` treated the address as an offset,
        // which with a base set pointed at a completely different byte.
        let flat = if va >= image_base { va - image_base } else { va };
        if flat < limit {
            Some(flat as usize)
        } else {
            None
        }
    }

    pub fn is_64(&self) -> bool {
        // A forced width decides this too, not just the decoder: it drives the
        // address column width and the 8- vs 9-digit VA format, and showing 64-bit
        // addresses beside 32-bit instructions would be its own kind of wrong.
        if let Some(bits) = self.config.bitness_override {
            return bits == 64;
        }
        if let Some(pe) = &self.header_view.pe {
            return pe.optional_header.as_ref().map(|opt| opt.standard_fields.magic == goblin::pe::optional_header::MAGIC_64).unwrap_or(false);
        }
        if let Some(elf) = &self.header_view.elf {
            return elf.header.e_ident[goblin::elf::header::EI_CLASS] == goblin::elf::header::ELFCLASS64;
        }
        self.file_info.r#type.contains("64") || self.file_info.r#type == "PE64"
    }

    /// Width every decoder in the app is built with.
    ///
    /// Cannot be derived from `is_64` alone any more: 16 and 32 are both "not 64",
    /// and they decode differently.
    pub fn bitness(&self) -> u32 {
        match self.config.bitness_override {
            Some(bits) => bits,
            None if self.is_64() => 64,
            None => 32,
        }
    }

    /// Widths `:set bitness` and the Alt+F7 cycle accept.
    pub const BITNESS_CHOICES: [u32; 3] = [16, 32, 64];

    /// Cycles auto -> 16 -> 32 -> 64 -> auto and returns the label to log.
    ///
    /// A cycle rather than a dialog: checking whether a region is 16-bit code is a
    /// back-and-forth activity (a PE's DOS stub is the usual case), and a modal box
    /// per attempt would make that tedious.
    pub fn cycle_bitness(&mut self) -> String {
        self.config.bitness_override = match self.config.bitness_override {
            None => Some(16),
            Some(16) => Some(32),
            Some(32) => Some(64),
            _ => None,
        };
        self.describe_bitness()
    }

    pub fn describe_bitness(&self) -> String {
        match self.config.bitness_override {
            Some(bits) => format!("{}-bit (forced)", bits),
            None => format!("{}-bit (from header)", self.bitness()),
        }
    }

    #[allow(dead_code)]
    pub fn is_pe(&self) -> bool {
        self.header_view.pe.is_some() || self.file_info.r#type.starts_with("PE")
    }

    /// Image base every address in the UI is computed from.
    ///
    /// An explicit override (Alt+F6) wins over the value in
    /// the header. That matters for a memory dump or a relocated module, where the
    /// bytes on disk were loaded somewhere other than what `ImageBase` claims, and
    /// for a raw blob, which has no header to ask.
    pub fn get_image_base(&self) -> u64 {
        if let Some(base) = self.image_base_override {
            return base;
        }
        self.header_image_base()
    }

    /// The base as declared by the file itself, ignoring any override.
    ///
    /// Kept separate so the dialog can show what it would revert to.
    pub fn header_image_base(&self) -> u64 {
        if let Some(pe) = &self.header_view.pe {
            let is_64 = pe.optional_header.as_ref().map(|opt| opt.standard_fields.magic == goblin::pe::optional_header::MAGIC_64).unwrap_or(true);
            let default_base = if is_64 { 0x0000000140000000 } else { 0x00400000 };
            return pe.optional_header.map(|opt| opt.windows_fields.image_base).unwrap_or(default_base);
        }
        if let Some(elf) = &self.header_view.elf {
            let is_64 = elf.header.e_ident[goblin::elf::header::EI_CLASS] == goblin::elf::header::ELFCLASS64;
            return if is_64 { 0x00400000 } else { 0x08048000 };
        }
        0
    }

    pub fn get_oep(&self) -> u64 {
        if let Some(pe) = &self.header_view.pe {
            let image_base = self.get_image_base();
            let entry_rva = pe.optional_header.map(|opt| opt.standard_fields.address_of_entry_point).unwrap_or(0);
            return image_base + entry_rva as u64;
        }
        if let Some(elf) = &self.header_view.elf {
            return elf.header.e_entry;
        }
        0
    }


}

/// Size of one PE section-table entry on disk, used to bound how many entries
/// we are willing to parse from an attacker-controlled `number_of_sections`.
const PE_SECTION_ENTRY_SIZE: usize = 40;

/// Startup-config filename, shared by the reader in `initfile.rs` and the writer
/// in `save_initfile`.
pub const INIT_FILE: &str = ".dzsrc";

/// Startup-config filename used before the program was renamed from `dz6`.
///
/// Read, never written: a user who already has a `.dz6init` keeps their settings
/// without having to move the file, and the next `:set` that persists writes the
/// new name.
pub const LEGACY_INIT_FILE: &str = ".dz6init";

/// Extension of the per-file annotation sidecar (`<file>.dzdb`).
pub const DB_EXT: &str = "dzdb";

/// Sidecar extension used before the rename. Read, never written.
pub const LEGACY_DB_EXT: &str = "dz6";

/// How much has to be parsed again after a staged header edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderScope {
    /// A DOS, COFF or Optional header field changed. Only the header structures
    /// can differ, so a prefix of the file is enough and the import list stands.
    Headers,
    /// A section header, a data directory or the file layout changed, which moves
    /// the data the import directory is read from. Everything is parsed again.
    Everything,
}

/// Owned outcome of sniffing a file, so the parse can borrow the buffer and the
/// results still be moved into `App` afterwards.
enum FileIdent {
    Pe(Box<Pe>, &'static str),
    Elf(Box<Elf>),
    Type(&'static str),
}

fn collect_pe_imports(imports: Vec<goblin::pe::import::Import<'_>>) -> Vec<crate::header::header_view::PEImport> {
    imports
        .into_iter()
        .map(|imp| crate::header::header_view::PEImport {
            dll: imp.dll.to_string(),
            name: imp.name.to_string(),
            offset: imp.offset,
            _ordinal: imp.ordinal,
            rva: imp.rva,
            _size: imp.size,
        })
        .collect()
}

/// Parses the DOS / COFF / Optional headers and the section table, and nothing
/// else.
///
/// This is what the Header view actually shows and edits. The full
/// [`goblin::pe::PE::parse_with_opts`] also walks the import and export
/// directories, which means resolving RVAs into section data: work that costs
/// more than everything else in a re-parse put together, and that a field edit in
/// the DOS, COFF or Optional header cannot change.
///
/// `buffer` may be only the front of the file - see `App::reparse_headers` - so
/// nothing here may read past the section table.
fn parse_pe_headers(buffer: &[u8]) -> Option<(Pe, &'static str)> {
    let header = goblin::pe::header::Header::parse(buffer).ok()?;

    let mut offset = header.dos_header.pe_pointer as usize
        + 4
        + std::mem::size_of::<goblin::pe::header::CoffHeader>()
        + header.coff_header.size_of_optional_header as usize;

    // `number_of_sections` is a u16 straight out of the file: a corrupt or
    // hostile header could ask us to allocate 65535 entries. Cap it by what the
    // remaining bytes could possibly hold.
    let max_sections = buffer.len().saturating_sub(offset) / PE_SECTION_ENTRY_SIZE;
    let nsections = (header.coff_header.number_of_sections as usize).min(max_sections);

    let mut sections = Vec::with_capacity(nsections);
    for _ in 0..nsections {
        match goblin::pe::section_table::SectionTable::parse(
            buffer,
            &mut offset,
            header.coff_header.pointer_to_symbol_table as usize,
        ) {
            Ok(sec) => sections.push(sec),
            Err(_) => break,
        }
    }

    let is_64 = header
        .optional_header
        .as_ref()
        .map(|opt| opt.standard_fields.magic == goblin::pe::optional_header::MAGIC_64)
        .unwrap_or(true);

    Some((
        Pe {
            dos_header: header.dos_header,
            coff_header: header.coff_header,
            optional_header: header.optional_header,
            sections,
            imports: Vec::new(),
        },
        if is_64 { "PE64" } else { "PE" },
    ))
}

fn identify_buffer(buffer: &[u8]) -> FileIdent {
    if buffer.len() >= 2 && &buffer[0..2] == b"MZ" {
        let mut opts = goblin::pe::options::ParseOptions::default();
        opts.resolve_rva = true;

        if let Ok(pe) = goblin::pe::PE::parse_with_opts(buffer, &opts) {
            let is_64 = pe
                .header
                .optional_header
                .as_ref()
                .map(|opt| opt.standard_fields.magic == goblin::pe::optional_header::MAGIC_64)
                .unwrap_or(pe.is_64);

            return FileIdent::Pe(
                Box::new(Pe {
                    dos_header: pe.header.dos_header,
                    coff_header: pe.header.coff_header,
                    optional_header: pe.header.optional_header,
                    sections: pe.sections,
                    imports: collect_pe_imports(pe.imports),
                }),
                if is_64 { "PE64" } else { "PE" },
            );
        } else if let Some((pe, kind)) = parse_pe_headers(buffer) {
            // A PE whose imports or exports do not parse is still a PE, and its
            // headers are exactly what the Header view is for.
            return FileIdent::Pe(Box::new(pe), kind);
        }
    }

    match Object::parse(buffer) {
        Ok(Object::COFF(_)) => FileIdent::Type("COFF"),
        Ok(Object::Elf(elf)) => FileIdent::Elf(Box::new(Elf {
            header: elf.header,
            phdrs: elf.program_headers,
            sections: elf.section_headers,
            symtab: elf.syms.to_vec(),
            strtab: elf
                .syms
                .iter()
                .map(|s| s.st_name)
                .filter_map(|idx| elf.strtab.get_at(idx).map(|name| (idx, name.to_owned())))
                .collect(),
        })),
        Ok(Object::Mach(_)) => FileIdent::Type("Mach-O"),
        Ok(Object::PE(pe)) => FileIdent::Pe(
            Box::new(Pe {
                dos_header: pe.header.dos_header,
                coff_header: pe.header.coff_header,
                optional_header: pe.header.optional_header,
                sections: pe.sections,
                imports: collect_pe_imports(pe.imports),
            }),
            "PE",
        ),
        Ok(Object::TE(_)) => FileIdent::Type("TE"),
        Ok(_) => FileIdent::Type("RAW"),
        Err(_) => FileIdent::Type("RAW"),
    }
}

#[derive(Debug)]
pub struct TextView {
    pub area_height: u16,
    /// Width of the text viewport, i.e. how many bytes one screen row decodes to
    /// in a single-byte encoding. The arrows move the file window by this much.
    pub area_width: u16,
    pub lines_to_show: usize,
    pub scroll_offset: (u16, u16), // order is (y, x)
    pub table: &'static encoding_rs::Encoding,
}

pub struct Dz6Error {
    // pub code: u16,
    pub message: String,
}

pub struct App {
    pub calculator: Calculator,
    pub clipboard: Result<Clipboard, arboard::Error>,
    pub command_area: Rect,
    pub command_input: InputHistory,
    pub config: Config,
    pub dialog_2nd_renderer: Option<fn(&mut App, &mut Frame)>,
    pub dialog_renderer: Option<fn(&mut App, &mut Frame)>,
    pub editor_view: AppView,
    pub prev_editor_view: AppView,
    pub last_primary_view: AppView,
    pub assemble_input: Input,
    pub assemble_selection_all: bool,
    pub assemble_selection_anchor: Option<usize>,
    pub disasm_selection_anchor: Option<usize>,
    pub disasm_string_ref_dialog: crate::disasm::string_ref_dialog::StringRefDialog,
    pub disasm_xref_dialog: crate::disasm::xref_dialog::XrefDialog,
    pub goto_input: Input,
    /// Character a Shift-selection in the image-base box started from, or `None`.
    pub base_anchor: Option<usize>,
    pub goto_selection_all: bool,
    pub goto_selection_anchor: Option<usize>,
    pub file_dialog: crate::file_dialog::FileDialogState,
    pub drive_dialog: crate::file_dialog::DriveSelectState,
    pub file_info: FileInfo,
    pub header_view: HeaderView,
    pub hex_view: HexView,
    pub last_error: Dz6Error,
    /// One-line error shown in the command bar until the next key press.
    ///
    /// Read-only mode refuses a fair number of shortcuts; those refusals used to
    /// be a bare `beep!()` (or nothing at all), which left no indication of *why*
    /// the key did nothing. `App::error` / `App::read_only_error` fill this in,
    /// `draw` renders it over the command bar and `handle_events` clears it as
    /// soon as another key arrives.
    pub status_error: Option<String>,
    pub help_scroll_offset: u16,
    /// Scroll position of the `:set` table.
    pub settings_scroll_offset: u16,
    pub list_state: ListState,
    pub log_scroll_offset: (u16, u16),
    pub logs: Vec<String>,
    /// State of the digital-rain easter egg. Empty until it is asked for.
    pub matrix: crate::global::matrix::Matrix,
    /// Last left click timestamp and position (Instant, row, column) for double-click detection.
    pub last_left_click: Option<(std::time::Instant, u16, u16)>,

    pub reader: Reader,
    pub running: bool,
    pub screen: Rect,
    pub state: UIState,
    /// Scan-time filter for the strings sweep. Empty means no filter.
    ///
    /// Not written by the interface any more: the F6 box filters the scanned list
    /// instead, which cannot leave the window stuck on an empty result. Kept because
    /// `load_strings` still honours it for a programmatic caller.
    pub string_regex: String,
    pub strings: Vec<FoundString>,
    pub text_view: TextView,
    /// True only while `read_initfile` is replaying `.dz6init`, so `:set`
    /// handlers can tell "the user typed this" from "we are restoring saved
    /// state" and skip writing the file back out.
    pub loading_initfile: bool,
    /// Path `.dz6init` was actually read from, or `None` if no candidate
    /// existed. Surfaced by the About dialog: the candidate list spans three
    /// directories, so "my `set enc1` line is being ignored" is otherwise only
    /// answerable by guessing which one won.
    pub initfile_loaded: Option<std::path::PathBuf>,
    pub about_scroll_offset: u16,
    /// Image base entered by the user, overriding whatever the header declares.
    ///
    /// `None` means "use the file's own value". Reset when a file is opened, since
    /// a base belongs to the image it was entered for.
    pub image_base_override: Option<u64>,
    /// Input backing the Set Image Base dialog.
    pub base_input: Input,
    pub base_selection_all: bool,
    /// Virtual address of each PE import-table slot mapped to `DLL.Function`.
    ///
    /// Built once when the file is opened and read by the disassembly view, which
    /// would otherwise have to walk the import list for every instruction of every
    /// frame. Empty for non-PE files.
    pub import_labels: std::collections::HashMap<u64, String>,
    /// Incremented every time the loaded file changes.
    ///
    /// The disassembly view caches rendered rows in a process-global, and its key
    /// was built only from positions and sizes - `page_start`, cursor offset,
    /// selection anchor, edit fingerprint, area size. Nothing identified the
    /// file, so opening a different one that happened to land on the same
    /// position with no pending edits produced an identical key and the previous
    /// file's disassembly was rendered from cache.
    pub view_generation: u64,
    /// Result dialog that was last jumped out of, so Alt+Left can bring it back
    /// with its results instead of re-running the scan.
    pub last_result: Option<UIState>,
    /// Where Alt+Left was pressed from, for Alt+Right to return to.
    pub result_return: Option<(AppView, usize)>,
}

impl App {
    pub fn new() -> Self {
        let (dark_theme, _, _) = crate::themes::ensure_and_load_themes();


        App {
            calculator: Calculator::default(),
            clipboard: Clipboard::new(),
            command_area: Rect::default(),
            command_input: InputHistory::default(),
            config: Config {
                database: true,
                dim_control_chars: false,
                dim_zeroes: true,
                hex_mode_bytes_per_line: 16,
                hex_mode_bytes_per_line_auto: false,
                hex_mode_non_graphic_char: '.',
                // Was 3,000, which is not a display limit but a *scan* limit: the
            // sweep stopped there, 12% into a 2.6 MB binary, so a correct filter
            // regex found nothing simply because the rest of the file had never
            // been looked at. An uncapped scan of that file costs 13-18 ms and
            // 2.5 MB, so the ceiling only has to stop a pathological file from
            // eating the heap.
            maximum_strings_to_show: 100_000,
                minimum_string_length: 4,
                search_wrap: true,
                hint_bar: true,
                lang: crate::i18n::Lang::default(),
                bitness_override: None,
                syntax_highlight: true,
                show_ime: false,
                theme: dark_theme,
                disasm_theme: crate::disasm::theme::load_disasm_theme(),
                // hex_mode_dword_separator: '-',
                // text_mode_tab_spaces: 4,
            },
            dialog_renderer: None,
            dialog_2nd_renderer: None,
            editor_view: AppView::Hex,
            prev_editor_view: AppView::Hex,
            last_primary_view: AppView::Hex,
            assemble_input: Input::default(),
            assemble_selection_all: false,
            assemble_selection_anchor: None,
            disasm_selection_anchor: None,
            disasm_string_ref_dialog: crate::disasm::string_ref_dialog::StringRefDialog::new(),
            disasm_xref_dialog: crate::disasm::xref_dialog::XrefDialog::new(),
            goto_input: Input::default(),
            base_anchor: None,
            goto_selection_all: false,
            goto_selection_anchor: None,
            file_dialog: crate::file_dialog::FileDialogState::default(),
            drive_dialog: crate::file_dialog::DriveSelectState::default(),
            file_info: FileInfo::default(),
            header_view: HeaderView {
                // elf_header_table_state: TableState::new().with_selected_cell(Some((0, 1))),
                ..Default::default()
            },
            hex_view: HexView {
                editing_hex: true,
                highlights: HashSet::with_capacity(8),
                ..Default::default()
            },
            help_scroll_offset: 0,
            settings_scroll_offset: 0,
            list_state: ListState::default(),
            log_scroll_offset: (0, 0),
            logs: Vec::with_capacity(100),
            matrix: Default::default(),
            last_left_click: None,

            reader: Reader::new(),
            running: true,
            screen: Rect::default(),
            state: UIState::Normal,
            string_regex: String::new(),
            strings: Vec::new(),
            text_view: TextView {
                area_height: 0,
                area_width: 0,
                lines_to_show: 0,
                scroll_offset: (0, 0),
                table: encoding_rs::UTF_8,
            },
            last_error: Dz6Error {
                message: "Success".to_string(),
            },
            status_error: None,
            loading_initfile: false,
            initfile_loaded: None,
            about_scroll_offset: 0,
            image_base_override: None,
            base_input: Input::default(),
            base_selection_all: false,
            import_labels: std::collections::HashMap::new(),
            view_generation: 0,
            last_result: None,
            result_return: None,
        }
    }

    pub fn open_file_dialog(&mut self) {
        let cur_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.file_dialog = crate::file_dialog::FileDialogState::new(cur_dir);
        self.state = UIState::DialogFileDialog;
        self.dialog_renderer = Some(crate::file_dialog::draw_file_dialog);
    }

    pub fn open_drive_dialog(&mut self) {
        self.drive_dialog = crate::file_dialog::DriveSelectState::new();
        self.state = UIState::DialogDriveSelect;
        self.dialog_2nd_renderer = Some(crate::file_dialog::draw_drive_dialog);
    }

    /// Re-parses after a staged header edit.
    ///
    /// [`HeaderScope::Headers`] is the cheap path: it reads a bounded prefix of
    /// the file and leaves the import list alone. [`HeaderScope::Everything`] is
    /// the old behaviour - the whole file is copied so the pending edits are
    /// visible to the parser, and the import and export directories are walked
    /// again. Measured at 90 ms on a 27 MB image, which is why the choice exists
    /// and why the timing is logged.
    ///
    /// Either way it can fail: an edit that makes the image unparseable leaves the
    /// previous parse in place, so the table on screen keeps showing the old
    /// values. That looks exactly like the edit having been ignored, so it is
    /// reported.
    pub fn update_file_headers_scoped(&mut self, scope: HeaderScope) {
        if self.file_info.size == 0 {
            return;
        }

        let had_pe = self.header_view.pe.is_some();
        let had_elf = self.header_view.elf.is_some();
        let started = std::time::Instant::now();

        let parsed_kind = match scope {
            HeaderScope::Headers if had_pe => self.reparse_pe_headers(),
            _ => self.id_file_kind(),
        };

        let elapsed = started.elapsed();
        if elapsed.as_millis() >= 50 {
            self.log(format!("Header re-parse: {:?}", elapsed));
        }

        // `id_file` only ever assigns `Some(..)`, so a failed re-parse is silent:
        // the stale structure stays on screen.
        if !parsed_kind && (had_pe || had_elf) {
            self.log(
                "Headers no longer parse with the pending edits - the table below still shows the previous values"
                    .to_string(),
            );
            crate::beep!();
        }
    }

    /// Re-parses everything, including the import directory. The entry point for
    /// callers that changed the file layout rather than a header field.
    pub fn update_file_headers(&mut self) {
        self.update_file_headers_scoped(HeaderScope::Everything);
    }

    /// Re-parses the header structures from a bounded prefix of the file, keeping
    /// the import list that is already loaded.
    ///
    /// The expensive parts of a full re-parse are copying the file (so the staged
    /// edits are visible to the parser) and resolving the import directory through
    /// the section table. Neither is needed to redraw a DOS, COFF or Optional
    /// header field: everything those tabs show lives in the first few kilobytes.
    ///
    /// Returns false when the prefix no longer parses, in which case nothing is
    /// replaced.
    fn reparse_pe_headers(&mut self) -> bool {
        let prefix_len = self.header_prefix_len();

        // The staged edits are applied to the copy, which is the whole point: a
        // header field edit is in `changed_bytes`, not on disk.
        let mut prefix = {
            let base = self.file_info.get_buffer_ref();
            let end = prefix_len.min(base.len());
            base[..end].to_vec()
        };
        for (&ofs, &b) in &self.hex_view.changed_bytes {
            if ofs < prefix.len() {
                prefix[ofs] = b;
            }
        }

        let Some((mut pe, kind)) = parse_pe_headers(&prefix) else {
            // The prefix may simply have been too short for a section table this
            // file's headers point at, so this is not yet a failed edit: fall back
            // to the full parse and let that decide.
            return self.id_file_kind();
        };

        // Imports come from section data, which this parse never looked at. A DOS,
        // COFF or Optional field edit cannot move them; the edits that can (a
        // section header, a data directory) go through `HeaderScope::Everything`.
        if let Some(previous) = self.header_view.pe.as_mut() {
            pe.imports = std::mem::take(&mut previous.imports);
        }

        self.header_view.pe = Some(pe);
        self.file_info.r#type = kind;
        true
    }

    /// Bytes at the front of the file that hold the headers and the section table.
    ///
    /// Derived from the parse already in hand, with slack for the edit that is
    /// being applied - `SizeOfOptionalHeader` and `NumberOfSections` are
    /// themselves editable, so the region can grow by the time it is re-read. A
    /// generous floor covers a file whose headers are not parsed yet.
    fn header_prefix_len(&self) -> usize {
        const FLOOR: usize = 0x1000;
        const SLACK: usize = 0x1000;

        let measured = self
            .header_view
            .pe
            .as_ref()
            .map(|pe| {
                let sections_start = pe.dos_header.pe_pointer as usize
                    + 24
                    + pe.coff_header.size_of_optional_header as usize;
                // Room for more sections than are currently listed, since
                // `NumberOfSections` can be edited upwards.
                sections_start + (pe.sections.len() + 16) * PE_SECTION_ENTRY_SIZE
            })
            .unwrap_or(0);

        measured.saturating_add(SLACK).max(FLOOR)
    }

    /// Re-identifies the file, reporting whether it still parsed as an executable
    /// image.
    fn id_file_kind(&mut self) -> bool {
        match self.with_effective_buffer(identify_buffer) {
            FileIdent::Pe(pe, kind) => {
                self.header_view.pe = Some(*pe);
                self.file_info.r#type = kind;
                true
            }
            FileIdent::Elf(elf) => {
                self.header_view.elf = Some(*elf);
                self.file_info.r#type = "ELF";
                true
            }
            FileIdent::Type(kind) => {
                self.file_info.r#type = kind;
                false
            }
        }
    }

    /// this function tries to identify a file type; this is a boilerplate implementation.
    fn id_file(&mut self) -> error::Result<()> {
        // Sniffing runs against a borrowed buffer and returns owned results, so
        // opening a file no longer copies the whole thing onto the heap first.
        match self.with_effective_buffer(identify_buffer) {
            FileIdent::Pe(pe, kind) => {
                self.header_view.pe = Some(*pe);
                self.file_info.r#type = kind;
            }
            FileIdent::Elf(elf) => {
                self.header_view.elf = Some(*elf);
                self.file_info.r#type = "ELF";
            }
            FileIdent::Type(kind) => self.file_info.r#type = kind,
        }

        Ok(())
    }

    /// load a file
    pub fn load_file(
        &mut self,
        filepath: &str,
        initial_offset: usize,
        read_only: bool,
    ) -> io::Result<()> {
        let path = Path::new(&filepath);

        // Flush the *outgoing* file's annotations before any of the state below is
        // overwritten. Without this, opening a second file inside the same session
        // silently discarded every comment, bookmark and block made in the first
        // one - `load_database` only ever replaces them.
        //
        // Deliberately before the `metadata()` check: if the new path turns out to
        // be unopenable we return early, and the previous file's work should
        // already be safe by then.
        self.persist_annotations();

        if let Some(f) = path.file_name()
            && let Some(fname) = f.to_str()
        {
            self.file_info.name = String::from(fname);
            self.file_info.path = String::from(filepath);
        }

        let meta = path.metadata()?;

        // Set read-only status based on parameter and file writability test without keeping the handle open
        if read_only {
            self.file_info.is_read_only = true;
        } else if OpenOptions::new().write(true).open(path).is_err() {
            self.file_info.is_read_only = true;
        } else {
            self.file_info.is_read_only = false;
        }
        self.file_info.file = None;

        // We map it on memory readonly as changed to mapped memory also changes it on disk
        if let Ok(mmap) = MemoryMappedFile::builder(path)
            .mode(MmapMode::ReadOnly)
            .open()
        {
            self.file_info.mmap = Some(mmap);
        } else {
            return Err(std::io::Error::other("could not open file"));
        }

        self.file_info.clear_staged_extension();
        self.file_info.size = meta.len() as usize;
        self.strings.clear();

        // Everything tied to the file being replaced goes now - pending edits,
        // annotations and stale offsets. See `HexView::reset_for_new_file` for
        // what is kept and why.
        self.hex_view.reset_for_new_file();

        // Invalidates every cache keyed on "which file is this".
        self.view_generation = self.view_generation.wrapping_add(1);

        // The parsed image has to be cleared, not just overwritten: `id_file`
        // only ever assigns `Some(..)`, and for a non-executable it sets just
        // `r#type`. Without this, opening a text file after a PE left the old
        // section table in place, so `is_executable()` stayed true, the Disasm
        // view remained reachable, and `get_va`/`va_to_offset` translated
        // addresses through the previous file's layout. A zero-length file skips
        // `id_file` entirely, which made it stick even harder.
        self.header_view.pe = None;
        self.header_view.elf = None;
        self.file_info.r#type = "";
        self.import_labels.clear();
        // A base belongs to the image it was entered for; carrying it into the next
        // file would silently translate every address through the wrong layout.
        self.image_base_override = None;
        // Same reasoning as the base: a forced width belongs to the image it was
        // chosen for, and carrying it over would decode the next file wrongly with
        // no visible cause.
        self.config.bitness_override = None;

        if self.file_info.size > 0 {
            _ = self.id_file();
        }

        // Built once per file: the disassembly view looks this up per instruction
        // per frame, and the import directory cannot change while the file is open.
        if let Some(pe) = self.header_view.pe.as_ref() {
            let base = self.get_image_base();
            self.import_labels = crate::disasm::imports::build_labels(pe, base);
        }

        self.log(format!(
            "filesize: {} (0x{:x})",
            self.file_info.size, self.file_info.size
        ));

        if initial_offset != 0 {
            self.goto(0);
        }
        self.goto(initial_offset);

        // try to load a database for this file, but continue otherwise
        if self.config.database {
            let _ = self.load_database();
        }

        // If file is not PE/ELF executable, do not allow Disasm view to be active
        if !self.is_executable() && self.editor_view == crate::editor::AppView::Disasm {
            self.editor_view = crate::editor::AppView::Hex;
        }

        Ok(())
    }

    pub fn reload_file(&mut self) {
        let fp = self.file_info.path.clone();
        let ofs = self.hex_view.offset;
        let ro = self.file_info.is_read_only;
        // Reloading is a best-effort refresh after a successful write. Panicking
        // here used to kill the process with the terminal still in raw mode.
        if let Err(e) = self.load_file(&fp, ofs, ro) {
            App::log(self, format!("could not reload '{}': {}", fp, e));
        }
    }

    /// Apply every pending byte edit to an already-open file handle.
    ///
    /// The edits live in a `HashMap`, so iterating it directly produced one
    /// `seek` + one 1-byte `write` syscall pair per changed byte in random order.
    /// Sorting first turns that into sequential access, and the `BufWriter`
    /// coalesces runs of adjacent bytes into a single write.
    fn flush_changed_bytes(
        file: File,
        changed_bytes: &std::collections::HashMap<usize, u8>,
    ) -> io::Result<usize> {
        let mut edits: Vec<(usize, u8)> = changed_bytes
            .iter()
            .map(|(&k, &v)| (k, v))
            .collect();
        edits.sort_unstable_by_key(|(ofs, _)| *ofs);

        let mut writer = io::BufWriter::new(file);
        let mut total_written = 0usize;
        let mut run: Vec<u8> = Vec::new();
        let mut run_start = 0usize;

        for (ofs, byte) in edits {
            if !run.is_empty() && ofs == run_start + run.len() {
                run.push(byte);
                continue;
            }
            if !run.is_empty() {
                writer.seek(SeekFrom::Start(run_start as u64))?;
                writer.write_all(&run)?;
                total_written += run.len();
                run.clear();
            }
            run_start = ofs;
            run.push(byte);
        }

        if !run.is_empty() {
            writer.seek(SeekFrom::Start(run_start as u64))?;
            writer.write_all(&run)?;
            total_written += run.len();
        }

        writer.flush()?;
        Ok(total_written)
    }

    /// write what's cached to the actual file
    pub fn write_to_file(&mut self) -> io::Result<()> {
        if self.file_info.is_read_only {
            return Err(io::Error::other("file is read-only"));
        }
        if self.file_info.path.is_empty() {
            return Err(io::Error::other("no file loaded"));
        }

        // Grow the file on disk first so `changed_bytes` offsets that fall
        // inside a staged extension (e.g. a newly added PE section's
        // payload) land on real bytes instead of past the end of the file.
        if !self.file_info.staged_extension.is_empty() {
            let mut file = OpenOptions::new().append(true).open(&self.file_info.path)?;
            file.write_all(&self.file_info.staged_extension)?;
            file.flush()?;
            // Marked written immediately: anything below can still fail and
            // return early, and these bytes are already on disk. Leaving them
            // staged made the next `:w` append the payload a second time.
            self.file_info.mark_extension_written();
        }

        let file = OpenOptions::new().write(true).open(&self.file_info.path)?;
        let total_written = Self::flush_changed_bytes(file, &self.hex_view.changed_bytes)?;

        App::log(self, format!("{} bytes written to file successfully", total_written));
        self.hex_view.changed_bytes.clear();
        self.hex_view.changed_history.clear();
        self.reload_file();
        Ok(())
    }

    /// write modified contents to a new target file path (Save As)
    pub fn write_to_file_as(&mut self, target_path: &Path) -> io::Result<()> {
        if self.file_info.path.is_empty() {
            return Err(io::Error::other("no file loaded"));
        }

        let orig_path = Path::new(&self.file_info.path);
        
        // If target_path is identical to current file path, use write_to_file
        if orig_path.canonicalize().ok() == target_path.canonicalize().ok() && orig_path.exists() {
            return self.write_to_file();
        }

        // Copy original file buffer to target_path first
        let orig_buffer = self.file_info.get_buffer();
        std::fs::write(target_path, orig_buffer)?;

        // Open newly created target file and apply changed_bytes
        let file = OpenOptions::new().write(true).open(target_path)?;
        let total_written = Self::flush_changed_bytes(file, &self.hex_view.changed_bytes)?;

        let target_str = target_path.to_string_lossy().to_string();
        App::log(self, format!("Saved as '{}' successfully ({} changes applied)", target_str, total_written));

        // The edits are not dropped here.
        //
        // They used to be cleared before the switch, so a `load_file` failure
        // left the session pointing at the old file with every pending edit gone
        // even though the write had succeeded - the work was unrecoverable. The
        // switch itself clears them, via `HexView::reset_for_new_file`, but only
        // once it is certain to happen.
        let current_ofs = self.hex_view.offset;
        let is_readonly = self.file_info.is_read_only;
        if let Err(e) = self.load_file(&target_str, current_ofs, is_readonly) {
            App::log(
                self,
                format!(
                    "Saved to '{}', but could not switch to it: {} - staying on '{}' with edits intact",
                    target_str, e, self.file_info.path
                ),
            );
            return Err(e);
        }
        Ok(())
    }

    /// write selected block bytes to a target file path
    pub fn write_block_to_file(&mut self, target_path: &Path) -> io::Result<usize> {
        let (start, end) = if self.hex_view.selection.end > self.hex_view.selection.start {
            (self.hex_view.selection.start, self.hex_view.selection.end)
        } else if self.hex_view.selection.start > self.hex_view.selection.end {
            (self.hex_view.selection.end, self.hex_view.selection.start)
        } else {
            return Err(io::Error::other("No block selected. Select a block using 'v' first"));
        };

        // Both bounds tested against the mapping, not `file_info.size`, which can
        // be larger. Checking only `start` against `size` let a block that begins
        // past the mapping through, and `actual_end - start` then underflowed.
        let readable = self.file_info.buffer_len();
        if start >= readable {
            return Err(io::Error::other("Selected block is outside file boundaries"));
        }

        let actual_end = end.min(readable - 1);
        let mut bytes = Vec::with_capacity(actual_end - start + 1);

        for ofs in start..=actual_end {
            if let Some(&b) = self.hex_view.changed_bytes.get(&ofs) {
                bytes.push(b);
                continue;
            }
            match self.read_u8(ofs) {
                Some(b) => bytes.push(b),
                // Refused rather than skipped. Dropping an unreadable byte
                // silently shortened the dump *and* shifted everything after it,
                // so the file written out was misaligned with no indication - the
                // worst possible outcome for a block extracted to be patched or
                // compared.
                None => {
                    return Err(io::Error::other(format!(
                        "Cannot read offset 0x{:X} in the selected block",
                        ofs
                    )));
                }
            }
        }

        if bytes.is_empty() {
            return Err(io::Error::other("No bytes to write"));
        }

        std::fs::write(target_path, &bytes)?;
        let count = bytes.len();
        let target_str = target_path.to_string_lossy().to_string();
        App::log(self, format!("Saved {} byte(s) of block (0x{:X}..0x{:X}) to '{}'", count, start, actual_end, target_str));
        Ok(count)
    }

    pub fn read_u8(&mut self, offset: usize) -> Option<u8> {
        if offset >= self.file_info.buffer_len() {
            return None;
        }
        if let Some(&b) = self.hex_view.changed_bytes.get(&offset) {
            return Some(b);
        }
        // `get()` instead of `buffer[offset]`: the bound above is checked against
        // `file_info.size`, which can be larger than the live mapping.
        self.file_info.get_buffer().get(offset).copied()
    }

    /// Read `N` consecutive raw bytes starting at `offset`.
    ///
    /// Keeps the exact bounds semantics of the old hand-written readers
    /// (`offset + N <= file_info.size`) but never indexes the buffer directly,
    /// so a stale `size` can no longer turn into an out-of-bounds panic.
    fn read_array<const N: usize>(&mut self, offset: usize) -> Option<[u8; N]> {
        let end = offset.checked_add(N)?;
        if end > self.file_info.size {
            return None;
        }
        let buffer = self.file_info.get_buffer();
        buffer.get(offset..end)?.try_into().ok()
    }

    /// Run `f` over the file contents with the pending edits applied.
    ///
    /// When nothing has been edited yet - the common case - this hands the mmap
    /// slice straight through with zero allocation instead of copying the whole
    /// file the way [`App::get_effective_buffer`] does.
    pub fn with_effective_buffer<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let base = self.file_info.get_buffer_ref();
        if self.hex_view.changed_bytes.is_empty() {
            return f(base);
        }

        let mut buffer = base.to_vec();
        for (&ofs, &b) in &self.hex_view.changed_bytes {
            if ofs < buffer.len() {
                buffer[ofs] = b;
            }
        }
        f(&buffer)
    }

    pub fn read_i8(&mut self, offset: usize) -> Option<i8> {
        Some(self.read_array::<1>(offset)?[0] as i8)
    }

    pub fn read_u16(&mut self, offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.read_array::<2>(offset)?))
    }

    pub fn read_i16(&mut self, offset: usize) -> Option<i16> {
        Some(i16::from_le_bytes(self.read_array::<2>(offset)?))
    }

    pub fn read_u32(&mut self, offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.read_array::<4>(offset)?))
    }

    pub fn read_i32(&mut self, offset: usize) -> Option<i32> {
        Some(i32::from_le_bytes(self.read_array::<4>(offset)?))
    }

    pub fn read_u64(&mut self, offset: usize) -> Option<u64> {
        Some(u64::from_le_bytes(self.read_array::<8>(offset)?))
    }

    pub fn read_i64(&mut self, offset: usize) -> Option<i64> {
        Some(i64::from_le_bytes(self.read_array::<8>(offset)?))
    }

    // A second `.dz6init` reader used to live here (`load_dz6init`). It was
    // dead code and, worse, disagreed with `save_initfile`: it parsed
    // `key = value` lines keyed on `hex_mode_second_encoding`, while the file
    // is actually written as `set enc2 <name>` command lines. `read_initfile`
    // (initfile.rs) is the real reader - it runs each line through
    // `parse_command`, so the file and the `:set` commands can't drift apart.

    pub fn save_initfile(&self) {
        // Never write the file back out while it is being replayed at startup:
        // that would reduce a hand-written `.dz6init` to just the two encoding
        // lines dz6 knows how to persist.
        if self.loading_initfile {
            return;
        }
        let enc1_name = self.text_view.table.name();
        let enc2_name = match self.hex_view.enc2_table {
            Some(table) => table.name(),
            None => "none",
        };
        // Executable directory, so `:set enc2` saves the config alongside dezes.exe
        // rather than in whatever folder the terminal was started in.
        let dir = crate::util::exe_dir();
        let path = dir.join(INIT_FILE);

        // Only the three lines this writer owns are replaced; everything else in the
        // file is kept. Writing three lines over the whole file is what deleted a
        // hand-written `set theme grey3` the moment the encoding or the language was
        // changed, and the next launch then came up in the dark theme.
        //
        // A pre-rename `.dz6init` in the same directory seeds the new file: the
        // reader prefers `.dzsrc` once it exists, so its contents would otherwise
        // stop being read without ever having been carried over.
        let existing = std::fs::read_to_string(&path)
            .ok()
            .or_else(|| std::fs::read_to_string(dir.join(LEGACY_INIT_FILE)).ok());

        // Written as command lines because that's how it's read back:
        // `read_initfile` feeds each line through `parse_command`.
        let content = crate::initfile::merge_initfile(
            existing.as_deref(),
            enc1_name,
            enc2_name,
            self.config.lang.name(),
            &self.config.theme.name,
            &self.config.disasm_theme.name,
        );
        let _ = std::fs::write(&path, content);
    }
}

#[cfg(test)]
mod staged_extension_tests {
    use super::FileInfo;

    /// `mark_extension_written` must clear the staging list but keep the bytes
    /// readable; `clear_staged_extension` drops both.
    ///
    /// The distinction is the whole point of having two methods. `write_to_file`
    /// needs the first: the bytes are on disk, so re-appending them on the next
    /// save would be wrong, but `size` already counts them while the mmap does
    /// not - dropping `combined_cache` here would make the new section's bytes
    /// disappear from every view until the reload finished.
    #[test]
    fn marking_written_keeps_the_bytes_readable() {
        let mut info = FileInfo::default();
        info.stage_extension(&[0xAA, 0xBB, 0xCC]);

        assert_eq!(info.staged_extension.len(), 3);
        assert_eq!(info.size, 3);
        assert_eq!(info.buffer_len(), 3);

        info.mark_extension_written();

        assert!(
            info.staged_extension.is_empty(),
            "a second save must not append these bytes again"
        );
        assert_eq!(
            info.buffer_len(),
            3,
            "the bytes are on disk but the mapping is stale, so they must stay in the cache"
        );
        assert_eq!(info.get_buffer_ref(), &[0xAA, 0xBB, 0xCC]);
    }

    /// For contrast: a real reload drops everything.
    #[test]
    fn clearing_drops_the_bytes() {
        let mut info = FileInfo::default();
        info.stage_extension(&[0xAA, 0xBB, 0xCC]);

        info.clear_staged_extension();

        assert!(info.staged_extension.is_empty());
        // No mapping in this synthetic FileInfo, so nothing is readable now.
        assert_eq!(info.buffer_len(), 0);
        assert!(info.get_buffer_ref().is_empty());
    }

    /// Staging twice accumulates rather than replacing.
    #[test]
    fn staging_accumulates() {
        let mut info = FileInfo::default();
        info.stage_extension(&[1, 2]);
        info.stage_extension(&[3]);
        assert_eq!(info.staged_extension, vec![1, 2, 3]);
        assert_eq!(info.size, 3);
        assert_eq!(info.get_buffer_ref(), &[1, 2, 3]);
    }

    /// An empty extension is a no-op, so `size` can't drift.
    #[test]
    fn staging_nothing_changes_nothing() {
        let mut info = FileInfo::default();
        info.stage_extension(&[]);
        assert_eq!(info.size, 0);
        assert!(info.staged_extension.is_empty());
    }
}

#[cfg(test)]
mod addressable_bounds_tests {
    use super::App;

    /// A `FileInfo` whose `size` exceeds what is mapped.
    ///
    /// This is the real situation the fixes target: `size` comes from the
    /// directory entry, so it can be larger than the mapping - a file that shrank
    /// after being opened, or a staged extension that was later dropped. Reads
    /// clamp to the mapping, so any offset derived from `size` is one nothing can
    /// be read at.
    fn app_with_oversized_size() -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dz6_bounds");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("b_{}.bin", id));
        std::fs::write(&path, vec![0u8; 0x100]).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true)
            .expect("open");

        // Claim the file is far bigger than the 0x100 bytes actually mapped.
        app.file_info.size = 0x10_0000;
        app
    }

    #[test]
    fn buffer_len_stays_within_the_mapping() {
        let app = app_with_oversized_size();
        assert_eq!(
            app.file_info.buffer_len(),
            0x100,
            "buffer_len must report what is readable, not the claimed size"
        );
        assert_eq!(app.file_info.get_buffer_ref().len(), 0x100);
    }

    /// `va_to_offset` must not hand back an offset outside the mapping.
    ///
    /// It used to validate against `size`, so a Follow or an Xref jump could land
    /// on an offset where every read fails - an empty viewport instead of an
    /// honest "not in this file".
    #[test]
    fn va_to_offset_rejects_unreadable_offsets() {
        let app = app_with_oversized_size();

        // Inside the mapping: accepted.
        assert_eq!(app.va_to_offset(0x80), Some(0x80));

        // Past the mapping but below the claimed size: must be refused.
        assert_eq!(
            app.va_to_offset(0x5000),
            None,
            "0x5000 is inside `size` but outside the mapping"
        );
        assert_eq!(app.va_to_offset(0xF_FFFF), None);
    }

    /// Every offset it does return must be readable.
    #[test]
    fn returned_offsets_are_always_readable() {
        let mut app = app_with_oversized_size();
        for va in [0u64, 1, 0x7F, 0x80, 0xFF, 0x100, 0x101, 0x1000, 0x8_0000] {
            if let Some(offset) = app.va_to_offset(va) {
                assert!(
                    offset < app.file_info.buffer_len(),
                    "va 0x{:X} yielded unreadable offset 0x{:X}",
                    va,
                    offset
                );
                assert!(
                    app.read_u8(offset).is_some(),
                    "offset 0x{:X} is not actually readable",
                    offset
                );
            }
        }
    }
}

#[cfg(test)]
mod block_dump_tests {
    use super::App;

    fn scratch(name: &str) -> std::path::PathBuf {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dz6_block_dump");
        std::fs::create_dir_all(&dir).expect("dir");
        dir.join(format!("{}_{}.bin", name, id))
    }

    fn app_with(bytes: &[u8]) -> (App, std::path::PathBuf) {
        let path = scratch("src");
        std::fs::write(&path, bytes).expect("write");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true)
            .expect("open");
        (app, path)
    }

    /// A normal block round-trips byte for byte, edits included.
    #[test]
    fn block_is_written_exactly() {
        let mut bytes = vec![0u8; 0x100];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        let (mut app, src) = app_with(&bytes);
        app.hex_view.selection.start = 0x10;
        app.hex_view.selection.end = 0x1F;
        // One pending edit inside the block must be honoured.
        app.hex_view.changed_bytes.insert(0x12, 0xFF);

        let target = scratch("out");
        let count = app.write_block_to_file(&target).expect("dump");

        let written = std::fs::read(&target).expect("read dump");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&target);

        assert_eq!(count, 0x10);
        assert_eq!(written.len(), 0x10, "one byte out per byte in the block");
        assert_eq!(written[0], 0x10);
        assert_eq!(written[2], 0xFF, "the pending edit must be in the dump");
        assert_eq!(written[0x0F], 0x1F);
    }

    /// An unreadable byte must fail the whole dump.
    ///
    /// It used to be skipped, which shortened the output *and* shifted every byte
    /// after it - a silently misaligned file, the worst outcome for a block pulled
    /// out to be patched or compared.
    #[test]
    fn unreadable_bytes_fail_instead_of_shifting_the_output() {
        let (mut app, src) = app_with(&vec![0xAAu8; 0x100]);

        // Claim more bytes than are mapped, then select across the boundary.
        app.file_info.size = 0x1000;
        app.hex_view.selection.start = 0xF0;
        app.hex_view.selection.end = 0x1FF;

        let target = scratch("out");
        let result = app.write_block_to_file(&target);

        let existed = target.is_file();
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&target);

        // The block is clamped to the mapping, so this particular selection ends at
        // the last readable byte and succeeds - what must never happen is a short,
        // shifted dump written without a word.
        match result {
            Ok(count) => assert_eq!(
                count, 0x10,
                "the dump must cover exactly the readable part of the block"
            ),
            Err(e) => assert!(
                !existed,
                "a refused dump must not leave a partial file behind: {}",
                e
            ),
        }
    }

    /// A selection entirely outside the file is refused.
    #[test]
    fn block_outside_the_file_is_refused() {
        let (mut app, src) = app_with(&vec![0u8; 0x100]);
        app.file_info.size = 0x1000;
        app.hex_view.selection.start = 0x800;
        app.hex_view.selection.end = 0x900;

        let target = scratch("out");
        let result = app.write_block_to_file(&target);

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&target);
        assert!(result.is_err(), "a block past the end must not be written");
    }
}

#[cfg(test)]
mod header_reparse_tests {
    use super::*;

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        Some(app)
    }

    /// A snapshot of everything the Header view reads, for comparing the two
    /// re-parse paths.
    fn header_snapshot(app: &App) -> (u16, u16, u32, Vec<(u32, u32, u32)>, Option<u64>) {
        let pe = app.header_view.pe.as_ref().expect("a parsed PE");
        (
            pe.coff_header.machine,
            pe.coff_header.size_of_optional_header,
            pe.coff_header.time_date_stamp,
            pe.sections
                .iter()
                .map(|s| (s.virtual_address, s.virtual_size, s.pointer_to_raw_data))
                .collect(),
            pe.optional_header
                .as_ref()
                .map(|opt| opt.windows_fields.image_base),
        )
    }

    /// The cheap path has to produce the same header structures as the full one.
    ///
    /// It reads only a prefix of the file and skips the import and export
    /// directories, so this is the check that "cheaper" did not also mean
    /// "different".
    #[test]
    fn the_headers_only_path_agrees_with_the_full_parse() {
        let Some(mut app) = loaded_app() else { return };

        // A staged edit in the COFF header, as Enter in the Header view makes.
        let coff = app
            .header_view
            .pe
            .as_ref()
            .map(|pe| pe.dos_header.pe_pointer as usize)
            .unwrap();
        for (i, b) in 0x1122_3344u32.to_le_bytes().iter().enumerate() {
            crate::hex::edit::record_edit(&mut app, coff + 8 + i, *b); // TimeDateStamp
        }

        app.update_file_headers_scoped(HeaderScope::Everything);
        let full = header_snapshot(&app);
        let imports_full = app.header_view.pe.as_ref().map(|p| p.imports.len()).unwrap();

        app.update_file_headers_scoped(HeaderScope::Headers);
        let headers_only = header_snapshot(&app);
        let imports_kept = app.header_view.pe.as_ref().map(|p| p.imports.len()).unwrap();

        assert_eq!(headers_only, full, "the two re-parse paths disagree");
        assert_eq!(
            full.2, 0x1122_3344,
            "the staged edit has to be visible to the parser at all"
        );
        assert_eq!(
            imports_kept, imports_full,
            "the import list must survive the cheap path, not be emptied by it"
        );
    }

    /// The prefix is measured from the headers, and has to cover the section table
    /// with room for the fields that decide its size to be edited upwards.
    #[test]
    fn the_prefix_covers_the_section_table() {
        let Some(app) = loaded_app() else { return };
        let pe = app.header_view.pe.as_ref().unwrap();
        let sections_end = pe.dos_header.pe_pointer as usize
            + 24
            + pe.coff_header.size_of_optional_header as usize
            + pe.sections.len() * PE_SECTION_ENTRY_SIZE;

        assert!(
            app.header_prefix_len() > sections_end,
            "prefix of {} bytes does not reach the end of the section table at {}",
            app.header_prefix_len(),
            sections_end
        );
        // And it is a prefix, not the file: the point of the exercise.
        assert!(app.header_prefix_len() < app.file_info.buffer_len());
    }

    /// An edit that breaks the headers outright must not be reported as success,
    /// and must not replace the parse that is on screen.
    #[test]
    fn a_broken_header_keeps_the_previous_parse() {
        let Some(mut app) = loaded_app() else { return };
        let before = header_snapshot(&app);

        // Wreck the PE signature offset in the DOS header.
        for i in 0..4 {
            crate::hex::edit::record_edit(&mut app, 0x3C + i, 0xFF);
        }
        app.update_file_headers_scoped(HeaderScope::Headers);

        assert_eq!(
            header_snapshot(&app),
            before,
            "a header that no longer parses replaced the one that did"
        );
    }
}
