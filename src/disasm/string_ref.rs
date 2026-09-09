use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter, Mnemonic, OpKind};
use crate::app::App;

#[derive(Debug, Clone)]
pub struct StringRefItem {
    /// File offset of the instruction that references the string.
    pub offset: usize,
    /// Virtual address of that instruction - the list's first column.
    pub va: u64,
    /// File offset of the string itself, i.e. what the instruction points at.
    ///
    /// Kept so Ctrl+Enter can open the bytes that are actually worth editing.
    /// Without it the only address on hand was the instruction's, which is the one
    /// place in the file a translator has no use for.
    pub string_offset: usize,
    /// Virtual address of the string.
    pub string_va: u64,
    pub va_str_64: String,
    pub va_str_32: String,
    pub instr_text: String,
    pub string_text: String,
    pub encoding_kind: &'static str,
    pub full_text_str: String,
}

pub fn try_read_string_at_offset(buffer: &[u8], offset: usize) -> Option<(String, &'static str)> {
    if offset >= buffer.len() {
        return None;
    }

    let bytes = &buffer[offset..(offset + 256).min(buffer.len())];
    if bytes.is_empty() || bytes[0] == 0 {
        return None;
    }

    // Fast-scan candidate non-zero printable / DBCS byte run directly on slice without allocating
    let mut text_len = 0usize;
    let mut is_pure_ascii = true;
    for &b in bytes {
        if b == 0 {
            break;
        }
        if b.is_ascii_graphic() || b == b' ' || b == b'\t' {
            text_len += 1;
        } else if b >= 0x80 {
            text_len += 1;
            is_pure_ascii = false;
        } else {
            break;
        }
    }

    let text_slice = &bytes[..text_len];

    // 1. Try ASCII
    if is_pure_ascii && text_slice.len() >= 3
        && let Ok(s) = std::str::from_utf8(text_slice)
    {
        let s_trimmed = s.trim();
        if s_trimmed.len() >= 3 {
            return Some((format!("\"{}\"", s_trimmed), "ASCII"));
        }
    }

    // 2. Try UTF-8
    if text_slice.len() >= 4 {
        let mut valid_len = 0;
        for i in 1..=text_slice.len() {
            if let Ok(s) = std::str::from_utf8(&text_slice[..i])
                && let Some(c) = s.chars().last()
            {
                if !c.is_control() && c != '\u{FFFD}' {
                    valid_len = i;
                } else {
                    break;
                }
            }
        }
        if valid_len >= 4
            && let Ok(s) = std::str::from_utf8(&text_slice[..valid_len])
        {
            let s_trimmed = s.trim();
            if s_trimmed.chars().count() >= 2 && s_trimmed.chars().any(|c| c as u32 > 0x7F) {
                return Some((format!("\"{}\"", s_trimmed), "UTF-8"));
            }
        }
    }

    // 3. Try Korean CP949 (EUC-KR)
    if text_slice.len() >= 4 {
        let (cow, has_eval) = encoding_rs::EUC_KR.decode_without_bom_handling(text_slice);
        if !has_eval {
            let s = cow.trim();
            if s.chars().count() >= 2 && s.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c)) {
                return Some((format!("\"{}\"", s), "CP949"));
            }
        }
    }

    // 4. Try Chinese CP936 (GBK)
    if text_slice.len() >= 4 {
        let (cow, has_eval) = encoding_rs::GBK.decode_without_bom_handling(text_slice);
        if !has_eval {
            let s = cow.trim();
            if s.chars().count() >= 2 && s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
                return Some((format!("\"{}\"", s), "CP936"));
            }
        }
    }

    // 4. Try UTF-16 LE
    if bytes.len() >= 4 {
        let mut u16_chars = Vec::new();
        for chunk in bytes.chunks_exact(2) {
            let val = u16::from_le_bytes([chunk[0], chunk[1]]);
            if val == 0 {
                break;
            }
            if let Some(ch) = char::from_u32(val as u32) {
                if !ch.is_control()
                    && (ch.is_ascii_graphic()
                        || ch == ' '
                        || ch == '\t'
                        || ('\u{00A0}'..='\u{024F}').contains(&ch)
                        || ('\u{0370}'..='\u{04FF}').contains(&ch)
                        || ('\u{2000}'..='\u{20BF}').contains(&ch)
                        || ('\u{3000}'..='\u{303F}').contains(&ch)
                        || ('\u{3040}'..='\u{30FF}').contains(&ch)
                        || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
                        || ('\u{3400}'..='\u{4DBF}').contains(&ch)
                        || ('\u{AC00}'..='\u{D7A3}').contains(&ch)
                        || ('\u{1100}'..='\u{11FF}').contains(&ch)
                        || ('\u{3130}'..='\u{318F}').contains(&ch)
                        || ('\u{FF01}'..='\u{FF60}').contains(&ch))
                {
                    u16_chars.push(ch);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let has_cjk = u16_chars.iter().any(|&c| c as u32 > 0x7F);
        let min_len = if has_cjk { 2 } else { 3 };

        if u16_chars.len() >= min_len {
            let s: String = u16_chars.into_iter().collect();
            let s_trimmed = s.trim();
            if s_trimmed.chars().count() >= min_len {
                return Some((format!("L\"{}\"", s_trimmed), "UTF-16LE"));
            }
        }
    }

    None
}

/// Upper bound on collected string references.
///
/// Each item holds four owned `String`s, so the scan's memory grew with the
/// number of references in the file - roughly 10,000 in a 5 MB binary, and
/// unbounded in principle. The filter dialog also walks the whole list per
/// keystroke, so a list far longer than this is neither cheap nor usable.
pub const MAX_STRING_REF_ITEMS: usize = 20_000;

/// True when a scan stopped early at [`MAX_STRING_REF_ITEMS`].
pub fn is_truncated(items: &[StringRefItem]) -> bool {
    items.len() >= MAX_STRING_REF_ITEMS
}

pub fn scan_string_references(app: &App) -> Vec<StringRefItem> {
    // Zero-copy when nothing has been edited yet; `get_effective_buffer()` used
    // to copy the entire file onto the heap on every scan.
    app.with_effective_buffer(|buffer| scan_string_references_in(app, buffer))
}

fn scan_string_references_in(app: &App, buffer: &[u8]) -> Vec<StringRefItem> {
    let mut items = Vec::new();
    let filesize = buffer.len();
    if filesize == 0 {
        return items;
    }

    let bitness = app.bitness();

    // Shared with the cross-reference search, which used to compute this
    // differently and got every PE address wrong as a result. This also drops a
    // full `goblin::Object::parse` of the buffer per scan - the image was already
    // parsed when the file was opened.
    let code_sections = crate::disasm::sections::code_sections(app, filesize);

    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_first_operand_char_index(0);
    formatter.options_mut().set_hex_prefix("0x");
    formatter.options_mut().set_hex_suffix("");
    formatter.options_mut().set_leading_zeroes(false);

    let mut raw_text = String::new();

    for section in code_sections {
        let (sec_start, sec_end, sec_va) = (section.start, section.end, section.va);
        let sec_bytes = &buffer[sec_start..sec_end];
        let decoder = Decoder::with_ip(bitness, sec_bytes, sec_va, DecoderOptions::NONE);

        let mut current_offset = sec_start;

        for instr in decoder {
            let len = instr.len();
            let va = instr.ip();
            let mut target_va = None;
            let mnem = instr.mnemonic();

            if bitness == 64 {
                if instr.is_ip_rel_memory_operand() {
                    let mem_addr = instr.ip_rel_memory_address();
                    if mem_addr >= 0x1000 {
                        target_va = Some(mem_addr);
                    }
                } else {
                    for op in 0..instr.op_count() {
                        match instr.op_kind(op) {
                            OpKind::Immediate64
                            | OpKind::Immediate32
                            | OpKind::Immediate32to64 => {
                                let imm = instr.immediate(op);
                                let rebased = app.rebase_va(imm);
                                if rebased >= 0x1000 {
                                    target_va = Some(rebased);
                                    break;
                                }
                            }
                            OpKind::Memory => {
                                let disp = instr.memory_displacement64();
                                if disp != 0 {
                                    let rebased = app.rebase_va(disp);
                                    if rebased >= 0x1000 {
                                        target_va = Some(rebased);
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                // In 32-bit, allow PUSH, MOV, LEA
                let is_valid_32 = matches!(mnem, Mnemonic::Push | Mnemonic::Mov | Mnemonic::Lea);
                if !is_valid_32 {
                    current_offset += len;
                    if current_offset >= sec_end {
                        break;
                    }
                    continue;
                }

                if instr.is_ip_rel_memory_operand() {
                    let mem_addr = instr.ip_rel_memory_address();
                    if mem_addr >= 0x1000 {
                        target_va = Some(mem_addr);
                    }
                } else {
                    for op in 0..instr.op_count() {
                        match instr.op_kind(op) {
                            OpKind::Immediate32
                            | OpKind::Immediate64
                            | OpKind::Immediate32to64 => {
                                let imm = instr.immediate(op);
                                let rebased = app.rebase_va(imm);
                                if rebased >= 0x1000 {
                                    target_va = Some(rebased);
                                    break;
                                }
                            }
                            OpKind::Memory => {
                                let disp = instr.memory_displacement64();
                                if disp != 0 {
                                    let rebased = app.rebase_va(disp);
                                    if rebased >= 0x1000 {
                                        target_va = Some(rebased);
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Some(t_va) = target_va
                && let Some(str_offset) = app.va_to_offset(t_va)
                && str_offset < filesize
                && let Some((str_val, enc_kind)) = try_read_string_at_offset(buffer, str_offset)
            {
                raw_text.clear();
                formatter.format(&instr, &mut raw_text);
                let clean_instr = crate::disasm::draw::clean_instruction_text(app, &instr, &raw_text);

                let va_str_64 = if va >= 0x1_0000_0000 {
                    format!("{:X}", va)
                } else {
                    format!("{:08X}", va)
                };
                let va_str_32 = format!("{:08X}", va);
                let full_text_str = format!("{} {}", enc_kind, str_val);

                items.push(StringRefItem {
                    offset: current_offset,
                    va,
                    string_offset: str_offset,
                    string_va: t_va,
                    va_str_64,
                    va_str_32,
                    instr_text: clean_instr,
                    string_text: str_val,
                    encoding_kind: enc_kind,
                    full_text_str,
                });
                if items.len() >= MAX_STRING_REF_ITEMS {
                    return items;
                }
            }

            current_offset += len;
            if current_offset >= sec_end {
                break;
            }
        }
    }

    items
}

#[cfg(test)]
mod string_location_tests {
    use super::*;

    /// Every item has to point at the string it reports, not just at the
    /// instruction: Ctrl+Enter navigates by `string_offset`, so a wrong value there
    /// would send the cursor somewhere arbitrary with nothing to show it had.
    #[test]
    fn the_string_location_matches_the_text_that_was_reported() {
        let mut app = crate::app::App::new();
        app.config.database = false;
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(path) = exe.to_str() else { return };
        if app.load_file(path, 0, false).is_err() {
            return;
        }
        if app.header_view.pe.is_none() {
            return;
        }

        let items = scan_string_references(&app);
        if items.is_empty() {
            return;
        }

        let checked = app.with_effective_buffer(|buffer| {
            let mut checked = 0usize;
            for item in items.iter().take(50) {
                assert!(
                    item.string_offset < buffer.len(),
                    "string offset 0x{:X} is past the end of the file",
                    item.string_offset
                );
                let read = try_read_string_at_offset(buffer, item.string_offset);
                let Some((text, encoding)) = read else {
                    panic!(
                        "nothing readable at the reported string offset 0x{:X} (instruction 0x{:X})",
                        item.string_offset, item.va
                    );
                };
                assert_eq!(text, item.string_text, "at offset 0x{:X}", item.string_offset);
                assert_eq!(encoding, item.encoding_kind);
                checked += 1;
            }
            checked
        });

        // And the two addresses agree with the section layout.
        for item in items.iter().take(50) {
            assert_eq!(
                app.va_to_offset(item.string_va),
                Some(item.string_offset),
                "0x{:X} does not map to offset 0x{:X}",
                item.string_va,
                item.string_offset
            );
            assert_ne!(
                item.string_offset, item.offset,
                "the string and the instruction that points at it cannot be the same byte"
            );
        }

        assert!(checked > 0);
    }

    #[test]
    fn utf16_cjk_chinese_string_is_decoded_properly() {
        // "分辨率 " in UTF-16LE: 06 52 A8 8F 87 73 20 00 00 00
        let bytes = vec![0x06, 0x52, 0xA8, 0x8F, 0x87, 0x73, 0x20, 0x00, 0x00, 0x00];
        let read = try_read_string_at_offset(&bytes, 0);
        assert!(read.is_some());
        let (s, enc) = read.unwrap();
        assert_eq!(enc, "UTF-16LE");
        assert_eq!(s, "L\"分辨率\"");
    }

    #[test]
    fn inspect_string_references_finds_all_references() {
        let path = "C:\\Users\\Administrator\\Desktop\\pecmd\\dumped_SCY.exe";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut app = crate::app::App::new();
        app.config.database = false;
        if app.load_file(path, 0, false).is_err() {
            return;
        }
        let items = scan_string_references(&app);
        assert!(items.len() > 1000, "Expected thousands of string references, got {}", items.len());
        let found = items.iter().any(|it| it.string_offset == 0x1206A8 && it.string_text.contains("分辨率"));
        assert!(found, "Expected to find reference to string at 0x1206A8");
    }
}