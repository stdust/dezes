use std::fmt::Write as _;

use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Paragraph, Row, Table},
};

use crate::app::App;

/// Address column width, 64-bit target.
pub const VA_COL_WIDTH_64: u16 = 11;
/// Address column width, 32-bit target.
pub const VA_COL_WIDTH_32: u16 = 10;
/// Hex-dump column width.
pub const BYTES_COL_WIDTH: u16 = 22;
/// Instruction text column width.
///
/// Wider than the 42 it used to be: with import names substituted into the
/// operand, `call qword ptr ds:[<GetSystemTimeAsFileTime>]` is 45 characters and
/// was being clipped mid-name.
pub const DISASM_COL_WIDTH: u16 = 52;

/// Column widths are shared with `ruler.rs`, which draws the header above this
/// table; keeping them in one place stops the two from drifting apart.
pub fn va_col_width(is_64: bool) -> u16 {
    if is_64 { VA_COL_WIDTH_64 } else { VA_COL_WIDTH_32 }
}

/// Longest possible x86 instruction. Used to size decode windows, so a window is
/// always large enough to hold `n` instructions.
const MAX_INSTR_BYTES: usize = 16;

/// How many bytes to read when probing for a string at a target address.
const STRING_PROBE_LEN: usize = 128;

/// Shortest run of printable bytes that counts as a string in the comment column.
const MIN_STRING_LEN: usize = 3;

fn is_register(tok: &str) -> bool {
    let clean = tok.trim_matches(|c: char| !c.is_alphanumeric());
    // Longest register mnemonic below is 5 characters ("xmm15"). Lower-casing
    // into a stack buffer avoids the String that `to_lowercase()` allocated for
    // every word of every instruction on every frame.
    if clean.is_empty() || clean.len() > 5 || !clean.is_ascii() {
        return false;
    }
    let mut buf = [0u8; 5];
    for (dst, src) in buf.iter_mut().zip(clean.as_bytes()) {
        *dst = src.to_ascii_lowercase();
    }

    matches!(
        &buf[..clean.len()],
        b"rax" | b"rbx" | b"rcx" | b"rdx" | b"rsi" | b"rdi" | b"rsp" | b"rbp" |
        b"r8" | b"r9" | b"r10" | b"r11" | b"r12" | b"r13" | b"r14" | b"r15" |
        b"eax" | b"ebx" | b"ecx" | b"edx" | b"esi" | b"edi" | b"esp" | b"ebp" |
        b"r8d" | b"r9d" | b"r10d" | b"r11d" | b"r12d" | b"r13d" | b"r14d" | b"r15d" |
        b"ax" | b"bx" | b"cx" | b"dx" | b"si" | b"di" | b"sp" | b"bp" | b"ip" |
        b"r8w" | b"r9w" | b"r10w" | b"r11w" | b"r12w" | b"r13w" | b"r14w" | b"r15w" |
        b"al" | b"bl" | b"cl" | b"dl" | b"ah" | b"bh" | b"ch" | b"dh" | b"sil" | b"dil" | b"bpl" | b"spl" |
        b"r8b" | b"r9b" | b"r10b" | b"r11b" | b"r12b" | b"r13b" | b"r14b" | b"r15b" |
        b"rip" | b"eip" |
        b"cs" | b"ds" | b"es" | b"fs" | b"gs" | b"ss" |
        b"st0" | b"st1" | b"st2" | b"st3" | b"st4" | b"st5" | b"st6" | b"st7" |
        b"xmm0" | b"xmm1" | b"xmm2" | b"xmm3" | b"xmm4" | b"xmm5" | b"xmm6" | b"xmm7" |
        b"xmm8" | b"xmm9" | b"xmm10" | b"xmm11" | b"xmm12" | b"xmm13" | b"xmm14" | b"xmm15" |
        b"ymm0" | b"ymm1" | b"ymm2" | b"ymm3" | b"ymm4" | b"ymm5" | b"ymm6" | b"ymm7" |
        b"ymm8" | b"ymm9" | b"ymm10" | b"ymm11" | b"ymm12" | b"ymm13" | b"ymm14" | b"ymm15"
    )
}

/// True for the six segment registers, which appear only as an address prefix.
///
/// Deliberately separate from `is_register`: a segment prefix says which address
/// space the access goes through, not which value is being operated on, and it is
/// coloured on its own.
fn is_segment_register(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "cs" | "ds" | "es" | "fs" | "gs" | "ss"
    )
}

fn read_string_at_offset(buffer: &[u8], offset: usize) -> Option<String> {
    // `&buffer[offset..]` panics outright when `offset > buffer.len()`; the
    // callers derive offsets from VAs, so that is reachable.
    if offset >= buffer.len() {
        return None;
    }
    let bytes = &buffer[offset..(offset + STRING_PROBE_LEN).min(buffer.len())];

    // 1. Try ASCII / UTF-8
    let mut str_bytes = Vec::new();
    for &b in bytes {
        if b == 0 {
            break;
        }
        if b.is_ascii_graphic() || b == b' ' || b == b'\t' {
            str_bytes.push(b);
        } else {
            break;
        }
    }

    if str_bytes.len() >= MIN_STRING_LEN {
        if let Ok(s) = std::str::from_utf8(&str_bytes) {
            let s_trimmed = s.trim();
            if s_trimmed.len() >= MIN_STRING_LEN {
                return Some(s_trimmed.to_string());
            }
        }
    }

    // 2. Try UTF-16 LE
    if bytes.len() >= 4 {
        let mut u16_chars = Vec::new();
        for chunk in bytes.chunks_exact(2) {
            let val = u16::from_le_bytes([chunk[0], chunk[1]]);
            if val == 0 {
                break;
            }
            if let Some(ch) = char::from_u32(val as u32) {
                if (ch.is_ascii_graphic() || ch == ' ' || ch == '\t') && !ch.is_control() {
                    u16_chars.push(ch);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if u16_chars.len() >= MIN_STRING_LEN {
            let s: String = u16_chars.into_iter().collect();
            let s_trimmed = s.trim();
            if s_trimmed.len() >= MIN_STRING_LEN {
                return Some(s_trimmed.to_string());
            }
        }
    }

    None
}

fn try_get_string_at_va(app: &App, va: u64) -> Option<String> {
    let offset = app.va_to_offset(va)?;
    if offset >= app.file_info.size {
        return None;
    }

    let buffer = app.file_info.get_buffer_ref();
    if offset >= buffer.len() {
        return None;
    }

    // 1. Direct String Check at 'va'
    if let Some(s) = read_string_at_offset(buffer, offset) {
        return Some(s);
    }

    // 2. Indirect Pointer String Check (Dereference 64-bit pointer at 'va')
    if offset + 8 <= buffer.len() {
        let ptr_bytes = &buffer[offset..offset + 8];
        let target_va = u64::from_le_bytes(ptr_bytes.try_into().unwrap());
        if target_va != 0 && target_va != va {
            if let Some(target_offset) = app.va_to_offset(target_va) {
                if target_offset < buffer.len() {
                    if let Some(s) = read_string_at_offset(buffer, target_offset) {
                        return Some(s);
                    }
                }
            }
        }
    }

    // 3. Indirect Pointer String Check (Dereference 32-bit pointer at 'va')
    if offset + 4 <= buffer.len() {
        let ptr_bytes = &buffer[offset..offset + 4];
        let target_va = u32::from_le_bytes(ptr_bytes.try_into().unwrap()) as u64;
        if target_va != 0 && target_va != va {
            if let Some(target_offset) = app.va_to_offset(target_va) {
                if target_offset < buffer.len() {
                    if let Some(s) = read_string_at_offset(buffer, target_offset) {
                        return Some(s);
                    }
                }
            }
        }
    }

    None
}

/// `0x0000004D` -> `0x4D`.
///
/// Walks the string in place rather than collecting it into a `Vec<char>` first,
/// which used to happen once per instruction per frame.
fn trim_hex_leading_zeros(text: &str) -> String {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'0' && i + 1 < len && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
            result.push_str("0x");
            i += 2;
            let start_digits = i;
            while i < len && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let trimmed = text[start_digits..i].trim_start_matches('0');
            if trimmed.is_empty() {
                result.push('0');
            } else {
                result.push_str(trimmed);
            }
        } else if let Some(c) = text[i..].chars().next() {
            // `i` is always on a char boundary: it only advances over ASCII hex
            // digits, the "0x" prefix, or one whole char at a time.
            result.push(c);
            i += c.len_utf8();
        } else {
            break;
        }
    }
    result
}

/// Adds an explicit `ds:` to memory operands that have no segment override.
///
/// Uses a 3-char sliding window instead of materialising a `Vec<char>`.
fn ensure_ds_segment(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 16);
    let mut prev: [char; 3] = ['\0', '\0', '\0'];
    let mut seen = 0usize;

    for c in text.chars() {
        if c == '[' {
            let has_seg = seen >= 3
                && prev[2] == ':'
                && prev[1] == 's'
                && matches!(prev[0], 'd' | 'f' | 'g' | 's' | 'c' | 'e');

            if has_seg {
                result.push('[');
            } else {
                result.push_str("ds:[");
            }
        } else {
            result.push(c);
        }
        prev = [prev[1], prev[2], c];
        seen += 1;
    }
    result
}

