use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter, OpKind};
use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefType {
    Call,
    Jmp,
    Jcc,
    Data,
    /// A 64-bit absolute pointer to the target, stored in a data section.
    Ptr,
    /// A 32-bit RVA of the target, stored in a data section.
    Rva,
}

impl XrefType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Call => "CALL",
            Self::Jmp => "JMP",
            Self::Jcc => "JCC",
            Self::Data => "DATA",
            Self::Ptr => "PTR",
            Self::Rva => "RVA",
        }
    }
}

/// Upper bound on collected cross references.
///
/// Every hit allocates a formatted instruction string, and the whole file is
/// decoded linearly, so an unlucky target on a large file could otherwise
/// collect millions of entries before the dialog ever appeared. A list longer
/// than this is not navigable by hand anyway.
pub const MAX_XREF_ITEMS: usize = 5000;

/// True when the scan stopped early because [`MAX_XREF_ITEMS`] was reached.
pub fn is_truncated(items: &[XrefItem]) -> bool {
    items.len() >= MAX_XREF_ITEMS
}

pub fn find_xrefs(app: &App, target_va: u64) -> Vec<XrefItem> {
    let mut items = Vec::new();
    let buffer = app.file_info.get_buffer_ref();
    let filesize = buffer.len();
    if filesize == 0 {
        return items;
    }

    // A zero target matches every zeroed immediate and displacement in the
    // file, which on non-code data is most of them - hundreds of meaningless
    // hits. Callers are expected to reject this before searching; this is the
    // backstop.
    if target_va == 0 {
        return items;
    }

    let bitness = app.bitness();

    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_first_operand_char_index(0);
    formatter.options_mut().set_hex_prefix("0x");
    formatter.options_mut().set_hex_suffix("");
    formatter.options_mut().set_leading_zeroes(false);

    let mut raw_text = String::new();

    // One decoder per executable section, each started at that section's VA.
    //
    // Previously this decoded the file as a single flat range from `get_va(0)`,
    // which made the IP track the file offset: for a PE, where a section's RVA
    // and its raw offset differ, every `instr.ip()` and every
    // `near_branch_target()` comparison was wrong by the section delta. It also
    // decoded the whole file - headers, data, resources - on every keypress,
    // where only code can hold a reference.
    'sections: for section in crate::disasm::sections::code_sections(app, filesize) {
        let decoder = Decoder::with_ip(
            bitness,
            &buffer[section.start..section.end],
            section.va,
            DecoderOptions::NONE,
        );

        let mut current_offset = section.start;

        for instr in decoder {
            let len = instr.len();
            let va = instr.ip();

        let mut is_match = false;
        let mut ref_type = XrefType::Data;

        let flow_control = instr.flow_control();
        match flow_control {
            iced_x86::FlowControl::Call => {
                let target = instr.near_branch_target();
                if target == target_va {
                    is_match = true;
                    ref_type = XrefType::Call;
                }
            }
            iced_x86::FlowControl::UnconditionalBranch => {
                let target = instr.near_branch_target();
                if target == target_va {
                    is_match = true;
                    ref_type = XrefType::Jmp;
                }
            }
            iced_x86::FlowControl::ConditionalBranch => {
                let target = instr.near_branch_target();
                if target == target_va {
                    is_match = true;
                    ref_type = XrefType::Jcc;
                }
            }
            _ => {}
        }

        if !is_match {
            if instr.is_ip_rel_memory_operand() {
                let mem_addr = instr.ip_rel_memory_address();
                if mem_addr == target_va {
                    is_match = true;
                    ref_type = XrefType::Data;
                }
            } else {
                for op in 0..instr.op_count() {
                    match instr.op_kind(op) {
                        OpKind::Memory => {
                            let disp = instr.memory_displacement64();
                            if disp == target_va {
                                is_match = true;
                                ref_type = XrefType::Data;
                                break;
                            }
                        }
                        OpKind::Immediate64
                        | OpKind::Immediate32
                        | OpKind::Immediate32to64 => {
                            let imm = instr.immediate(op);
                            if imm == target_va {
                                is_match = true;
                                ref_type = XrefType::Data;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if is_match {
            raw_text.clear();
            formatter.format(&instr, &mut raw_text);
            let clean_text = raw_text.replace(" short ", " ");
            items.push(XrefItem {
                offset: current_offset,
                va,
                ref_type,
                instr_text: clean_text,
            });
            if items.len() >= MAX_XREF_ITEMS {
                break 'sections;
            }
        }

        current_offset += len;
        if current_offset >= section.end {
            break;
        }
    }
    }

    if items.len() < MAX_XREF_ITEMS {
        find_data_references(app, target_va, buffer, &mut items);
    }

    items
}

/// Offsets in `haystack` where `value` appears as a little-endian `u64`.
///
/// Anchored on the first byte with `memchr`, so the common case - a value that
/// barely occurs - costs one pass over the bytes rather than eight comparisons per
/// offset.
fn find_qword(haystack: &[u8], value: u64) -> Vec<usize> {
    let needle = value.to_le_bytes();
    let mut hits = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = memchr::memchr(needle[0], &haystack[from..]) {
        let at = from + pos;
        if at + 8 <= haystack.len() && haystack[at..at + 8] == needle {
            hits.push(at);
        }
        from = at + 1;
    }
    hits
}

/// As [`find_qword`], for a little-endian `u32`.
fn find_dword(haystack: &[u8], value: u32) -> Vec<usize> {
    let needle = value.to_le_bytes();
    let mut hits = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = memchr::memchr(needle[0], &haystack[from..]) {
        let at = from + pos;
        if at + 4 <= haystack.len() && haystack[at..at + 4] == needle {
            hits.push(at);
        }
        from = at + 1;
    }
    hits
}

/// Adds the references that are stored *as values* rather than encoded in an
/// instruction.
///
/// On x64 a string or a function usually reaches the code through a table: the
/// code loads the table's address RIP-relative and indexes into it, so the
/// target's own address appears nowhere in any instruction. Decoding cannot find
/// those, however much of the file is decoded - the sample this was built against
/// holds 34,474 such pointers, and a third of its Chinese UI strings are reachable
/// only this way.
///
/// Two forms are looked for, tagged apart because they are not equally
/// trustworthy: a full 64-bit pointer (eight bytes have to match, so a coincidence
/// is vanishingly unlikely) and a 32-bit RVA (four bytes, which a plain integer can
/// match by accident - `.pdata` and resource directories are full of RVAs).
fn find_data_references(app: &App, target_va: u64, buffer: &[u8], items: &mut Vec<XrefItem>) {
    let image_base = app.get_image_base();
    let target_rva = target_va.checked_sub(image_base).and_then(|r| u32::try_from(r).ok());

    for section in crate::disasm::sections::data_sections(app, buffer.len()) {
        let bytes = &buffer[section.start..section.end];

        for at in find_qword(bytes, target_va) {
            let offset = section.start + at;
            items.push(XrefItem {
                offset,
                va: section.va + at as u64,
                ref_type: XrefType::Ptr,
                instr_text: format!("qword 0x{:X}", target_va),
            });
            if items.len() >= MAX_XREF_ITEMS {
                return;
            }
        }

        // A zero or tiny RVA would match every small integer in the file.
        if let Some(rva) = target_rva.filter(|r| *r >= 0x1000) {
            for at in find_dword(bytes, rva) {
                let offset = section.start + at;
                items.push(XrefItem {
                    offset,
                    va: section.va + at as u64,
                    ref_type: XrefType::Rva,
                    instr_text: format!("dword rva 0x{:X}", rva),
                });
                if items.len() >= MAX_XREF_ITEMS {
                    return;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct XrefItem {
    pub offset: usize,
    pub va: u64,
    pub ref_type: XrefType,
    pub instr_text: String,
}

#[cfg(test)]
mod xref_tests {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions};

    fn loaded_app() -> Option<App> {
        let mut app = crate::app::App::new();
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        Some(app)
    }

    /// Finds a real `call <addr>` in the loaded image: its file offset and the
    /// address it targets, taken straight from the decoder.
    fn first_call(app: &App) -> Option<(usize, u64)> {
        let buffer = app.file_info.get_buffer_ref();
        let bitness = app.bitness();
        for section in crate::disasm::sections::code_sections(app, buffer.len()) {
            let decoder = Decoder::with_ip(
                bitness,
                &buffer[section.start..section.end],
                section.va,
                DecoderOptions::NONE,
            );
            let mut ofs = section.start;
            for instr in decoder {
                if instr.flow_control() == iced_x86::FlowControl::Call {
                    let target = instr.near_branch_target();
                    if target != 0 {
                        return Some((ofs, target));
                    }
                }
                ofs += instr.len();
                if ofs >= section.end {
                    break;
                }
            }
        }
        None
    }

    /// A call found by decoding must be found again by searching for its target.
    ///
    /// This is the end-to-end form of the flat-mapping bug: `find_xrefs` used to
    /// decode the file as one range starting at `get_va(0)`, so its IPs tracked
    /// file offsets instead of virtual addresses. Every `near_branch_target()`
    /// comparison was then off by the section delta and a genuine call site could
    /// not be located by its own target address.
    #[test]
    fn a_real_call_is_found_by_its_target() {
        let Some(app) = loaded_app() else { return };
        let Some((call_offset, target_va)) = first_call(&app) else {
            return;
        };

        let items = find_xrefs(&app, target_va);
        assert!(
            items.iter().any(|i| i.offset == call_offset),
            "the call at file offset 0x{:X} targets 0x{:X} but the xref search did not report it",
            call_offset,
            target_va
        );
        assert!(
            items
                .iter()
                .any(|i| i.offset == call_offset && i.ref_type == XrefType::Call),
            "the hit must be classified as a CALL"
        );
    }

    /// Reported addresses must be real virtual addresses, i.e. inside a section's
    /// mapped range - not `image_base + file_offset`.
    #[test]
    fn reported_addresses_are_virtual_addresses() {
        let Some(app) = loaded_app() else { return };
        let Some((_, target_va)) = first_call(&app) else {
            return;
        };
        let sections = crate::disasm::sections::code_sections(&app, app.file_info.buffer_len());

        let items = find_xrefs(&app, target_va);
        // Required non-empty, or the loop below would pass by having nothing to
        // check - which is exactly what happened with the broken flat mapping.
        assert!(
            !items.is_empty(),
            "searching for a target taken from a real call must return hits"
        );

        // Instruction hits only: a PTR or RVA hit is a value in a data section and
        // is checked by `data_hits_stay_out_of_code_and_reloc`.
        for item in items
            .into_iter()
            .filter(|i| !matches!(i.ref_type, XrefType::Ptr | XrefType::Rva))
        {
            let section = sections
                .iter()
                .find(|s| item.offset >= s.start && item.offset < s.end)
                .expect("a hit must come from a scanned section");
            let expected = section.va + (item.offset - section.start) as u64;
            assert_eq!(
                item.va, expected,
                "hit at offset 0x{:X} reported VA 0x{:X}, expected 0x{:X}",
                item.offset, item.va, expected
            );
        }
    }

    /// Instruction hits come from code; nothing lands in the headers.
    #[test]
    fn hits_never_come_from_outside_a_code_section() {
        let Some(app) = loaded_app() else { return };
        let Some((_, target_va)) = first_call(&app) else {
            return;
        };
        let sections = crate::disasm::sections::code_sections(&app, app.file_info.buffer_len());

        let items = find_xrefs(&app, target_va);
        assert!(!items.is_empty(), "expected at least one hit to check");

        for item in items
            .into_iter()
            .filter(|i| !matches!(i.ref_type, XrefType::Ptr | XrefType::Rva))
        {
            assert!(
                sections
                    .iter()
                    .any(|s| item.offset >= s.start && item.offset < s.end),
                "hit at 0x{:X} is outside every code section",
                item.offset
            );
        }
    }

    /// The byte matchers find every occurrence, aligned or not, and nothing else.
    #[test]
    fn the_value_matchers_are_exact() {
        // 0x1122334455667788 twice - once 8-byte aligned, once not - plus a value
        // that shares its first byte but differs later.
        let mut buf = vec![0u8; 40];
        buf[8..16].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        buf[21..29].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        buf[32..40].copy_from_slice(&0x1122_3344_5566_7799u64.to_le_bytes());

        assert_eq!(find_qword(&buf, 0x1122_3344_5566_7788), vec![8, 21]);
        assert!(find_qword(&buf, 0xDEAD_BEEF_DEAD_BEEF).is_empty());

        let mut dbuf = vec![0u8; 16];
        dbuf[4..8].copy_from_slice(&0x0009_0188u32.to_le_bytes());
        assert_eq!(find_dword(&dbuf, 0x0009_0188), vec![4]);
        // A match must not be reported when it would run past the end.
        let short = 0x0009_0188u32.to_le_bytes()[..3].to_vec();
        assert!(find_dword(&short, 0x0009_0188).is_empty());
    }

    /// A 64-bit pointer stored in a data section is reported.
    ///
    /// This is the reference kind no amount of disassembly can find: the code loads
    /// a table's address and indexes into it, so the target's own address is in no
    /// instruction. The target here is taken *from* the file - a pointer-looking
    /// value in a data section - so the assertion is about our own reading of it.
    #[test]
    fn a_pointer_in_data_is_reported() {
        let Some(app) = loaded_app() else { return };
        let buffer = app.file_info.get_buffer_ref();
        let base = app.get_image_base();
        let image_end = base + buffer.len() as u64 * 4; // generous upper bound

        // The first 8-byte aligned value in a data section that looks like an
        // address inside this image.
        let mut found: Option<(usize, u64)> = None;
        'outer: for section in crate::disasm::sections::data_sections(&app, buffer.len()) {
            let mut at = section.start;
            while at + 8 <= section.end {
                let value = u64::from_le_bytes(buffer[at..at + 8].try_into().unwrap());
                if value > base + 0x1000 && value < image_end {
                    found = Some((at, value));
                    break 'outer;
                }
                at += 8;
            }
        }
        let Some((offset, value)) = found else { return };

        let items = find_xrefs(&app, value);
        assert!(
            items
                .iter()
                .any(|i| i.offset == offset && i.ref_type == XrefType::Ptr),
            "the pointer at file 0x{:X} holding 0x{:X} was not reported",
            offset,
            value
        );
    }

    /// Data hits never come from a code section or from `.reloc`.
    ///
    /// Both would be double-reporting or noise: an `imm64` in an instruction is
    /// already a DATA hit from the code scan, and a relocation block cannot hold a
    /// pointer.
    #[test]
    fn data_hits_stay_out_of_code_and_reloc() {
        let Some(app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        let Some((_, target_va)) = first_call(&app) else { return };

        let code = crate::disasm::sections::code_sections(&app, len);
        let data = crate::disasm::sections::data_sections(&app, len);

        for item in find_xrefs(&app, target_va) {
            if matches!(item.ref_type, XrefType::Ptr | XrefType::Rva) {
                assert!(
                    data.iter().any(|s| item.offset >= s.start && item.offset < s.end),
                    "data hit at 0x{:X} is outside every data section",
                    item.offset
                );
                assert!(
                    !code.iter().any(|s| item.offset >= s.start && item.offset < s.end),
                    "data hit at 0x{:X} is inside a code section",
                    item.offset
                );
            }
        }
    }

    /// A target too close to the image base has an RVA small enough to match plain
    /// integers, so the RVA scan skips it.
    #[test]
    fn tiny_rvas_are_not_scanned() {
        let Some(app) = loaded_app() else { return };
        let base = app.get_image_base();
        // RVA 0x40 - the DOS header - would otherwise match every 0x40 in the file.
        let items = find_xrefs(&app, base + 0x40);
        assert!(
            !items.iter().any(|i| i.ref_type == XrefType::Rva),
            "a 0x40 RVA was scanned for and produced {} hit(s)",
            items.iter().filter(|i| i.ref_type == XrefType::Rva).count()
        );
    }

    /// The zero-target guard stays in place.
    #[test]
    fn zero_target_finds_nothing() {
        let Some(app) = loaded_app() else { return };
        assert!(find_xrefs(&app, 0).is_empty());
    }

    /// The result cap is still honoured across sections.
    #[test]
    fn result_count_never_exceeds_the_cap() {
        let Some(app) = loaded_app() else { return };
        let Some((_, target_va)) = first_call(&app) else {
            return;
        };
        assert!(find_xrefs(&app, target_va).len() <= MAX_XREF_ITEMS);
    }
}
