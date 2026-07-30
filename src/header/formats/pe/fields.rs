//! The DOS / COFF / Optional Header field tables, in one place.
//!
//! These three tabs are plain key-value lists, and three separate copies of the
//! list used to exist: `draw.rs` built the rows on screen, the Enter handler in
//! `events.rs` built its own array to turn `detail_index` into a file offset, and
//! the `g`/`f` jump handler built a third one of bare offsets.
//!
//! They drifted, as duplicated tables do. The COFF list on screen starts with
//! `PE_Signature` and the edit list did not, so every COFF row edited the field
//! *below* the highlighted one: selecting `PointerToSymbolTable` opened
//! `NumberOfSymbols`, and the value the user typed landed in a field they were not
//! looking at - which reads exactly like "my edits are not applied".
//!
//! One table, three consumers. Adding a field means adding it once.

use crate::header::header_view::Pe;

/// One row of a key-value header tab.
pub struct KvField {
    pub offset: usize,
    pub size: usize,
    pub name: String,
    /// Decoded value as shown in the table.
    pub value: String,
    /// False for rows that are text or a blob rather than a number, so the edit
    /// dialog can refuse them instead of writing a little-endian integer over
    /// something that is not one.
    pub editable: bool,
}

impl KvField {
    fn num(offset: usize, size: usize, name: &str, value: String) -> Self {
        Self { offset, size, name: name.to_string(), value, editable: true }
    }

    fn blob(offset: usize, size: usize, name: &str, value: &str) -> Self {
        Self { offset, size, name: name.to_string(), value: value.to_string(), editable: false }
    }

    /// A field resolved from one of the cell-based tabs (Data Directories,
    /// Sections), which have no decoded-value column of their own.
    pub fn cell(offset: usize, size: usize, name: String) -> Self {
        Self { offset, size, name, value: String::new(), editable: true }
    }
}

fn machine_string(mach: u16) -> &'static str {
    match mach {
        goblin::pe::header::COFF_MACHINE_ARM => "ARM",
        goblin::pe::header::COFF_MACHINE_ARM64 => "AARCH64",
        goblin::pe::header::COFF_MACHINE_X86 => "Intel386 (x86)",
        goblin::pe::header::COFF_MACHINE_X86_64 => "AMD64 (x86-64)",
        _ => "Unknown",
    }
}

/// Names of the sixteen data directory entries, in index order.
///
/// Indexed by `detail_index`, and the index is what turns into a file offset
/// (`data_directory(idx)` strides eight bytes per entry). Two copies of this list
/// used to exist - one in `draw.rs` for the rows, one in `events.rs` for the edit
/// target - so a reorder in one would have moved the edit somewhere the label did
/// not say.
pub const DATA_DIRECTORY_NAMES: [&str; 16] = [
    "Export Table",
    "Import Table",
    "Resource Table",
    "Exception Table",
    "Certificate Table",
    "Base Relocation Table",
    "Debug Data",
    "Architecture",
    "Global Ptr",
    "TLS Table",
    "Load Config Table",
    "Bound Import",
    "IAT (Import Address Table)",
    "Delay Import",
    "COM+ Runtime Header",
    "Reserved",
];

/// Half-open range of rows to build, keeping `selected` inside it.
///
/// Every detail table used to render its whole list with `render_widget`, which
/// draws from the first row and simply stops at the bottom border: with 289
/// imports and room for 24, rows 25 onwards were unreachable - the selection went
/// on moving invisibly below the table. Windowing is also what keeps the Import
/// Directory tab from building a thousand rows of cloned strings every frame
/// (measured at 3.9 ms a frame, ten times the other tabs).
pub fn visible_window(total: usize, selected: usize, visible_rows: usize) -> (usize, usize) {
    if total == 0 || visible_rows == 0 {
        return (0, 0);
    }
    let selected = selected.min(total - 1);
    let half = visible_rows / 2;
    let start = if selected > half {
        (selected - half).min(total.saturating_sub(visible_rows))
    } else {
        0
    };
    (start, (start + visible_rows).min(total))
}