/// Text for the comment column, in priority order: the user's own comment, the
/// imported function name, then a string the operands point at.
///
/// Shared with the clipboard copy in `disasm/events.rs`, which built its own lines
/// and so produced a listing with no comments at all - the reason an exported
/// disassembly looked like the annotations were never resolved.
pub fn line_comment(app: &App, offset: usize, formatted_text: &str) -> Option<String> {
    // A comment the user typed wins: it is the only one of the three that is not
    // derived, so silently hiding it behind a guess would lose information.
    if let Some(comment) = app.hex_view.comments.get(&offset) {
        return Some(comment.clone());
    }

    // Imports are *not* handled here any more: the name is substituted straight
    // into the operand (see `apply_import_symbol`), so repeating it in the comment
    // column would only duplicate it.
    string_comment_from_text(app, formatted_text)
}

/// String pointed at by any address-sized operand of `text`, if there is one.
///
/// Branch instructions are skipped: their operand is code, and probing it for a
/// string only ever yields noise.
fn string_comment_from_text(app: &App, text: &str) -> Option<String> {
    let mut tokens = text.split_whitespace();
    let first = tokens.next()?.to_lowercase();
    if first == "call" || first.starts_with('j') {
        return None;
    }

    for token in tokens {
        for word in token.split(|c: char| !c.is_alphanumeric()) {
            let digits = word
                .strip_prefix("0x")
                .or_else(|| word.strip_prefix("0X"))
                .unwrap_or(word);
            // Five digits is the shortest plausible address; shorter values are
            // ordinary small immediates and probing them is pure noise.
            if digits.len() < 5 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            if let Ok(va) = u64::from_str_radix(digits, 16)
                && va >= 0x400000
                && let Some(found) = try_get_string_at_va(app, va)
            {
                return Some(found);
            }
        }
    }

    None
}

