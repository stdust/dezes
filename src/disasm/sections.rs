//! Which byte ranges of a file are code, and what address each maps to.
//!
//! Both scanners that walk a whole file - the cross-reference search and the
//! string-reference scan - need the same answer, and they used to compute it
//! differently:
//!
//! * `string_ref.rs` enumerated the PE's executable sections and started a
//!   decoder at each section's VA, which is correct.
//! * `xref.rs` decoded the file as one flat blob starting at `get_va(0)`. That
//!   makes the decoder's IP advance in step with the *file offset*, so for any
//!   image whose section RVAs differ from their raw offsets - essentially every
//!   PE - every reported address and every branch-target comparison was off by
//!   the section delta. The dialog invented references that don't exist and
//!   missed the ones that do.
//!
//! Keeping the mapping in one place is what stops that from happening again.

use crate::app::App;

/// A run of bytes to disassemble, and the address its first byte lives at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeSection {
    /// File offset of the first byte.
    pub start: usize,
    /// File offset one past the last byte.
    pub end: usize,
    /// Virtual address the byte at `start` is mapped to.
    pub va: u64,
}

impl CodeSection {
    /// Byte length of the region. Only the tests need it today; kept because a
    /// region's size is part of what this type means.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

/// `IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE`.
const EXECUTABLE_CHARACTERISTICS: u32 = 0x2000_0020;

/// Executable regions of the loaded file, in file order.
///
/// Uses the PE image already parsed into `header_view`, rather than re-parsing
/// the buffer with goblin on every scan - that parse was pure duplicated work,
/// since opening the file did it once already.
///
/// Falls back to a single region covering the whole buffer when there is no PE
/// image or it declares no executable section, which is what makes the scanners
/// still work on raw shellcode dumps and on ELF files.
pub fn code_sections(app: &App, buffer_len: usize) -> Vec<CodeSection> {
    let mut sections = Vec::new();

    if let Some(pe) = app.header_view.pe.as_ref() {
        // Through `get_image_base`, so an override reaches the scanners too. Read
        // straight from the optional header, the cross-reference and string-ref
        // sweeps kept using the file's declared base while the view showed the
        // overridden one, and every address they reported was off by the delta.
        let image_base = app.get_image_base();

        for sec in &pe.sections {
            if sec.characteristics & EXECUTABLE_CHARACTERISTICS == 0 {
                continue;
            }
            let start = sec.pointer_to_raw_data as usize;
            // Clamped to the buffer: both fields come straight out of the file
            // and a corrupt header can point past the end of it.
            let end = (start.saturating_add(sec.size_of_raw_data as usize)).min(buffer_len);
            if start >= buffer_len || start >= end {
                continue;
            }
            sections.push(CodeSection {
                start,
                end,
                va: image_base + sec.virtual_address as u64,
            });
        }
    }

    if sections.is_empty() && buffer_len > 0 {
        sections.push(CodeSection {
            start: 0,
            end: buffer_len,
            va: app.get_va(0),
        });
    }

    sections.sort_unstable_by_key(|s| s.start);
    sections
}

/// `IMAGE_SCN_MEM_EXECUTE`, on its own: a section can be executable without being
/// marked as containing code.
const EXECUTE_FLAG: u32 = 0x2000_0000;

/// Non-executable regions of the file, in file order, with the address each maps
/// to.
///
/// The counterpart of [`code_sections`], for the cross-reference search: on x64 a
/// string's address usually reaches the code through a table of 64-bit pointers in
/// `.rdata` or `.data`, and the address itself never appears in an instruction.
/// Those tables are here.
///
/// Two exclusions, both deliberate:
///
/// * Executable sections. An `imm64` in an instruction is already reported by the
///   code scan, and scanning `.text` for the same bytes would report it twice.
/// * `.reloc`. Relocation blocks are lists of 12-bit offsets packed into 16-bit
///   fields - a pointer-sized value cannot legitimately live there, so any match is
///   a coincidence. This is where a debugger that disassembles everything produces
///   its most confusing false positives.
///
/// Yields nothing when there is no PE image: without a section table there is no
/// way to tell data from code, and reporting the whole file would bury the real
/// hits.
pub fn data_sections(app: &App, buffer_len: usize) -> Vec<CodeSection> {
    let mut sections = Vec::new();

    let Some(pe) = app.header_view.pe.as_ref() else {
        return sections;
    };
    let image_base = app.get_image_base();

    for sec in &pe.sections {
        if sec.characteristics & EXECUTE_FLAG != 0 {
            continue;
        }
        let name = sec.name().unwrap_or("");
        if name == ".reloc" {
            continue;
        }
        let start = sec.pointer_to_raw_data as usize;
        let end = (start.saturating_add(sec.size_of_raw_data as usize)).min(buffer_len);
        if start >= buffer_len || start >= end {
            continue;
        }
        sections.push(CodeSection {
            start,
            end,
            va: image_base + sec.virtual_address as u64,
        });
    }

    sections.sort_unstable_by_key(|s| s.start);
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        Some(app)
    }