/// Rows of the `sidebar_index` tab, or an empty Vec for the tabs that are not
/// key-value lists (Data Directories, Sections, Imports, Section Tools).
pub fn kv_fields(pe: &Pe, sidebar_index: usize) -> Vec<KvField> {
    let pe_ptr = pe.dos_header.pe_pointer as usize;
    let mut rows: Vec<KvField> = Vec::new();

    match sidebar_index {
        0 => {
            let d = &pe.dos_header;
            rows.push(KvField::num(0, 2, "DOS_Signature", format!("MZ (0x{:04X})", d.signature)));
            rows.push(KvField::num(2, 2, "DOS_PartPag", format!("{} (0x{:04X})", d.bytes_on_last_page, d.bytes_on_last_page)));
            rows.push(KvField::num(4, 2, "DOS_PageCnt", format!("{} (0x{:04X})", d.pages_in_file, d.pages_in_file)));
            rows.push(KvField::num(6, 2, "DOS_ReloCnt", format!("{}", d.relocations)));
            rows.push(KvField::num(8, 2, "DOS_HdrSize", format!("{}", d.size_of_header_in_paragraphs)));
            rows.push(KvField::num(10, 2, "DOS_MinMem", format!("{}", d.minimum_extra_paragraphs_needed)));
            rows.push(KvField::num(12, 2, "DOS_MaxMem", format!("{} (0x{:04X})", d.maximum_extra_paragraphs_needed, d.maximum_extra_paragraphs_needed)));
            rows.push(KvField::num(14, 2, "DOS_ReloSS", format!("0x{:04X}", d.initial_relative_ss)));
            rows.push(KvField::num(16, 2, "DOS_ExeSP", format!("0x{:04X}", d.initial_sp)));
            rows.push(KvField::num(18, 2, "DOS_ChkSum", format!("0x{:04X}", d.checksum)));
            rows.push(KvField::num(20, 2, "DOS_ExeIP", format!("0x{:04X}", d.initial_ip)));
            rows.push(KvField::num(22, 2, "DOS_ReloCS", format!("0x{:04X}", d.initial_relative_cs)));
            rows.push(KvField::num(24, 2, "DOS_TablOff", format!("0x{:04X}", d.file_address_of_relocation_table)));
            rows.push(KvField::num(26, 2, "DOS_Overlay", format!("{}", d.overlay_number)));
            rows.push(KvField::num(0x3C, 4, "PE_Header_Offset", format!("0x{:08X}", d.pe_pointer)));
            // Text, not a number: editing it as a little-endian integer would
            // write four bytes of garbage over the start of the message.
            rows.push(KvField::blob(0x40, 64, "DOS Stub Message", "\"This program cannot be run in DOS mode.\""));
        }
        1 => {
            let c = &pe.coff_header;
            let coff_off = pe_ptr;
            rows.push(KvField::blob(coff_off, 4, "PE_Signature", "PE\\0\\0 (0x00004550)"));
            rows.push(KvField::num(coff_off + 4, 2, "Machine", format!("{} (0x{:04X})", machine_string(c.machine), c.machine)));
            rows.push(KvField::num(coff_off + 6, 2, "NumberOfSections", format!("{}", c.number_of_sections)));
            rows.push(KvField::num(coff_off + 8, 4, "TimeDateStamp", format!("0x{:08X}", c.time_date_stamp)));
            rows.push(KvField::num(coff_off + 12, 4, "PointerToSymbolTable", format!("0x{:08X}", c.pointer_to_symbol_table)));
            rows.push(KvField::num(coff_off + 16, 4, "NumberOfSymbols", format!("{}", c.number_of_symbol_table)));
            rows.push(KvField::num(coff_off + 20, 2, "SizeOfOptionalHeader", format!("{} (0x{:04X})", c.size_of_optional_header, c.size_of_optional_header)));
            rows.push(KvField::num(coff_off + 22, 2, "Characteristics", format!("0x{:04X}", c.characteristics)));
        }
        2 => {
            let Some(opt) = &pe.optional_header else { return rows };
            let opt_off = pe_ptr + 24;
            // ImageBase and the stack/heap fields sit at different offsets (and
            // widths) in PE32+ than in PE32.
            let lay = super::OptionalHeaderLayout::from_pe(pe);
            let (image_base_off, image_base_size) = lay.image_base();
            let (stack_res_off, size_w) = lay.size_of_stack_reserve();
            let (stack_com_off, _) = lay.size_of_stack_commit();
            let (heap_res_off, _) = lay.size_of_heap_reserve();
            let (heap_com_off, _) = lay.size_of_heap_commit();
            let s = &opt.standard_fields;
            let w = &opt.windows_fields;

            rows.push(KvField::num(opt_off, 2, "Magic", format!("{} (0x{:04X})", if s.magic == super::OptionalHeaderLayout::PE32_PLUS_MAGIC { "PE32+ (64-bit)" } else { "PE32 (32-bit)" }, s.magic)));
            rows.push(KvField::num(opt_off + 2, 1, "MajorLinkerVersion", format!("{}", s.major_linker_version)));
            rows.push(KvField::num(opt_off + 3, 1, "MinorLinkerVersion", format!("{}", s.minor_linker_version)));
            rows.push(KvField::num(opt_off + 4, 4, "SizeOfCode", format!("{} (0x{:08X})", s.size_of_code, s.size_of_code)));
            rows.push(KvField::num(opt_off + 8, 4, "SizeOfInitializedData", format!("{} (0x{:08X})", s.size_of_initialized_data, s.size_of_initialized_data)));
            rows.push(KvField::num(opt_off + 12, 4, "SizeOfUninitializedData", format!("{}", s.size_of_uninitialized_data)));
            rows.push(KvField::num(opt_off + 16, 4, "AddressOfEntryPoint", format!("0x{:08X}", s.address_of_entry_point)));
            rows.push(KvField::num(opt_off + 20, 4, "BaseOfCode", format!("0x{:08X}", s.base_of_code)));
            rows.push(KvField::num(image_base_off, image_base_size, "ImageBase", format!("0x{:X}", w.image_base)));
            rows.push(KvField::num(opt_off + 32, 4, "SectionAlignment", format!("0x{:08X}", w.section_alignment)));
            rows.push(KvField::num(opt_off + 36, 4, "FileAlignment", format!("0x{:08X}", w.file_alignment)));
            rows.push(KvField::num(opt_off + 40, 2, "MajorOperatingSystemVersion", format!("{}", w.major_operating_system_version)));
            rows.push(KvField::num(opt_off + 42, 2, "MinorOperatingSystemVersion", format!("{}", w.minor_operating_system_version)));
            rows.push(KvField::num(opt_off + 44, 2, "MajorImageVersion", format!("{}", w.major_image_version)));
            rows.push(KvField::num(opt_off + 46, 2, "MinorImageVersion", format!("{}", w.minor_image_version)));
            rows.push(KvField::num(opt_off + 48, 2, "MajorSubsystemVersion", format!("{}", w.major_subsystem_version)));
            rows.push(KvField::num(opt_off + 50, 2, "MinorSubsystemVersion", format!("{}", w.minor_subsystem_version)));
            // +52 Win32VersionValue and +64 CheckSum are deliberately not listed:
            // both are reserved-or-computed and were never in the table on screen.
            rows.push(KvField::num(opt_off + 56, 4, "SizeOfImage", format!("{} (0x{:08X})", w.size_of_image, w.size_of_image)));
            rows.push(KvField::num(opt_off + 60, 4, "SizeOfHeaders", format!("{} (0x{:08X})", w.size_of_headers, w.size_of_headers)));
            rows.push(KvField::num(opt_off + 68, 2, "Subsystem", format!("{} (0x{:04X})", match w.subsystem { 2 => "Windows GUI", 3 => "Windows CUI", _ => "Unknown" }, w.subsystem)));
            rows.push(KvField::num(opt_off + 70, 2, "DllCharacteristics", format!("0x{:04X}", w.dll_characteristics)));
            rows.push(KvField::num(stack_res_off, size_w, "SizeOfStackReserve", format!("{} (0x{:X})", w.size_of_stack_reserve, w.size_of_stack_reserve)));
            rows.push(KvField::num(stack_com_off, size_w, "SizeOfStackCommit", format!("{} (0x{:X})", w.size_of_stack_commit, w.size_of_stack_commit)));
            rows.push(KvField::num(heap_res_off, size_w, "SizeOfHeapReserve", format!("{} (0x{:X})", w.size_of_heap_reserve, w.size_of_heap_reserve)));
            rows.push(KvField::num(heap_com_off, size_w, "SizeOfHeapCommit", format!("{} (0x{:X})", w.size_of_heap_commit, w.size_of_heap_commit)));
        }
        _ => {}
    }

    rows
}