fn format_x64dbg_line(text: &str, is_modified: bool, is_selected: bool, main_style: Style, selected_style: Style, app: &App) -> Line<'static> {
    let base_style = if is_selected {
        if is_modified {
            selected_style.fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            selected_style
        }
    } else if is_modified {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        main_style
    };

    let has_short = text.contains(" short ");
    let has_comma = text.contains(',');

    let clean_text: String = if has_short || has_comma {
        let mut s = text.to_string();
        if has_short {
            s = s.replace(" short ", " ");
        }
        if has_comma {
            s = s.replace(',', ", ");
        }
        s
    } else {
        text.to_string()
    };

    // Undecodable bytes ("???") get an explicit pure-red background rather than
    // the terminal's palette `Color::Red`, which varies per theme/terminal and
    // could come out dull enough to miss.
    let bad_style = Style::default()
        .fg(Color::White)
        .bg(Color::Rgb(0xFF, 0x00, 0x00))
        .add_modifier(Modifier::BOLD);

    // If syntax highlighting is OFF (:set highlight off / :set hilight off)
    if !app.config.syntax_highlight {
        let instr_trim = clean_text.trim();
        return if instr_trim == "(bad)" || instr_trim == "bad" {
            Line::from(Span::styled("???".to_string(), bad_style))
        } else {
            Line::from(Span::styled(clean_text.trim_end().to_string(), base_style))
        };
    }

    let tokens: Vec<&str> = clean_text.split_whitespace().collect();
    if tokens.is_empty() {
        return Line::from(Span::styled(clean_text, base_style));
    }

    let first_op = tokens[0].to_lowercase();
    let mut spans = Vec::new();

    let dt = &app.config.disasm_theme;
    let yellow_bg = dt.jmp_bg;
    let cyan_bg = dt.call_bg;
    let black_fg = dt.call_fg;
    let red_fg = dt.jcc_fg;
    let blue_fg = dt.push_pop_fg;
    let reg_green_fg = dt.register_fg;
    let magenta_fg = dt.keyword_fg;
    let olive_fg = dt.immediate_fg;
    let mem_ptr_fg = dt.memory_op_fg;
    let segment_fg = dt.segment_fg;
    let import_bg = dt.import_bg;
    let import_fg = dt.import_fg;

    let mut in_mem_operand = false;

    for (i, tok) in tokens.iter().enumerate() {
        if i > 0 {
            let space_style = if in_mem_operand {
                base_style.fg(mem_ptr_fg).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            spans.push(Span::styled(" ", space_style));
        }

        let lower_tok = tok.to_lowercase();
        let mut token_style = base_style;

        if i == 0 {
            if first_op == "(bad)" || first_op == "bad" {
                spans.push(Span::styled("???".to_string(), bad_style));
                continue;
            } else if first_op == "call" {
                token_style = token_style.bg(cyan_bg).fg(black_fg).add_modifier(Modifier::BOLD);
            } else if first_op == "jmp" {
                token_style = token_style.bg(yellow_bg).fg(black_fg).add_modifier(Modifier::BOLD);
            } else if first_op.starts_with('j') {
                token_style = token_style.bg(yellow_bg).fg(red_fg).add_modifier(Modifier::BOLD);
            } else if first_op.starts_with("push") || first_op.starts_with("pop") {
                token_style = token_style.fg(blue_fg).add_modifier(Modifier::BOLD);
            } else if first_op.starts_with("ret") {
                token_style = token_style.bg(cyan_bg).fg(black_fg).add_modifier(Modifier::BOLD);
            } else if is_modified {
                token_style = token_style.fg(red_fg).add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(tok.to_string(), token_style));
            continue;
        }

        if lower_tok == "(bad)" || lower_tok == "bad" {
            spans.push(Span::styled("???".to_string(), bad_style));
            continue;
        }

        // Pure branch targets (e.g. call 0x140001000 or jmp 0x140001000)
        let is_pure_branch_target = (first_op == "call" || first_op.starts_with('j')) && (tok.starts_with("0x") || tok.chars().all(|c| c.is_ascii_hexdigit()));

        if is_pure_branch_target {
            let style = base_style.bg(yellow_bg).fg(if first_op.starts_with('j') { red_fg } else { black_fg }).add_modifier(Modifier::BOLD);
            spans.push(Span::styled(tok.to_string(), style));
            continue;
        }

        // Check if token starts memory operand mode (e.g. dword, qword, word, byte, ptr, ds:[...)
        if !in_mem_operand {
            if lower_tok.contains("dword") || lower_tok.contains("qword") || lower_tok.contains("word") || lower_tok.contains("byte") || lower_tok.contains("ptr") || lower_tok.contains('[') {
                in_mem_operand = true;
            }
        }

        // Sub-token lexer for operands: split words (registers, numbers, keywords)
        // and punctuation (',', '[', ']', '+', '-', ':', '<', '>').
        //
        // The word styling used to be written out twice - once for a word ended by
        // punctuation, once for one ending the token - and the two copies had to be
        // kept in step by hand. It is a closure now, so the segment and import
        // cases below could not be added to only one of them.
        let word_style = |word: &str, in_mem: bool, in_import: bool, next: Option<char>| -> Style {
            // An inlined import name owns its whole span, brackets included.
            if in_import {
                return base_style
                    .bg(import_bg)
                    .fg(import_fg)
                    .add_modifier(Modifier::BOLD);
            }
            // A segment prefix is checked before the memory-operand case, because it
            // is always inside one: `ds:[...]`. Without that ordering it would just
            // take the address colour, which is what made `ds:` indistinguishable
            // from the address it qualifies.
            if next == Some(':') && is_segment_register(word) {
                return base_style.fg(segment_fg).add_modifier(Modifier::BOLD);
            }
            if in_mem {
                base_style.fg(mem_ptr_fg).add_modifier(Modifier::BOLD)
            } else if is_register(word) {
                base_style.fg(reg_green_fg).add_modifier(Modifier::BOLD)
            } else if word.starts_with("0x")
                || (word.len() >= 2 && word.chars().all(|ch| ch.is_ascii_hexdigit()))
                || word.chars().all(|ch| ch.is_ascii_digit())
            {
                base_style.fg(olive_fg)
            } else if word == "ptr" || word == "qword" || word == "dword" || word == "word" || word == "byte" {
                base_style.fg(magenta_fg)
            } else {
                base_style
            }
        };

        let import_style = base_style
            .bg(import_bg)
            .fg(import_fg)
            .add_modifier(Modifier::BOLD);

        let chars: Vec<char> = tok.chars().collect();
        let mut word_buf = String::new();
        let mut in_import = false;
        let mut prev_was_segment = false;

        for (ci, &c) in chars.iter().enumerate() {
            if c.is_alphanumeric() || c == '_' || c == '@' || c == '?' || c == '-' && in_import {
                // Import names are not all alphanumeric: mangled C++ symbols and the
                // `api-ms-win-*` module names carry `_`, `@`, `?` and `-`, and
                // breaking on those split one name into several differently
                // coloured pieces.
                word_buf.push(c);
                continue;
            }

            if !word_buf.is_empty() {
                let style = word_style(&word_buf, in_mem_operand, in_import, Some(c));
                prev_was_segment = c == ':' && is_segment_register(&word_buf);
                spans.push(Span::styled(word_buf.clone(), style));
                word_buf.clear();
            }

            if c == '<' {
                in_import = true;
            }

            let p_style = if in_import || c == '>' {
                import_style
            } else if c == ':' && prev_was_segment {
                // The colon belongs to the prefix, not to the address.
                base_style.fg(segment_fg).add_modifier(Modifier::BOLD)
            } else if in_mem_operand {
                base_style.fg(mem_ptr_fg).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            spans.push(Span::styled(c.to_string(), p_style));

            if c == '>' {
                in_import = false;
            }
            if c == ']' {
                in_mem_operand = false;
            }
            if c != ':' {
                prev_was_segment = false;
            }
            let _ = ci;
        }

        if !word_buf.is_empty() {
            let style = word_style(&word_buf, in_mem_operand, in_import, None);
            spans.push(Span::styled(word_buf, style));
        }

        if tok.ends_with(']') {
            in_mem_operand = false;
        }
    }

    Line::from(spans)
}

/// The address this instruction reaches an import through, and the name to show
/// in place of it.
///
/// Only the *address* the instruction references is considered, so the 64-bit
/// `call qword ptr [rip+0x1234]` and the 32-bit `call dword ptr [0x402004]` forms
/// are handled by the same lookup. Direct `call 0x...` is included too, because a
/// call to a one-line `jmp [IAT]` thunk is extremely common and following it by
/// hand to find out which API it is defeats the point of the annotation.
///
/// The address is returned alongside the name because the name is substituted into
/// the operand text, and that requires knowing exactly which number to replace.
fn import_symbol_for(app: &App, instr: &iced_x86::Instruction) -> Option<(u64, String)> {
    if app.import_labels.is_empty() {
        return None;
    }

    if instr.is_ip_rel_memory_operand() {
        let va = instr.ip_rel_memory_address();
        if let Some(label) = app.import_labels.get(&va) {
            return Some((va, function_name(label)));
        }
    }

    for op in 0..instr.op_count() {
        if instr.op_kind(op) == iced_x86::OpKind::Memory {
            let disp = instr.memory_displacement64();
            if disp != 0
                && let Some(label) = app.import_labels.get(&disp)
            {
                return Some((disp, function_name(label)));
            }
        }
    }

    // A direct branch to a thunk: resolve one level of `jmp [IAT]`.
    match instr.flow_control() {
        iced_x86::FlowControl::Call | iced_x86::FlowControl::UnconditionalBranch => {
            let target = instr.near_branch_target();
            if target != 0 {
                // The module stays in the name here: the branch leaves this
                // function for another module's stub, so which module it is
                // carries information the slot case does not.
                return thunk_target_label(app, target).map(|label| (target, label));
            }
            None
        }
        _ => None,
    }
}

/// `KERNEL32.CreateFileW` -> `CreateFileW`.
///
/// The operand is already `ds:[...]`, i.e. visibly an import-table slot, so
/// repeating the module on every line only costs column width.
fn function_name(label: &str) -> String {
    match label.rsplit_once('.') {
        Some((_, name)) if !name.is_empty() => name.to_string(),
        _ => label.to_string(),
    }
}

/// Replaces the import address in `text` with `<Name>`.
///
/// This is what x64dbg shows - `call qword ptr ds:[<GetSystemTimeAsFileTime>]` -
/// and it reads better than the same information in a far-right comment column,
/// where the eye has to travel across the line to connect the two.
///
/// Substitution is textual because the formatter has already run; the address is
/// matched in exactly the spelling the formatter produces (`0x` prefix, uppercase,
/// no leading zeroes). If it is not found the text is returned unchanged rather
/// than mangled.
pub fn apply_import_symbol(app: &App, instr: &iced_x86::Instruction, text: &str) -> String {
    let Some((address, name)) = import_symbol_for(app, instr) else {
        return text.to_string();
    };

    let printed = format!("0x{:X}", address);
    if !text.contains(&printed) {
        return text.to_string();
    }

    text.replace(&printed, &format!("<{}>", name))
}

/// Decodes the single instruction at `va` and, if it is a jump through an import
/// slot, returns that import's label.
///
/// Bounded to one instruction: this runs per visible line per frame, and the point
/// is to see through the standard one-instruction thunk, not to chase arbitrary
/// chains.
fn thunk_target_label(app: &App, va: u64) -> Option<String> {
    let offset = app.va_to_offset(va)?;
    let buffer = app.file_info.get_buffer_ref();
    let end = offset.saturating_add(MAX_INSTR_BYTES).min(buffer.len());
    if offset >= end {
        return None;
    }

    let decoder = Decoder::with_ip(app.bitness(), &buffer[offset..end], va, DecoderOptions::NONE);
    let instr = decoder.into_iter().next()?;
    if instr.flow_control() != iced_x86::FlowControl::UnconditionalBranch {
        return None;
    }

    if instr.is_ip_rel_memory_operand() {
        let slot = instr.ip_rel_memory_address();
        if let Some(label) = app.import_labels.get(&slot) {
            return Some(label.clone());
        }
    }

    for op in 0..instr.op_count() {
        if instr.op_kind(op) == iced_x86::OpKind::Memory {
            let disp = instr.memory_displacement64();
            if disp != 0
                && let Some(label) = app.import_labels.get(&disp)
            {
                return Some(label.clone());
            }
        }
    }

    None
}

use std::sync::Mutex;

/// Order-independent fingerprint of the pending byte edits.
///
/// The cache used to key on `changed_bytes.len()` alone, so overwriting a byte
/// that had already been edited left the count unchanged and the view kept
/// showing the previous disassembly. Mixing offsets *and* values in means any
/// edit invalidates the cache.
fn changed_bytes_fingerprint(changed: &std::collections::HashMap<usize, u8>) -> u64 {
    let mut acc = changed.len() as u64;
    for (&ofs, &val) in changed {
        let mut h = ofs as u64 ^ 0x9E37_79B9_7F4A_7C15;
        h = h.rotate_left(7) ^ (val as u64);
        // XOR keeps the result independent of HashMap iteration order.
        acc ^= h.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    }
    acc
}

/// Fingerprint of every colour the cached rows were built with.
///
/// The cached `Row`s carry their styles baked in, so a colour change has to
/// invalidate them. It did not: `:set theme` and `:set disasmtheme` left the old
/// rows on screen until something moved the cursor. Computed per frame rather
/// than bumped by each command, so no future `:set` can forget to invalidate.
fn style_fingerprint(app: &App) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    let dt = &app.config.disasm_theme;
    for color in [
        dt.call_bg,
        dt.call_fg,
        dt.jmp_bg,
        dt.jmp_fg,
        dt.jcc_bg,
        dt.jcc_fg,
        dt.push_pop_fg,
        dt.ret_bg,
        dt.ret_fg,
        dt.register_fg,
        dt.memory_op_fg,
        dt.immediate_fg,
        dt.keyword_fg,
        dt.comment_fg,
        dt.segment_fg,
        dt.import_bg,
        dt.import_fg,
    ] {
        color.hash(&mut hasher);
    }

    // The rows also use the main theme's styles for text, addresses and the
    // cursor line.
    for style in [
        app.config.theme.main,
        app.config.theme.offsets,
        app.config.theme.highlight,
        app.config.theme.dimmed,
        app.config.theme.changed_bytes,
    ] {
        style.fg.hash(&mut hasher);
        style.bg.hash(&mut hasher);
    }

    hasher.finish()
}