    /// For a real PE the code region must be a section, not the whole file, and
    /// its VA must be the section's mapped address rather than
    /// `image_base + file_offset`.
    ///
    /// That difference is the bug this module exists to fix: with the flat
    /// mapping, `.text`'s first instruction was reported at
    /// `image_base + pointer_to_raw_data` instead of
    /// `image_base + virtual_address`.
    #[test]
    fn pe_sections_use_their_virtual_address() {
        let Some(app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        let sections = code_sections(&app, len);

        assert!(!sections.is_empty(), "a PE must yield at least one code section");
        assert!(
            sections.iter().any(|s| s.len() < len),
            "code sections must be narrower than the whole file"
        );

        let pe = app.header_view.pe.as_ref().expect("pe");
        let image_base = pe
            .optional_header
            .as_ref()
            .map(|o| o.windows_fields.image_base)
            .unwrap_or(0);

        for s in &sections {
            let matching = pe
                .sections
                .iter()
                .find(|sec| sec.pointer_to_raw_data as usize == s.start)
                .expect("section must come from the section table");
            assert_eq!(
                s.va,
                image_base + matching.virtual_address as u64,
                "section VA must be image_base + VirtualAddress"
            );
            // The whole point: raw offset and RVA differ, so the flat mapping
            // would have produced a different address.
            if matching.pointer_to_raw_data != matching.virtual_address {
                assert_ne!(
                    s.va,
                    image_base + s.start as u64,
                    "this section proves the flat file-offset mapping was wrong"
                );
            }
        }
    }

    /// Sections must stay inside the buffer even if the header lies.
    #[test]
    fn sections_are_clamped_to_the_buffer() {
        let Some(app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        for s in code_sections(&app, len) {
            assert!(s.start < len);
            assert!(s.end <= len);
            assert!(s.start < s.end);
        }
    }

    /// A non-PE file falls back to one region covering everything.
    #[test]
    fn non_pe_falls_back_to_the_whole_buffer() {
        let mut app = App::new();
        let txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("COPYING");
        if !txt.is_file() {
            return;
        }
        app.load_file(txt.to_str().expect("path"), 0, true).expect("open");
        let len = app.file_info.buffer_len();

        let sections = code_sections(&app, len);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].start, 0);
        assert_eq!(sections[0].end, len);
    }

    /// An empty buffer yields nothing rather than a zero-length region.
    #[test]
    fn empty_buffer_yields_no_sections() {
        let app = App::new();
        assert!(code_sections(&app, 0).is_empty());
    }

    /// Data regions must be the sections code is *not* in, and must not overlap
    /// them.
    #[test]
    fn data_sections_are_the_non_executable_ones() {
        let Some(app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        let code = code_sections(&app, len);
        let data = data_sections(&app, len);

        assert!(!data.is_empty(), "a PE has data sections");
        for d in &data {
            assert!(d.start < len && d.end <= len && d.start < d.end);
            for c in &code {
                assert!(
                    d.end <= c.start || d.start >= c.end,
                    "data region 0x{:X}..0x{:X} overlaps code 0x{:X}..0x{:X}",
                    d.start,
                    d.end,
                    c.start,
                    c.end
                );
            }
        }

        // `.reloc` is left out: a pointer value cannot live in a relocation block,
        // and scanning it only produces coincidences.
        let pe = app.header_view.pe.as_ref().expect("pe");
        if let Some(reloc) = pe.sections.iter().find(|s| s.name().unwrap_or("") == ".reloc") {
            let start = reloc.pointer_to_raw_data as usize;
            assert!(
                !data.iter().any(|d| d.start == start),
                ".reloc must not be scanned"
            );
        }
    }

    /// Without a section table there is nothing to call data.
    #[test]
    fn a_non_pe_has_no_data_sections() {
        let mut app = App::new();
        let txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("COPYING");
        if !txt.is_file() {
            return;
        }
        app.load_file(txt.to_str().expect("path"), 0, true).expect("open");
        assert!(data_sections(&app, app.file_info.buffer_len()).is_empty());
    }
}