#[cfg(test)]
mod tests {
    use crate::app::App;

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        Some(app)
    }

    /// Every row on screen must be reachable by the edit path, at the same index.
    ///
    /// This is the off-by-one that made COFF edits land one field low: the table
    /// had `PE_Signature` at row 0 and the edit list started at `Machine`.
    #[test]
    fn the_edit_path_resolves_the_row_that_is_highlighted() {
        let Some(mut app) = loaded_app() else { return };

        for tab in 0..=2 {
            let expected: Vec<(usize, usize, String)> = {
                let pe = app.header_view.pe.as_ref().unwrap();
                super::kv_fields(pe, tab)
                    .into_iter()
                    .map(|f| (f.offset, f.size, f.name))
                    .collect()
            };
            assert!(!expected.is_empty(), "tab {} has no rows", tab);

            app.header_view.sidebar_index = tab;
            app.header_view.active_pane = crate::header::header_view::HeaderPane::Detail;

            for (idx, (offset, size, name)) in expected.iter().enumerate() {
                app.header_view.detail_index = idx;
                let Some(got) = super::super::events::field_at_cursor(&app) else {
                    panic!("tab {} row {} ({}) resolves to nothing", tab, idx, name);
                };
                assert_eq!(
                    (got.offset, got.size, got.name.as_str()),
                    (*offset, *size, name.as_str()),
                    "tab {} row {} resolves to the wrong field",
                    tab,
                    idx
                );
            }
        }
    }

    /// The blob rows are listed but must not be editable as numbers.
    #[test]
    fn text_rows_are_not_editable() {
        let Some(app) = loaded_app() else { return };
        let pe = app.header_view.pe.as_ref().unwrap();

        let dos = super::kv_fields(pe, 0);
        let stub = dos.iter().find(|f| f.name == "DOS Stub Message").expect("stub row");
        assert!(!stub.editable);

        let coff = super::kv_fields(pe, 1);
        let sig = coff.iter().find(|f| f.name == "PE_Signature").expect("signature row");
        assert!(!sig.editable, "the PE signature is not a number to type over");
    }
}
#[cfg(test)]
mod data_directory_tests {
    /// The name list has to be exactly as long as the directory it labels.
    ///
    /// The index is turned into a file offset with an eight-byte stride, so a list
    /// longer than the array would offer a row that edits past the end of it.
    #[test]
    fn the_names_match_the_directory_count() {
        assert_eq!(
            super::DATA_DIRECTORY_NAMES.len(),
            super::super::events::DATA_DIRECTORY_COUNT,
            "the data directory name list and the entry count disagree"
        );
    }
}