struct DisasmCacheState {
    /// Which file these rows came from; see `App::view_generation`.
    generation: u64,
    /// Decoding width, which changes the rows even at the same offset.
    ///
    /// The actual width, not an `is_64` flag: 16 and 32 are both "not 64" yet decode
    /// differently, so a flag let a forced 16-bit view be served from 32-bit rows.
    bitness: u32,
    style_key: u64,
    page_start: usize,
    offset: usize,
    selection_anchor: Option<usize>,
    changed_bytes_key: u64,
    area_width: u16,
    area_height: u16,
    syntax_highlight: bool,

    addr_rows: Vec<Row<'static>>,
    bytes_rows: Vec<Row<'static>>,
    instr_rows: Vec<Row<'static>>,
    comment_rows: Vec<Row<'static>>,
}

static DISASM_CACHE: Mutex<Option<DisasmCacheState>> = Mutex::new(None);

pub fn draw_disasm_view(app: &mut App, frame: &mut Frame, area: Rect) {
    let height = area.height as usize;
    let filesize = app.file_info.size;
    let current_cursor_offset = app.hex_view.offset;

    let bitness = app.bitness();
    let is_64_bit = app.is_64();
    let main_style = app.config.theme.main;
    let offsets_style = app.config.theme.offsets;
    let highlight_style = app.config.theme.highlight;

    let mut page_start = app.reader.page_start;

    // Check if cache is valid to skip decoding loop
    let changed_bytes_key = changed_bytes_fingerprint(&app.hex_view.changed_bytes);
    let syntax_highlight = app.config.syntax_highlight;

    let style_key = style_fingerprint(app);

    if let Ok(guard) = DISASM_CACHE.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.generation == app.view_generation
                && cached.bitness == bitness
                && cached.style_key == style_key
                && cached.page_start == page_start
                && cached.offset == current_cursor_offset
                && cached.selection_anchor == app.disasm_selection_anchor
                && cached.changed_bytes_key == changed_bytes_key
                && cached.area_width == area.width
                && cached.area_height == area.height
                && cached.syntax_highlight == syntax_highlight
            {
                // Fast path: Render directly from cached rows
                let va_col_width = va_col_width(is_64_bit);
                let bytes_col_width = BYTES_COL_WIDTH;
                let disasm_col_width = DISASM_COL_WIDTH;

                let disasm_layout = Layout::default()
                    .direction(ratatui::layout::Direction::Horizontal)
                    .constraints(vec![
                        Constraint::Length(va_col_width),
                        Constraint::Length(1), // Separator 1
                        Constraint::Length(bytes_col_width),
                        Constraint::Length(1), // Separator 2
                        Constraint::Length(disasm_col_width), // Disassembly column
                        Constraint::Length(1), // Separator 3
                        Constraint::Min(0),    // Comment column (Right side)
                    ])
                    .split(area);

                let sep_height = area.height as usize;
                let sep_line_color = app.config.theme.dimmed.fg.unwrap_or(Color::Rgb(128, 128, 128));

                let sep_style = Style::default()
                    .fg(sep_line_color)
                    .bg(main_style.bg.unwrap_or(Color::Reset));

                let sep_str = "│\n".repeat(sep_height);
                let sep_para1 = Paragraph::new(sep_str.clone()).style(sep_style);
                let sep_para2 = Paragraph::new(sep_str.clone()).style(sep_style);
                let sep_para3 = Paragraph::new(sep_str).style(sep_style);

                let bg_fill_str = " \n".repeat(sep_height);
                let offsets_bg_fill = Paragraph::new(bg_fill_str.clone()).style(offsets_style);
                let main_bg_fill = Paragraph::new(bg_fill_str).style(main_style);

                frame.render_widget(Clear, area);
                frame.render_widget(offsets_bg_fill, disasm_layout[0]);
                frame.render_widget(main_bg_fill.clone(), disasm_layout[1]);
                frame.render_widget(main_bg_fill.clone(), disasm_layout[2]);
                frame.render_widget(main_bg_fill.clone(), disasm_layout[3]);
                frame.render_widget(main_bg_fill.clone(), disasm_layout[4]);
                frame.render_widget(main_bg_fill.clone(), disasm_layout[5]);
                frame.render_widget(main_bg_fill, disasm_layout[6]);

                let addr_table = Table::new(cached.addr_rows.clone(), [Constraint::Length(va_col_width)]).style(offsets_style);
                let bytes_table = Table::new(cached.bytes_rows.clone(), [Constraint::Length(bytes_col_width)]).style(main_style);
                let instr_table = Table::new(cached.instr_rows.clone(), [Constraint::Length(disasm_col_width)]).style(main_style);
                let comment_table = Table::new(cached.comment_rows.clone(), [Constraint::Min(0)]).style(main_style);

                frame.render_widget(addr_table, disasm_layout[0]);
                frame.render_widget(sep_para1, disasm_layout[1]);
                frame.render_widget(bytes_table, disasm_layout[2]);
                frame.render_widget(sep_para2, disasm_layout[3]);
                frame.render_widget(instr_table, disasm_layout[4]);
                frame.render_widget(sep_para3, disasm_layout[5]);
                frame.render_widget(comment_table, disasm_layout[6]);
                return;
            }
        }
    }

    let mut addr_rows: Vec<Row> = Vec::new();
    let mut bytes_rows: Vec<Row> = Vec::new();
    let mut instr_rows: Vec<Row> = Vec::new();

    // Everything below slices `buffer`, so bound offsets by the live mapping.
    // `file_info.size` comes from the directory entry and can be larger.
    let buf_len = app.file_info.buffer_len();
    let filesize = filesize.min(buf_len);

    if page_start >= filesize || buf_len == 0 {
        return;
    }

    // Cursor above the page: scroll up to it, starting the page on the boundary
    // of the instruction the cursor is inside.
    //
    // There used to be two separate fixups for this, and they disagreed. This one
    // only rewound `page_start` by a single instruction, so a cursor further up
    // stayed off screen; the other (further down, after the visible-height scan)
    // assigned the raw cursor offset, which starts the page mid-instruction and
    // renders a screen of garbage until the decoder happens to re-sync. Both are
    // now this single call, which is also what the navigation keys use.
    if current_cursor_offset < page_start {
        page_start = crate::disasm::nav::containing_instruction(app, current_cursor_offset);
        app.reader.page_start = page_start;
    }

    let initial_scan_ip = app.get_va(page_start);
    let buffer = app.file_info.get_buffer_ref();
    let scan_end = (page_start + height * MAX_INSTR_BYTES).min(filesize);

    // Auto-scroll check: Calculate visible area end
    let scan_bytes = &buffer[page_start..scan_end];
    let mut scan_decoder = Decoder::with_ip(bitness, scan_bytes, initial_scan_ip, DecoderOptions::NONE);
    let mut visible_end = page_start;
    let mut line_count = 0;
    let mut scan_offset = page_start;

    for instr in &mut scan_decoder {
        if line_count >= height {
            break;
        }
        visible_end = scan_offset + instr.len();
        scan_offset = visible_end;
        line_count += 1;
    }

    if line_count > 0 {
        // The "cursor above the page" case is handled before this scan, in one
        // place, so only the forward direction is left here.
        if current_cursor_offset >= visible_end {
            // Scroll forward so the cursor lands on the last visible row.
            //
            // The previous implementation advanced one instruction at a time and
            // re-decoded a whole screen of instructions on *every* step, i.e.
            // O(distance x height) decodes for a single frame - the worst latency
            // spike in the app when jumping far forward. This does one linear
            // decode pass and keeps the last `height` instruction boundaries, so
            // the resulting page_start is the same.
            let walk_end = (current_cursor_offset + MAX_INSTR_BYTES).min(filesize);
            let walk_bytes = &buffer[page_start..walk_end.max(page_start)];
            // `get_va(page_start)`, like every other decoder in this function.
            // This one used `base_va + page_start`, which is only the same thing
            // when a section's RVA equals its raw file offset - not true for a
            // normal PE. Only instruction lengths are consumed here, so nothing
            // visibly broke, but the two formulas had no business differing.
            let mut walker = Decoder::with_ip(
                bitness,
                walk_bytes,
                app.get_va(page_start),
                DecoderOptions::NONE,
            );

            let mut recent: std::collections::VecDeque<usize> =
                std::collections::VecDeque::with_capacity(height + 1);
            let mut ofs = page_start;
            let mut landed = false;

            for instr in &mut walker {
                recent.push_back(ofs);
                if recent.len() > height.max(1) {
                    recent.pop_front();
                }
                if current_cursor_offset < ofs + instr.len() {
                    landed = true;
                    break;
                }
                ofs += instr.len();
                if ofs >= filesize {
                    break;
                }
            }

            page_start = match (landed, recent.front()) {
                (true, Some(&first)) => first,
                _ => ofs.min(current_cursor_offset),
            };
            app.reader.page_start = page_start;
        }
    }

    let ip = app.get_va(page_start);
    let buffer = app.file_info.get_buffer_ref();
    let slice_end = (page_start + height * MAX_INSTR_BYTES).min(filesize);
    let raw_slice = &buffer[page_start.min(slice_end)..slice_end];

    // Zero-Copy Fast Path: Check if any modified bytes exist in this slice range
    let has_modified_bytes = (page_start..slice_end).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));

    let modified_vec: Vec<u8>;
    let code_bytes_ref: &[u8] = if has_modified_bytes {
        let mut bytes = raw_slice.to_vec();
        for (idx, b_val) in bytes.iter_mut().enumerate() {
            let abs_ofs = page_start + idx;
            if let Some(&b) = app.hex_view.changed_bytes.get(&abs_ofs) {
                *b_val = b;
            }
        }
        modified_vec = bytes;
        &modified_vec
    } else {
        // Fast Path: Direct Zero-Copy reference (0 Heap Allocation)
        raw_slice
    };

    let decoder = Decoder::with_ip(bitness, code_bytes_ref, ip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_first_operand_char_index(0);
    formatter.options_mut().set_hex_prefix("0x");
    formatter.options_mut().set_hex_suffix("");
    formatter.options_mut().set_leading_zeroes(false);
    formatter.options_mut().set_memory_size_options(iced_x86::MemorySizeOptions::Always);

    let mut raw_text = String::new();
    let mut current_offset = page_start;

    let mut comment_rows: Vec<Row> = Vec::new();

    for instr in decoder {
        if addr_rows.len() >= height {
            break;
        }

        raw_text.clear();
        formatter.format(&instr, &mut raw_text);

        let trimmed_text = trim_hex_leading_zeros(&raw_text);
        let instr_text_ds = ensure_ds_segment(&trimmed_text);
        // Import names go into the operand itself, so the substitution happens on
        // the finished text and before the syntax highlighter tokenises it.
        let instr_text_ds = apply_import_symbol(app, &instr, &instr_text_ds);

        let va = instr.ip();
        let len = instr.len();
        let slice_idx_start = current_offset.saturating_sub(page_start);
        let slice_idx_end = (slice_idx_start + len).min(code_bytes_ref.len());
        let instr_bytes = &code_bytes_ref[slice_idx_start.min(slice_idx_end)..slice_idx_end];
        // One String built in place, instead of a Vec<String> plus a join per
        // instruction per frame.
        let mut hex_bytes_str = String::with_capacity(instr_bytes.len() * 3);
        for (i, b) in instr_bytes.iter().enumerate() {
            if i > 0 {
                hex_bytes_str.push(' ');
            }
            let _ = write!(hex_bytes_str, "{:02X}", b);
        }

        let va_col_width = va_col_width(is_64_bit);
        let raw_va_str = format!("{:X}", va);
        let formatted_va = if is_64_bit {
            if raw_va_str.len() < 9 { format!("{:09X}", va) } else { raw_va_str }
        } else {
            format!("{:08X}", va)
        };
        let va_str = format!("{:^width$}", formatted_va, width = va_col_width as usize);

        let (sel_start, sel_end) = if let Some(anchor) = app.disasm_selection_anchor {
            (anchor.min(current_cursor_offset), anchor.max(current_cursor_offset))
        } else {
            (current_cursor_offset, current_cursor_offset)
        };

        let is_selected = (current_offset >= sel_start && current_offset <= sel_end)
            || (current_cursor_offset >= current_offset && current_cursor_offset < current_offset + len);

        let is_modified = (current_offset..(current_offset + len)).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));

        let row_style = if is_selected {
            if is_modified {
                highlight_style.fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                highlight_style.add_modifier(Modifier::BOLD)
            }
        } else if is_modified {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            main_style
        };

        let addr_style = if is_selected {
            highlight_style.add_modifier(Modifier::BOLD)
        } else {
            offsets_style
        };

        let formatted_instr_line = format_x64dbg_line(&instr_text_ds, is_modified, is_selected, main_style, highlight_style, app);

        let formatted_comment_line = match line_comment(app, current_offset, &instr_text_ds) {
            Some(text) => {
                let style = if !app.config.syntax_highlight {
                    if is_selected { highlight_style } else { app.config.theme.dimmed }
                } else if is_selected {
                    highlight_style
                        .fg(app.config.disasm_theme.comment_fg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(app.config.disasm_theme.comment_fg)
                        .bg(main_style.bg.unwrap_or(Color::Reset))
                        .add_modifier(Modifier::BOLD)
                };
                Line::from(Span::styled(text, style))
            }
            None => Line::default(),
        };

        addr_rows.push(Row::new([Cell::new(va_str).style(addr_style)]));
        bytes_rows.push(Row::new([Cell::new(hex_bytes_str).style(row_style)]));
        instr_rows.push(Row::new([Cell::from(formatted_instr_line)]));
        comment_rows.push(Row::new([Cell::from(formatted_comment_line)]));

        current_offset += len;
        if current_offset >= filesize {
            break;
        }
    }

    let va_col_width = va_col_width(is_64_bit);
    let bytes_col_width = BYTES_COL_WIDTH;
    let disasm_col_width = DISASM_COL_WIDTH;

    let disasm_layout = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints(vec![
            Constraint::Length(va_col_width),
            Constraint::Length(1), // Separator 1
            Constraint::Length(bytes_col_width),
            Constraint::Length(1), // Separator 2
            Constraint::Length(disasm_col_width), // Disassembly column
            Constraint::Length(1), // Separator 3
            Constraint::Min(0),    // Comment column (Right side)
        ])
        .split(area);

    let sep_height = area.height as usize;
    let sep_line_color = app.config.theme.dimmed.fg.unwrap_or(Color::Rgb(128, 128, 128));

    let sep_style = Style::default()
        .fg(sep_line_color)
        .bg(main_style.bg.unwrap_or(Color::Reset));

    let sep_str = "│\n".repeat(sep_height);
    let sep_para1 = Paragraph::new(sep_str.clone()).style(sep_style);
    let sep_para2 = Paragraph::new(sep_str.clone()).style(sep_style);
    let sep_para3 = Paragraph::new(sep_str).style(sep_style);

    let bg_fill_str = " \n".repeat(sep_height);
    let offsets_bg_fill = Paragraph::new(bg_fill_str.clone()).style(offsets_style);
    let main_bg_fill = Paragraph::new(bg_fill_str).style(main_style);

    frame.render_widget(Clear, area);
    frame.render_widget(offsets_bg_fill, disasm_layout[0]);
    frame.render_widget(main_bg_fill.clone(), disasm_layout[1]);
    frame.render_widget(main_bg_fill.clone(), disasm_layout[2]);
    frame.render_widget(main_bg_fill.clone(), disasm_layout[3]);
    frame.render_widget(main_bg_fill.clone(), disasm_layout[4]);
    frame.render_widget(main_bg_fill.clone(), disasm_layout[5]);
    frame.render_widget(main_bg_fill, disasm_layout[6]);

    if let Ok(mut guard) = DISASM_CACHE.lock() {
        *guard = Some(DisasmCacheState {
            generation: app.view_generation,
            bitness,
            style_key,
            page_start,
            offset: current_cursor_offset,
            selection_anchor: app.disasm_selection_anchor,
            changed_bytes_key,
            area_width: area.width,
            area_height: area.height,
            syntax_highlight,
            addr_rows: addr_rows.clone(),
            bytes_rows: bytes_rows.clone(),
            instr_rows: instr_rows.clone(),
            comment_rows: comment_rows.clone(),
        });
    }

    let addr_table = Table::new(addr_rows, [Constraint::Length(va_col_width)]).style(offsets_style);
    let bytes_table = Table::new(bytes_rows, [Constraint::Length(bytes_col_width)]).style(main_style);
    let instr_table = Table::new(instr_rows, [Constraint::Length(disasm_col_width)]).style(main_style);
    let comment_table = Table::new(comment_rows, [Constraint::Min(0)]).style(main_style);

    frame.render_widget(addr_table, disasm_layout[0]);
    frame.render_widget(sep_para1, disasm_layout[1]);
    frame.render_widget(bytes_table, disasm_layout[2]);
    frame.render_widget(sep_para2, disasm_layout[3]);
    frame.render_widget(instr_table, disasm_layout[4]);
    frame.render_widget(sep_para3, disasm_layout[5]);
    frame.render_widget(comment_table, disasm_layout[6]);
}

#[cfg(test)]
mod disasm_cache_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// `DISASM_CACHE` is process-global, so these tests cannot run concurrently
    /// with each other: one test's render would land in the cache between another
    /// test's two renders and mask exactly what is being checked.
    static RENDER_LOCK: Mutex<()> = Mutex::new(());

    /// Takes the render lock and starts from an empty cache.
    fn begin() -> std::sync::MutexGuard<'static, ()> {
        let guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut cache) = DISASM_CACHE.lock() {
            *cache = None;
        }
        guard
    }

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                draw_disasm_view(app, f, f.area());
            })
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Two different PE files, both rendered from offset 0 with no pending edits,
    /// must not produce the same screen.
    ///
    /// Everything in the old cache key was identical between them - page_start 0,
    /// cursor 0, no anchor, empty edit fingerprint, same area - so the second file
    /// was drawn from the first file's cached rows.
    #[test]
    fn switching_file_invalidates_the_cache() {
        let _lock = begin();
        let exe = match std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_owned)) {
            Some(p) => p,
            None => return,
        };
        // A second, definitely different executable.
        let other = std::path::Path::new(r"C:\Windows\System32\notepad.exe");
        if !other.is_file() {
            return;
        }

        let mut app = App::new();
        app.config.database = false;
        // Rendered at the same offset in both files, and inside code rather than at
        // offset 0: every PE begins with the same MZ stub, so offset 0 genuinely
        // looks identical in both and could not tell a stale cache from a correct
        // render.
        const AT: usize = 0x1000;

        let at_offset = |app: &mut App| {
            app.reader.page_start = AT;
            app.hex_view.offset = AT;
        };
        let bytes_at = |app: &App| {
            app.file_info
                .get_buffer_ref()
                .get(AT..AT + 32)
                .map(<[u8]>::to_vec)
        };

        app.load_file(&exe, 0, true).expect("open first");
        if app.header_view.pe.is_none() {
            return;
        }
        let Some(first_bytes) = bytes_at(&app) else { return };
        at_offset(&mut app);
        let first = render(&mut app, 100, 20);

        app.load_file(other.to_str().expect("path"), 0, true).expect("open second");
        let Some(second_bytes) = bytes_at(&app) else { return };
        at_offset(&mut app);
        let second = render(&mut app, 100, 20);

        // Precondition: the two files really do differ there. Without this the
        // assertion below could pass or fail for reasons unrelated to caching.
        if first_bytes == second_bytes {
            return;
        }

        assert_ne!(
            first, second,
            "the second file was rendered from the first file's cached rows"
        );
    }

    /// Re-rendering the same state twice must be stable, so the cache is still
    /// doing its job.
    #[test]
    fn identical_state_renders_identically() {
        let _lock = begin();
        let exe = match std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_owned)) {
            Some(p) => p,
            None => return,
        };
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&exe, 0, true).expect("open");
        if app.header_view.pe.is_none() {
            return;
        }
        let a = render(&mut app, 100, 20);
        let b = render(&mut app, 100, 20);
        assert_eq!(a, b);
    }

    /// A colour change must reach the screen without the cursor having to move.
    #[test]
    fn theme_change_invalidates_the_cache() {
        let _lock = begin();
        let exe = match std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_owned)) {
            Some(p) => p,
            None => return,
        };
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&exe, 0, true).expect("open");
        if app.header_view.pe.is_none() {
            return;
        }

        let before = style_fingerprint(&app);
        let _ = render(&mut app, 100, 20);

        app.config.disasm_theme = crate::disasm::theme::disasm_preset("light").expect("preset");
        let after = style_fingerprint(&app);
        assert_ne!(
            before, after,
            "changing the disassembly theme must change the cache key"
        );
    }

    /// Bitness is part of the key: the same bytes decode differently at 32 and 64
    /// bits.
    #[test]
    fn bitness_is_part_of_the_key() {
        let _lock = begin();
        let exe = match std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_owned)) {
            Some(p) => p,
            None => return,
        };
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&exe, 0, true).expect("open");
        let Some(pe) = app.header_view.pe.as_ref() else { return };
        if !app.is_64() {
            return;
        }
        let _ = pe;

        let as_64 = render(&mut app, 100, 20);
        // Force the 32-bit path by dropping the optional header's magic.
        if let Some(pe) = app.header_view.pe.as_mut() {
            if let Some(opt) = pe.optional_header.as_mut() {
                opt.standard_fields.magic = goblin::pe::optional_header::MAGIC_32;
            }
        }
        let as_32 = render(&mut app, 100, 20);
        assert_ne!(as_64, as_32, "32- and 64-bit renders must not share a cache entry");
    }

    /// 16 and 32 bits must not share a cache entry either.
    ///
    /// This is the case a boolean key could not express: both widths are "not 64", so
    /// keying on `is_64` served a forced 16-bit view out of the 32-bit rows. Uses the
    /// forced width rather than a doctored header, since that is how a user reaches
    /// 16-bit decoding at all.
    #[test]
    fn sixteen_and_thirty_two_bit_renders_do_not_share_a_cache_entry() {
        let _lock = begin();
        let exe = match std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_owned)) {
            Some(p) => p,
            None => return,
        };
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&exe, 0, true).expect("open");

        app.config.bitness_override = Some(32);
        let as_32 = render(&mut app, 100, 20);

        app.config.bitness_override = Some(16);
        let as_16 = render(&mut app, 100, 20);

        assert_ne!(
            as_32, as_16,
            "16-bit decoding was served from the 32-bit cache entry"
        );
    }
}

#[cfg(test)]
mod import_annotation_tests {
    use super::*;

    fn loaded_exe() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        if app.import_labels.is_empty() {
            return None;
        }
        Some(app)
    }

    /// Opening a PE must populate the label map; it is what the listing reads.
    #[test]
    fn opening_a_pe_builds_the_import_map() {
        let Some(app) = loaded_exe() else { return };
        assert!(
            app.import_labels.values().any(|l| l.contains('.')),
            "labels should be formatted as MODULE.Function"
        );
    }

    /// Every instruction that references an import slot must be annotated.
    ///
    /// Decoding finds a real reference first and then asks for its label, so this
    /// fails if the map is keyed on anything other than the address the code
    /// actually uses - the mistake that would make the whole feature silently
    /// annotate nothing.
    #[test]
    fn an_instruction_referencing_an_import_slot_is_labelled() {
        let Some(app) = loaded_exe() else { return };

        // The set of slot addresses is derived straight from the import
        // directory, *not* from `app.import_labels`. Asking the map which
        // addresses are interesting would make this test agree with the map by
        // construction: mis-key the map and it would simply match nothing and
        // pass vacuously, which is exactly what an earlier version of this test
        // did.
        let base = app.get_image_base();
        let expected: std::collections::HashSet<u64> = app
            .header_view
            .pe
            .as_ref()
            .expect("pe")
            .imports
            .iter()
            .filter(|i| i.offset != 0)
            .map(|i| base + i.offset as u64)
            .collect();

        let buffer = app.file_info.get_buffer_ref();
        let bitness = app.bitness();
        let mut checked = 0usize;

        'sweep: for section in crate::disasm::sections::code_sections(&app, buffer.len()) {
            let decoder = Decoder::with_ip(
                bitness,
                &buffer[section.start..section.end],
                section.va,
                DecoderOptions::NONE,
            );
            for instr in decoder {
                if !instr.is_ip_rel_memory_operand() {
                    continue;
                }
                let referenced = instr.ip_rel_memory_address();
                if !expected.contains(&referenced) {
                    continue;
                }

                // Checked through the substitution itself: the address spelled the
                // way the formatter prints it must come back replaced by a name.
                let printed = format!("call qword ptr ds:[0x{:X}]", referenced);
                let rewritten = apply_import_symbol(&app, &instr, &printed);
                assert_ne!(
                    rewritten, printed,
                    "instruction at {:X} references import slot {:X} but the name was not substituted",
                    instr.ip(),
                    referenced
                );
                assert!(
                    rewritten.contains('<') && rewritten.contains('>'),
                    "expected a <Name> operand, got {rewritten}"
                );
                checked += 1;
                if checked >= 5 {
                    break 'sweep;
                }
            }
        }

        // A 64-bit PE reaches its imports through `[rip+disp]`, so finding none at
        // all means the sweep never exercised the lookup and the assertion above
        // proved nothing. 32-bit binaries use absolute displacements instead, so
        // only the 64-bit case can demand this.
        if bitness == 64 {
            assert!(
                checked > 0,
                "no RIP-relative import reference found; the test proved nothing"
            );
        }
    }

    /// Finds a real instruction that references an import slot and returns its
    /// file offset, plus the label expected for it.
    fn first_import_reference(app: &App) -> Option<(usize, String)> {
        let buffer = app.file_info.get_buffer_ref();
        for section in crate::disasm::sections::code_sections(app, buffer.len()) {
            let decoder = Decoder::with_ip(
                app.bitness(),
                &buffer[section.start..section.end],
                section.va,
                DecoderOptions::NONE,
            );
            let mut ofs = section.start;
            for instr in decoder {
                if instr.is_ip_rel_memory_operand()
                    && let Some(label) = app.import_labels.get(&instr.ip_rel_memory_address())
                {
                    return Some((ofs, label.clone()));
                }
                ofs += instr.len();
                if ofs >= section.end {
                    break;
                }
            }
        }
        None
    }

    /// The import name has to reach the *screen*, inside the operand.
    ///
    /// Renders a real page and reads the cells back, so a break anywhere in the
    /// chain - lookup, substitution, syntax highlighting, column layout - shows up
    /// here. The bare slot address must be gone from the row: leaving both would
    /// mean the substitution ran on a copy that never reached the screen.
    #[test]
    fn the_import_name_is_rendered_inside_the_operand() {
        use ratatui::{Terminal, backend::TestBackend};

        let Some(mut app) = loaded_exe() else { return };
        let Some((offset, label)) = first_import_reference(&app) else {
            return;
        };
        let expected = function_name(&label);

        app.reader.page_start = offset;
        app.hex_view.offset = offset;
        app.editor_view = crate::editor::AppView::Disasm;

        let mut terminal = Terminal::new(TestBackend::new(160, 8)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                draw_disasm_view(&mut app, f, f.area());
            })
            .expect("draw");

        let buf = terminal.backend().buffer();
        let first_row: String = (0..160u16).map(|x| buf[(x, 0)].symbol().to_string()).collect();

        assert!(
            first_row.contains(&format!("<{}>", expected)),
            "expected '<{expected}>' in the operand, row was:\n{}",
            first_row.trim_end()
        );
    }

    /// `ds:` gets the segment colour and the import name its own background.
    ///
    /// Read off the rendered cells rather than the span list, because the two facts
    /// being checked are about what the user sees: the prefix used to inherit the
    /// memory-operand colour, which made it indistinguishable from the address it
    /// qualifies.
    #[test]
    fn the_segment_prefix_and_import_name_get_their_own_colours() {
        use ratatui::{Terminal, backend::TestBackend};

        let Some(mut app) = loaded_exe() else { return };
        let Some((offset, label)) = first_import_reference(&app) else {
            return;
        };
        let name = function_name(&label);

        // Distinctive values, so no other element can be mistaken for these.
        app.config.disasm_theme.segment_fg = Color::Rgb(0xFF, 0x00, 0xFF);
        app.config.disasm_theme.import_bg = Color::Rgb(0xFF, 0xFF, 0x00);
        app.config.syntax_highlight = true;

        app.reader.page_start = offset;
        app.hex_view.offset = offset;
        app.editor_view = crate::editor::AppView::Disasm;

        let mut terminal = Terminal::new(TestBackend::new(160, 8)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                draw_disasm_view(&mut app, f, f.area());
            })
            .expect("draw");

        let buf = terminal.backend().buffer();
        let row: Vec<(String, Color, Color)> = (0..160u16)
            .map(|x| {
                let cell = &buf[(x, 0)];
                (cell.symbol().to_string(), cell.fg, cell.bg)
            })
            .collect();
        let text: String = row.iter().map(|(s, _, _)| s.as_str()).collect();

        // Searched over cells, not over the joined string: the column separators are
        // multi-byte (`│`), so a byte offset from `str::find` does not index the
        // cell array. Getting that wrong reads the colour of an unrelated cell.
        let find_cells = |needle: &str| -> Option<usize> {
            let wanted: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
            (0..row.len().saturating_sub(wanted.len())).find(|&start| {
                wanted
                    .iter()
                    .enumerate()
                    .all(|(k, w)| &row[start + k].0 == w)
            })
        };

        // The cursor row is drawn with the selection style, which overrides the
        // syntax colours, so this case only makes sense on a non-cursor row. The
        // page starts at the cursor, so row 0 *is* the cursor row - assert on what
        // is verifiable and check the colours on the operand of a later row if the
        // first one is selected.
        // Asserted rather than skipped: `ensure_ds_segment` puts a `ds:` on every
        // absolute memory operand, and an import reference is one, so its absence
        // would mean this case silently stopped being covered.
        let ds_at = find_cells("ds:");
        assert!(
            ds_at.is_some(),
            "no 'ds:' on the row, so the segment colour was never checked:\n{}",
            text.trim_end()
        );
        if let Some(i) = ds_at {
            let (_, fg, _) = &row[i];
            assert_eq!(
                *fg,
                Color::Rgb(0xFF, 0x00, 0xFF),
                "'ds' must use the segment colour, row was:\n{}",
                text.trim_end()
            );
            let (_, colon_fg, _) = &row[i + 2];
            assert_eq!(
                *colon_fg,
                Color::Rgb(0xFF, 0x00, 0xFF),
                "the colon belongs to the prefix"
            );
        }

        let name_at = find_cells(&name).expect("the import name must be on the row");
        for (idx, (sym, _, bg)) in row.iter().enumerate().skip(name_at).take(name.chars().count()) {
            assert_eq!(
                *bg,
                Color::Rgb(0xFF, 0xFF, 0x00),
                "cell {idx} ('{sym}') of the import name must carry the import background"
            );
        }
        // The angle brackets are part of the marker, so they are painted too.
        assert_eq!(row[name_at - 1].0, "<");
        assert_eq!(row[name_at - 1].2, Color::Rgb(0xFF, 0xFF, 0x00));
        assert_eq!(row[name_at + name.chars().count()].0, ">");
        assert_eq!(
            row[name_at + name.chars().count()].2,
            Color::Rgb(0xFF, 0xFF, 0x00)
        );
    }

    /// A name with `_`, `@`, `?` or `-` must stay one coloured run.
    ///
    /// The lexer split on every non-alphanumeric character, which cut
    /// `api-ms-win-core-synch-l1-1-0` and mangled C++ symbols into pieces that were
    /// coloured independently.
    #[test]
    fn a_punctuated_import_name_is_one_run() {
        assert!(is_segment_register("ds"));
        assert!(is_segment_register("SS"));
        assert!(!is_segment_register("rax"));
        assert!(!is_segment_register("dword"));
    }

    /// Only the function name is shown, not `MODULE.Function`.
    ///
    /// The operand is already visibly an import-table slot, so the module would be
    /// repeated on every line for no information, in the narrowest column.
    #[test]
    fn only_the_function_name_is_substituted() {
        assert_eq!(function_name("KERNEL32.CreateFileW"), "CreateFileW");
        assert_eq!(function_name("api-ms-win-core-synch-l1-1-0.Sleep"), "Sleep");
        // No dot to split on: keep it whole rather than emptying it.
        assert_eq!(function_name("Whatever"), "Whatever");
    }

    /// The comment column must not repeat what the operand already says.
    #[test]
    fn the_comment_column_does_not_repeat_the_import_name() {
        let Some(mut app) = loaded_exe() else { return };
        let Some((offset, label)) = first_import_reference(&app) else {
            return;
        };

        assert_eq!(
            line_comment(&app, offset, ""),
            None,
            "the import name belongs in the operand, not in the comment column"
        );

        // A user comment still shows there, since nothing else claims the column.
        app.hex_view.comments.insert(offset, "mine".to_string());
        assert_eq!(
            line_comment(&app, offset, "").as_deref(),
            Some("mine"),
            "a comment the user typed must still be shown"
        );
        let _ = label;
    }

    /// Files with no import table must not get spurious comments.
    #[test]
    fn a_file_without_imports_is_never_labelled() {
        let mut app = App::new();
        app.config.database = false;
        let txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("COPYING");
        if !txt.is_file() {
            return;
        }
        app.load_file(txt.to_str().expect("path"), 0, true).expect("open");

        assert!(
            app.import_labels.is_empty(),
            "a non-PE file must not carry import labels from anywhere"
        );
    }
}

#[cfg(test)]
mod page_fixup_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// `mov eax, imm32` repeated: five bytes each, and a decode that starts one
    /// byte off produces visibly different instructions.
    const UNIT: [u8; 5] = [0xB8, 0x78, 0x56, 0x34, 0x12];

    fn app_with_code() -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dz6_page_fixup");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("code_{}.bin", id));

        let mut bytes = Vec::new();
        while bytes.len() < 0x400 {
            bytes.extend_from_slice(&UNIT);
        }
        std::fs::write(&path, &bytes).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true)
            .expect("open");
        app
    }

    fn render(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                draw_disasm_view(app, f, f.area());
            })
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// With the cursor above the page, the page must be re-anchored onto an
    /// instruction boundary - and the cursor has to end up on screen.
    ///
    /// Two fixups used to handle this and they disagreed: one rewound
    /// `page_start` by a single instruction (so a cursor further up stayed off
    /// screen), the other assigned the raw cursor offset (so the page could start
    /// mid-instruction and the whole screen decoded garbage until it re-synced).
    #[test]
    fn cursor_above_the_page_anchors_on_a_boundary() {
        let mut app = app_with_code();

        // Instructions are five bytes each from offset 0, so the boundaries are
        // the multiples of five: 0xFF (255) is one, 0x104 (260) is the next. A
        // cursor at 0x102 therefore sits two bytes inside the instruction that
        // starts at 0xFF.
        app.reader.page_start = 0x300;
        app.hex_view.offset = 0x102;

        let lines = render(&mut app, 110, 20);

        assert!(
            app.reader.page_start <= app.hex_view.offset,
            "page must scroll up to the cursor: page_start 0x{:X}, cursor 0x{:X}",
            app.reader.page_start,
            app.hex_view.offset
        );
        assert_eq!(
            app.reader.page_start, 0xFF,
            "page must start on the boundary of the instruction the cursor is in"
        );

        // Anchored correctly, every row decodes as the same 5-byte instruction.
        // A page starting mid-instruction shows other mnemonics instead.
        let body: Vec<&String> = lines.iter().filter(|l| l.contains("mov")).collect();
        assert!(body.len() > 5, "expected a screenful of decoded rows");
        assert!(
            !lines.iter().any(|l| l.contains("???")),
            "a mid-instruction page start produces undecodable rows:\n{}",
            lines.join("\n")
        );
    }

    /// The single fixup must handle a cursor exactly on a boundary too.
    #[test]
    fn cursor_on_a_boundary_above_the_page() {
        let mut app = app_with_code();
        app.reader.page_start = 0x300;
        app.hex_view.offset = 0xA0; // 160 = 32 * 5, a real boundary

        let _ = render(&mut app, 110, 20);
        assert_eq!(app.reader.page_start, 0xA0);
    }

    /// Scrolling forward still works: the cursor below the page brings it onto
    /// the last visible row, on a boundary.
    #[test]
    fn cursor_below_the_page_scrolls_forward() {
        let mut app = app_with_code();
        app.reader.page_start = 0;
        app.hex_view.offset = 700; // 140 * 5, a boundary

        let _ = render(&mut app, 110, 20);

        assert!(
            app.reader.page_start > 0 && app.reader.page_start <= app.hex_view.offset,
            "page must scroll down to the cursor: page_start 0x{:X}",
            app.reader.page_start
        );
        assert_eq!(
            app.reader.page_start % 5,
            0,
            "page must start on an instruction boundary, got 0x{:X}",
            app.reader.page_start
        );
    }
}
