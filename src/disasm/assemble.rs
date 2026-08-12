use iced_x86::{Code, Encoder, Instruction, Register};
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::Alignment,
    widgets::{Block, Clear, Paragraph},
};
use std::io::Result;

use crate::{app::App, editor::UIState, util::center_widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegSize {
    R64,
    R32,
    R16,
    R8,
}

fn get_reg_size(reg: Register) -> RegSize {
    let u = reg as u32;
    if (Register::RAX as u32..=Register::R15 as u32).contains(&u) {
        RegSize::R64
    } else if (Register::EAX as u32..=Register::R15D as u32).contains(&u) {
        RegSize::R32
    } else if (Register::AX as u32..=Register::R15W as u32).contains(&u) {
        RegSize::R16
    } else {
        RegSize::R8
    }
}

fn parse_reg(s: &str) -> Option<Register> {
    match s.trim().to_lowercase().as_str() {
        "rax" => Some(Register::RAX),
        "rcx" => Some(Register::RCX),
        "rdx" => Some(Register::RDX),
        "rbx" => Some(Register::RBX),
        "rsp" => Some(Register::RSP),
        "rbp" => Some(Register::RBP),
        "rsi" => Some(Register::RSI),
        "rdi" => Some(Register::RDI),
        "r8"  => Some(Register::R8),
        "r9"  => Some(Register::R9),
        "r10" => Some(Register::R10),
        "r11" => Some(Register::R11),
        "r12" => Some(Register::R12),
        "r13" => Some(Register::R13),
        "r14" => Some(Register::R14),
        "r15" => Some(Register::R15),

        "eax" => Some(Register::EAX),
        "ecx" => Some(Register::ECX),
        "edx" => Some(Register::EDX),
        "ebx" => Some(Register::EBX),
        "esp" => Some(Register::ESP),
        "ebp" => Some(Register::EBP),
        "esi" => Some(Register::ESI),
        "edi" => Some(Register::EDI),
        "r8d" => Some(Register::R8D),
        "r9d" => Some(Register::R9D),
        "r10d" => Some(Register::R10D),
        "r11d" => Some(Register::R11D),
        "r12d" => Some(Register::R12D),
        "r13d" => Some(Register::R13D),
        "r14d" => Some(Register::R14D),
        "r15d" => Some(Register::R15D),

        "ax"  => Some(Register::AX),
        "cx"  => Some(Register::CX),
        "dx"  => Some(Register::DX),
        "bx"  => Some(Register::BX),
        "sp"  => Some(Register::SP),
        "bp"  => Some(Register::BP),
        "si"  => Some(Register::SI),
        "di"  => Some(Register::DI),

        "al"  => Some(Register::AL),
        "cl"  => Some(Register::CL),
        "dl"  => Some(Register::DL),
        "bl"  => Some(Register::BL),
        "ah"  => Some(Register::AH),
        "ch"  => Some(Register::CH),
        "dh"  => Some(Register::DH),
        "bh"  => Some(Register::BH),

        _ => None,
    }
}

fn encode_instruction(instr: &Instruction, bitness: u32) -> Option<Vec<u8>> {
    encode_instruction_at(instr, bitness, 0)
}

fn encode_instruction_at(instr: &Instruction, bitness: u32, ip: u64) -> Option<Vec<u8>> {
    let mut encoder = Encoder::new(bitness);
    if encoder.encode(instr, ip).is_ok() {
        Some(encoder.take_buffer())
    } else {
        None
    }
}

fn parse_imm(text: &str) -> Option<i128> {
    let s = text.trim();
    let (negative, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, s.strip_prefix('+').unwrap_or(s).trim()),
    };
    if s.is_empty() {
        return None;
    }

    let magnitude = if let Some(decimal) = s.strip_suffix('t').or_else(|| s.strip_suffix('T')) {
        let decimal = decimal.trim();
        if decimal.is_empty() || !decimal.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        decimal.parse::<i128>().ok()?
    } else {
        let hex = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        let hex = hex
            .strip_suffix('h')
            .or_else(|| hex.strip_suffix('H'))
            .unwrap_or(hex);
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        i128::from_str_radix(hex, 16).ok()?
    };

    Some(if negative { -magnitude } else { magnitude })
}

fn fit_imm(value: i128, bits: u32) -> Option<i64> {
    let unsigned_max = (1i128 << bits) - 1;
    let signed_min = -(1i128 << (bits - 1));
    if value >= 0 && value <= unsigned_max {
        return Some(value as i64);
    }
    if value < 0 && value >= signed_min {
        return Some(value as i64);
    }
    None
}

fn covering_span(app: &App, offset: usize, len: usize) -> Option<usize> {
    let mut span = crate::disasm::nav::instruction_len(app, offset)?;
    let mut cursor = offset.saturating_add(span);

    for _ in 0..MAX_PATCH_INSTRUCTIONS {
        if span >= len {
            return Some(span);
        }
        let next = crate::disasm::nav::instruction_len(app, cursor)?;
        span = span.saturating_add(next);
        cursor = cursor.saturating_add(next);
    }

    if span >= len { Some(span) } else { None }
}

const MAX_PATCH_INSTRUCTIONS: usize = 16;

pub fn stage_assembled_bytes(
    app: &mut App,
    offset: usize,
    bytes: &[u8],
) -> std::result::Result<String, String> {
    if bytes.is_empty() {
        return Err(crate::i18n::M::ErrNothingToAssemble.tr(app.config.lang).to_string());
    }

    if app.file_info.is_read_only {
        return Err(crate::i18n::M::ErrFileReadOnly.tr(app.config.lang).to_string());
    }

    let new_len = bytes.len();
    let limit = app.file_info.buffer_len();
    let span = covering_span(app, offset, new_len).unwrap_or(new_len).max(new_len);

    if offset.checked_add(span).is_none_or(|end| end > limit) {
        return Err(crate::i18n::M::ErrAssemblePastEof.tr(app.config.lang).to_string());
    }

    for (i, &b) in bytes.iter().enumerate() {
        let target = offset + i;
        crate::hex::edit::record_edit(app, target, b);
    }

    let padding = span - new_len;
    for i in new_len..span {
        let target = offset + i;
        crate::hex::edit::record_edit(app, target, 0x90);
    }

    let va = app.get_va(offset);
    Ok(if padding > 0 {
        format!(
            "Assembled {} byte(s) at 0x{:X}, padded with {} NOP(s) to the next instruction boundary",
            new_len, va, padding
        )
    } else {
        format!("Assembled {} byte(s) at 0x{:X}", new_len, va)
    })
}

pub fn parse_assemble_input(input: &str, bitness: u32, ip: u64) -> Option<Vec<u8>> {
    let clean = input.trim();
    if clean.is_empty() {
        return None;
    }

    let single_clean = clean.trim_start_matches("0x").trim_start_matches("0X");
    if single_clean.len() % 2 == 0 && single_clean.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut bytes = Vec::new();
        for i in (0..single_clean.len()).step_by(2) {
            if let Ok(b) = u8::from_str_radix(&single_clean[i..i+2], 16) {
                bytes.push(b);
            }
        }
        if !bytes.is_empty() {
            return Some(bytes);
        }
    }

    let hex_tokens: Vec<&str> = clean.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .collect();

    let mut hex_bytes = Vec::new();
    let mut all_hex = true;

    for tok in &hex_tokens {
        let tok_clean = tok.trim_start_matches("0x").trim_start_matches("0X");
        if tok_clean.len() <= 2 && tok_clean.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(b) = u8::from_str_radix(tok_clean, 16) {
                hex_bytes.push(b);
                continue;
            }
        }
        all_hex = false;
        break;
    }

    if all_hex && !hex_bytes.is_empty() {
        return Some(hex_bytes);
    }

    let lower = clean.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let is_64 = bitness == 64;
    let op = tokens[0];

    match op {
        "nop" => return encode_instruction(&Instruction::with(Code::Nopd), bitness),
        "ret" | "retn" => return encode_instruction(&Instruction::with(if is_64 { Code::Retnq } else { Code::Retnd }), bitness),
        "int3" => return encode_instruction(&Instruction::with(Code::Int3), bitness),
        "hlt" => return encode_instruction(&Instruction::with(Code::Hlt), bitness),
        "clc" => return encode_instruction(&Instruction::with(Code::Clc), bitness),
        "stc" => return encode_instruction(&Instruction::with(Code::Stc), bitness),
        "cli" => return encode_instruction(&Instruction::with(Code::Cli), bitness),
        "sti" => return encode_instruction(&Instruction::with(Code::Sti), bitness),
        "cld" => return encode_instruction(&Instruction::with(Code::Cld), bitness),
        "std" => return encode_instruction(&Instruction::with(Code::Std), bitness),
        "leave" => return encode_instruction(&Instruction::with(if is_64 { Code::Leaveq } else { Code::Leaved }), bitness),
        "syscall" => return encode_instruction(&Instruction::with(Code::Syscall), bitness),
        "sysenter" => return encode_instruction(&Instruction::with(Code::Sysenter), bitness),
        "ud2" => return encode_instruction(&Instruction::with(Code::Ud2), bitness),

        "pop" => {
            if tokens.len() >= 2 {
                if let Some(reg) = parse_reg(tokens[1]) {
                    let code = match get_reg_size(reg) {
                        RegSize::R64 => Code::Pop_r64,
                        _ => Code::Pop_r32,
                    };
                    if let Ok(instr) = Instruction::with1(code, reg) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "push" => {
            if tokens.len() >= 2 {
                let target = tokens[1].trim();
                if let Some(reg) = parse_reg(target) {
                    let code = match get_reg_size(reg) {
                        RegSize::R64 => Code::Push_r64,
                        _ => Code::Push_r32,
                    };
                    if let Ok(instr) = Instruction::with1(code, reg) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                } else if let Some(val) = parse_imm(target).and_then(|v| fit_imm(v, 32)) {
                    let val = val as i32;
                    {
                        if let Ok(instr) = Instruction::with1(if is_64 { Code::Pushq_imm32 } else { Code::Pushd_imm32 }, val) {
                            if let Some(bytes) = encode_instruction(&instr, bitness) {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        "mov" => {
            let rest = lower.trim_start_matches("mov").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                let dst_reg = parse_reg(parts[0]);
                let src_reg = parse_reg(parts[1]);

                if let (Some(dst), Some(src)) = (dst_reg, src_reg) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Mov_rm64_r64,
                        RegSize::R32 => Code::Mov_rm32_r32,
                        RegSize::R16 => Code::Mov_rm16_r16,
                        RegSize::R8  => Code::Mov_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                } else if let Some(dst) = dst_reg {
                    let size = get_reg_size(dst);
                    let bits = match size {
                        RegSize::R64 => 64,
                        RegSize::R32 => 32,
                        RegSize::R16 => 16,
                        RegSize::R8 => 8,
                    };
                    if let Some(fitted) = parse_imm(parts[1]).and_then(|v| fit_imm(v, bits)) {
                        let val = fitted as u64;
                        let res = match size {
                            RegSize::R64 => Instruction::with2(Code::Mov_r64_imm64, dst, val),
                            RegSize::R32 => Instruction::with2(Code::Mov_r32_imm32, dst, fitted as i32),
                            RegSize::R16 => Instruction::with2(Code::Mov_r16_imm16, dst, fitted as i32),
                            RegSize::R8  => Instruction::with2(Code::Mov_r8_imm8, dst, fitted as i32),
                        };
                        if let Ok(instr) = res {
                            if let Some(bytes) = encode_instruction(&instr, bitness) {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        "xor" => {
            let rest = lower.trim_start_matches("xor").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1])) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Xor_rm64_r64,
                        RegSize::R32 => Code::Xor_rm32_r32,
                        RegSize::R16 => Code::Xor_rm16_r16,
                        RegSize::R8  => Code::Xor_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "add" => {
            let rest = lower.trim_start_matches("add").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1])) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Add_rm64_r64,
                        RegSize::R32 => Code::Add_rm32_r32,
                        RegSize::R16 => Code::Add_rm16_r16,
                        RegSize::R8  => Code::Add_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "sub" => {
            let rest = lower.trim_start_matches("sub").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1])) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Sub_rm64_r64,
                        RegSize::R32 => Code::Sub_rm32_r32,
                        RegSize::R16 => Code::Sub_rm16_r16,
                        RegSize::R8  => Code::Sub_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "cmp" => {
            let rest = lower.trim_start_matches("cmp").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1])) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Cmp_rm64_r64,
                        RegSize::R32 => Code::Cmp_rm32_r32,
                        RegSize::R16 => Code::Cmp_rm16_r16,
                        RegSize::R8  => Code::Cmp_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "test" => {
            let rest = lower.trim_start_matches("test").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1])) {
                    let code = match get_reg_size(dst) {
                        RegSize::R64 => Code::Test_rm64_r64,
                        RegSize::R32 => Code::Test_rm32_r32,
                        RegSize::R16 => Code::Test_rm16_r16,
                        RegSize::R8  => Code::Test_rm8_r8,
                    };
                    if let Ok(instr) = Instruction::with2(code, dst, src) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "inc" => {
            if tokens.len() >= 2 {
                if let Some(reg) = parse_reg(tokens[1]) {
                    let code = match get_reg_size(reg) {
                        RegSize::R64 => Code::Inc_rm64,
                        RegSize::R32 => Code::Inc_rm32,
                        RegSize::R16 => Code::Inc_rm16,
                        RegSize::R8  => Code::Inc_rm8,
                    };
                    if let Ok(instr) = Instruction::with1(code, reg) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "dec" => {
            if tokens.len() >= 2 {
                if let Some(reg) = parse_reg(tokens[1]) {
                    let code = match get_reg_size(reg) {
                        RegSize::R64 => Code::Dec_rm64,
                        RegSize::R32 => Code::Dec_rm32,
                        RegSize::R16 => Code::Dec_rm16,
                        RegSize::R8  => Code::Dec_rm8,
                    };
                    if let Ok(instr) = Instruction::with1(code, reg) {
                        if let Some(bytes) = encode_instruction(&instr, bitness) {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "lea" => {
            let rest = clean.get(3..).unwrap_or("").trim();
            if let Some((dst_str, src_str)) = rest.split_once(',') {
                if let Some(dst_reg) = parse_reg(dst_str) {
                    let clean_src = src_str.trim().trim_start_matches('[').trim_end_matches(']').trim();
                    let addr_text = clean_src
                        .rsplit_once('+')
                        .map(|(_, tail)| tail)
                        .unwrap_or(clean_src)
                        .trim();
                    if let Some(target) = parse_imm(addr_text).filter(|v| *v >= 0) {
                        let code = match get_reg_size(dst_reg) {
                            RegSize::R64 => Code::Lea_r64_m,
                            _ => Code::Lea_r32_m,
                        };
                        let mem =
                            iced_x86::MemoryOperand::with_base_displ(Register::RIP, target as i64);
                        if let Ok(instr) = Instruction::with2(code, dst_reg, mem) {
                            if let Some(bytes) = encode_instruction_at(&instr, bitness, ip) {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        _ => {}
    }

    None
}

pub fn dialog_assemble_draw(app: &mut App, frame: &mut Frame) {
    let width = 60.min(frame.area().width.saturating_sub(4)).max(30);
    let height = 3;
    let mut dialog_area = center_widget(width, height, frame.area());
    dialog_area.y = dialog_area.y.saturating_sub(4);

    frame.render_widget(Clear, dialog_area);

    let input_text = app.assemble_input.value();
    let cursor_pos = app.assemble_input.cursor();

    let paragraph = if app.assemble_selection_all && !input_text.is_empty() {
        use ratatui::text::{Line, Span};
        let line = Line::from(vec![
            Span::styled(input_text.to_string(), app.config.theme.highlight),
        ]);
        Paragraph::new(line)
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::AssembleTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    } else if let Some(anchor) = app.assemble_selection_anchor {
        use ratatui::text::{Line, Span};
        let (before, selected, after) = safe_slice_parts(input_text, anchor, cursor_pos);

        let line = Line::from(vec![
            Span::styled(before.to_string(), app.config.theme.dialog),
            Span::styled(selected.to_string(), app.config.theme.highlight),
            Span::styled(after.to_string(), app.config.theme.dialog),
        ]);
        Paragraph::new(line)
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::AssembleTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    } else {
        Paragraph::new(input_text.to_string())
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::AssembleTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    };

    frame.render_widget(paragraph, dialog_area);

    let cursor_x = dialog_area.x + 1 + app.assemble_input.cursor() as u16;
    let cursor_y = dialog_area.y + 1;
    if cursor_x < dialog_area.x + dialog_area.width - 1 {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn safe_slice_parts(text: &str, start_pos: usize, end_pos: usize) -> (&str, &str, &str) {
    let char_indices: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    let total_chars = char_indices.len();

    let s = start_pos.min(end_pos).min(total_chars);
    let e = start_pos.max(end_pos).min(total_chars);

    let b_start = if s < total_chars { char_indices[s] } else { text.len() };
    let b_end = if e < total_chars { char_indices[e] } else { text.len() };

    (&text[..b_start], &text[b_start..b_end], &text[b_end..])
}

pub fn dialog_assemble_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        let is_shift = key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::SHIFT);
        let is_ctrl = key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL);

        if is_ctrl && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C')) {
            let input_val = app.assemble_input.value();
            let text_to_copy = if app.assemble_selection_all {
                input_val.to_string()
            } else if let Some(anchor) = app.assemble_selection_anchor {
                let cursor = app.assemble_input.cursor();
                let (_, selected, _) = safe_slice_parts(input_val, anchor, cursor);
                selected.to_string()
            } else {
                input_val.to_string()
            };

            if !text_to_copy.is_empty() {
                if let Ok(cb) = &mut app.clipboard {
                    let _ = cb.set_text(text_to_copy);
                    App::log(app, "Copied assembly text to clipboard".to_string());
                }
            }
            return Ok(false);
        }

        if is_ctrl && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V')) {
            if let Ok(cb) = &mut app.clipboard {
                if let Ok(pasted) = cb.get_text() {
                    let clean_pasted = pasted.trim().replace('\n', " ").replace('\r', "");
                    let pasted_char_cnt = clean_pasted.chars().count();
                    if app.assemble_selection_all {
                        app.assemble_selection_all = false;
                        app.assemble_selection_anchor = None;
                        app.assemble_input = tui_input::Input::new(clean_pasted);
                    } else if let Some(anchor) = app.assemble_selection_anchor {
                        let cursor = app.assemble_input.cursor();
                        let val = app.assemble_input.value();
                        let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                        let before_char_cnt = before.chars().count();
                        let mut new_val = String::new();
                        new_val.push_str(before);
                        new_val.push_str(&clean_pasted);
                        new_val.push_str(after);
                        let new_cursor = before_char_cnt + pasted_char_cnt;
                        app.assemble_selection_anchor = None;
                        app.assemble_input = tui_input::Input::new(new_val).with_cursor(new_cursor);
                    } else {
                        let cursor = app.assemble_input.cursor();
                        let val = app.assemble_input.value();
                        let (before, _, after) = safe_slice_parts(val, cursor, cursor);
                        let before_char_cnt = before.chars().count();
                        let mut new_val = String::new();
                        new_val.push_str(before);
                        new_val.push_str(&clean_pasted);
                        new_val.push_str(after);
                        let new_cursor = before_char_cnt + pasted_char_cnt;
                        app.assemble_input = tui_input::Input::new(new_val).with_cursor(new_cursor);
                    }
                }
            }
            return Ok(false);
        }

        if is_shift {
            let cursor = app.assemble_input.cursor();
            let val_char_len = app.assemble_input.value().chars().count();
            if app.assemble_selection_anchor.is_none() {
                app.assemble_selection_anchor = Some(cursor);
            }
            app.assemble_selection_all = false;

            match key.code {
                KeyCode::Left => {
                    let new_cursor = cursor.saturating_sub(1);
                    app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(new_cursor);
                    return Ok(false);
                }
                KeyCode::Right => {
                    let new_cursor = (cursor + 1).min(val_char_len);
                    app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(new_cursor);
                    return Ok(false);
                }
                KeyCode::Home => {
                    app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(val_char_len);
                    return Ok(false);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            KeyCode::Enter => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                let input_str = app.assemble_input.value().to_string();
                let bitness = app.bitness();
                let offset = app.hex_view.offset;
                let va = app.get_va(offset);
                if let Some(bytes) = parse_assemble_input(&input_str, bitness, va) {
                    match stage_assembled_bytes(app, offset, &bytes) {
                        Ok(message) => {
                            App::log(app, message);
                            app.state = UIState::Normal;
                            app.dialog_renderer = None;
                        }
                        Err(reason) => {
                            let message = crate::i18n::fill(
                                crate::i18n::M::ErrRefusingAssemble.tr(app.config.lang),
                                &[&reason],
                            );
                            app.error(message);
                        }
                    }
                } else {
                    let message = crate::i18n::fill(
                        crate::i18n::M::ErrFailedAssemble.tr(app.config.lang),
                        &[&input_str],
                    );
                    app.error(message);
                }
            }
            KeyCode::Home => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(0);
            }
            KeyCode::End => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                let val_char_len = app.assemble_input.value().chars().count();
                app.assemble_input = tui_input::Input::new(app.assemble_input.value().to_string()).with_cursor(val_char_len);
            }
            KeyCode::Left => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.assemble_input, event);
            }
            KeyCode::Right => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.assemble_input, event);
            }
            KeyCode::Char(c) if app.assemble_selection_all => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                app.assemble_input = tui_input::Input::new(c.to_string());
            }
            KeyCode::Char(c) if app.assemble_selection_anchor.is_some() => {
                if let Some(anchor) = app.assemble_selection_anchor {
                    let cursor = app.assemble_input.cursor();
                    let val = app.assemble_input.value();
                    let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                    let before_char_cnt = before.chars().count();
                    let mut new_val = String::new();
                    new_val.push_str(before);
                    new_val.push(c);
                    new_val.push_str(after);
                    app.assemble_selection_anchor = None;
                    app.assemble_input = tui_input::Input::new(new_val).with_cursor(before_char_cnt + 1);
                }
            }
            KeyCode::Backspace | KeyCode::Delete if app.assemble_selection_all => {
                app.assemble_selection_all = false;
                app.assemble_selection_anchor = None;
                app.assemble_input = tui_input::Input::default();
            }
            KeyCode::Backspace | KeyCode::Delete if app.assemble_selection_anchor.is_some() => {
                if let Some(anchor) = app.assemble_selection_anchor {
                    let cursor = app.assemble_input.cursor();
                    let val = app.assemble_input.value();
                    let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                    let before_char_cnt = before.chars().count();
                    let mut new_val = String::new();
                    new_val.push_str(before);
                    new_val.push_str(after);
                    app.assemble_selection_anchor = None;
                    app.assemble_input = tui_input::Input::new(new_val).with_cursor(before_char_cnt);
                }
            }
            _ => {
                if app.assemble_selection_all {
                    app.assemble_selection_all = false;
                }
                if app.assemble_selection_anchor.is_some() {
                    app.assemble_selection_anchor = None;
                }
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.assemble_input, event);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod assemble_tests {
    use super::*;

    #[test]
    fn immediates_are_hex_by_default() {
        assert_eq!(parse_imm("10"), Some(0x10));
        assert_eq!(parse_imm("0x10"), Some(0x10));
        assert_eq!(parse_imm("0X10"), Some(0x10));
        assert_eq!(parse_imm("10h"), Some(0x10));
        assert_eq!(parse_imm("ff"), Some(0xff));
        assert_eq!(parse_imm("  1F  "), Some(0x1f));
    }

    #[test]
    fn t_suffix_means_decimal() {
        assert_eq!(parse_imm("10t"), Some(10));
        assert_eq!(parse_imm("10T"), Some(10));
        assert_eq!(parse_imm("255t"), Some(255));
        assert_ne!(parse_imm("10t"), parse_imm("10"));
    }

    #[test]
    fn signs_and_junk() {
        assert_eq!(parse_imm("-1"), Some(-1));
        assert_eq!(parse_imm("-10t"), Some(-10));
        assert_eq!(parse_imm("+20"), Some(0x20));
        assert_eq!(parse_imm(""), None);
        assert_eq!(parse_imm("-"), None);
        assert_eq!(parse_imm("0x"), None);
        assert_eq!(parse_imm("zz"), None);
        assert_eq!(parse_imm("12x"), None);
        assert_eq!(parse_imm("1.5"), None);
        assert_eq!(parse_imm("fft"), None);
    }

    #[test]
    fn immediates_fit_signed_or_unsigned() {
        assert_eq!(fit_imm(0xff, 8), Some(0xff));
        assert_eq!(fit_imm(-1, 8), Some(-1));
        assert_eq!(fit_imm(0x100, 8), None);
        assert_eq!(fit_imm(-129, 8), None);
        assert_eq!(fit_imm(0xffff_ffff, 32), Some(0xffff_ffff));
        assert_eq!(fit_imm(0x1_0000_0000, 32), None);
    }

    #[test]
    fn push_immediate_radix() {
        let hex = parse_assemble_input("push 10", 64, 0).expect("push 10");
        let dec = parse_assemble_input("push 10t", 64, 0).expect("push 10t");
        assert_ne!(hex, dec, "hex and decimal spellings must differ");
        assert_eq!(hex.last_chunk::<4>().map(|c| u32::from_le_bytes(*c)), Some(0x10));
        assert_eq!(dec.last_chunk::<4>().map(|c| u32::from_le_bytes(*c)), Some(10));
    }

    #[test]
    fn mov_immediate_radix_and_width() {
        let hex = parse_assemble_input("mov eax, 10", 64, 0).expect("mov hex");
        let dec = parse_assemble_input("mov eax, 10t", 64, 0).expect("mov dec");
        assert_eq!(hex.last_chunk::<4>().map(|c| u32::from_le_bytes(*c)), Some(0x10));
        assert_eq!(dec.last_chunk::<4>().map(|c| u32::from_le_bytes(*c)), Some(10));

        let neg = parse_assemble_input("mov eax, -1t", 64, 0).expect("mov -1");
        assert_eq!(neg.last_chunk::<4>().map(|c| u32::from_le_bytes(*c)), Some(0xFFFF_FFFF));

        assert!(
            parse_assemble_input("mov al, 1FF", 64, 0).is_none(),
            "0x1FF does not fit in an 8-bit register and must not be truncated to 0xFF"
        );
        assert!(parse_assemble_input("mov al, 7F", 64, 0).is_some());
    }

    #[test]
    fn lea_rip_relative_displacement_is_correct() {
        let ip = 0x1_4000_0000u64;
        let target = 0x1_4000_1000u64;

        let bytes = parse_assemble_input("lea rax, [0x140001000]", 64, ip).expect("lea");
        let disp = i32::from_le_bytes(*bytes.last_chunk::<4>().expect("disp32"));
        let next_ip = ip + bytes.len() as u64;
        assert_eq!(
            next_ip.wrapping_add(disp as i64 as u64),
            target,
            "rip + disp32 must land on the requested address"
        );

        let with_rip = parse_assemble_input("lea rax, [rip + 0x140001000]", 64, ip).expect("lea rip");
        assert_eq!(with_rip, bytes);
    }

    #[test]
    fn lea_displacement_depends_on_the_instruction_address() {
        let a = parse_assemble_input("lea rax, [0x140001000]", 64, 0x1_4000_0000).expect("a");
        let b = parse_assemble_input("lea rax, [0x140001000]", 64, 0x1_4000_0100).expect("b");
        assert_ne!(a, b, "the displacement must follow the instruction's address");
    }

    #[test]
    fn simple_forms_are_ip_independent() {
        for text in ["nop", "ret", "int3", "push rax", "xor eax, eax"] {
            let at_zero = parse_assemble_input(text, 64, 0);
            let at_va = parse_assemble_input(text, 64, 0x1_4000_0000);
            assert_eq!(at_zero, at_va, "'{}' must encode the same at any address", text);
            assert!(at_zero.is_some(), "'{}' must assemble", text);
        }
    }

    #[test]
    fn raw_hex_bytes_still_work() {
        assert_eq!(parse_assemble_input("909090", 64, 0), Some(vec![0x90, 0x90, 0x90]));
        assert_eq!(parse_assemble_input("31 c0", 64, 0), Some(vec![0x31, 0xc0]));
    }
}

#[cfg(test)]
mod patch_padding_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CODE: &[u8] = &[
        0x50,
        0xB8, 0x78, 0x56, 0x34, 0x12,
        0xC3,
        0xCC,
    ];

    static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_with_code() -> App {
        let dir = std::env::temp_dir().join("dz6_patch_pad");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let id = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("code_{id}.bin"));
        let mut bytes = CODE.to_vec();
        bytes.resize(0x100, 0x90);
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, false)
            .expect("open fixture");
        app.file_info.is_read_only = false;
        app
    }

    fn staged(app: &App, offset: usize) -> Option<u8> {
        app.hex_view
            .changed_bytes
            .get(&offset)
            .copied()
    }

    #[test]
    fn a_shorter_instruction_is_padded_with_nops() {
        let mut app = app_with_code();
        let bytes = parse_assemble_input("nop", 64, 0).expect("nop encodes");
        assert_eq!(bytes.len(), 1);

        let message = stage_assembled_bytes(&mut app, 1, &bytes).expect("staged");

        assert_eq!(staged(&app, 1), Some(0x90), "the new instruction");
        for ofs in 2..6 {
            assert_eq!(
                staged(&app, ofs),
                Some(0x90),
                "leftover operand byte at {ofs} must be NOPed out"
            );
        }
        assert_eq!(staged(&app, 6), None, "the next instruction must be untouched");
        assert!(message.contains("padded with 4 NOP"), "message was: {message}");
    }

    #[test]
    fn a_longer_instruction_rounds_up_to_whole_instructions() {
        let mut app = app_with_code();
        let bytes = parse_assemble_input("mov eax, 0x11223344", 64, 0).expect("mov encodes");
        assert_eq!(bytes.len(), 5);

        let message = stage_assembled_bytes(&mut app, 0, &bytes).expect("staged");

        for (i, &b) in bytes.iter().enumerate() {
            assert_eq!(staged(&app, i), Some(b), "byte {i} of the new instruction");
        }
        assert_eq!(
            staged(&app, 5),
            Some(0x90),
            "the tail of the instruction being overwritten must be NOPed"
        );
        assert_eq!(staged(&app, 6), None, "the `ret` after it must be untouched");
        assert!(message.contains("padded with 1 NOP"), "message was: {message}");
    }

    #[test]
    fn an_exact_fit_pads_nothing() {
        let mut app = app_with_code();
        let bytes = parse_assemble_input("int3", 64, 0).expect("int3 encodes");
        assert_eq!(bytes.len(), 1);

        let message = stage_assembled_bytes(&mut app, 0, &bytes).expect("staged");

        assert_eq!(staged(&app, 0), Some(0xCC));
        assert_eq!(staged(&app, 1), None, "nothing beyond the instruction");
        assert!(!message.contains("padded"), "message was: {message}");
    }

    #[test]
    fn padding_is_undoable() {
        let mut app = app_with_code();
        let bytes = parse_assemble_input("nop", 64, 0).expect("nop");
        stage_assembled_bytes(&mut app, 1, &bytes).expect("staged");

        assert_eq!(
            app.hex_view.changed_history.len(),
            5,
            "all five staged offsets must be on the undo history"
        );
        for ofs in 1..6 {
            assert!(
                app.hex_view.changed_history.contains(&ofs),
                "offset {ofs} is staged but cannot be undone"
            );
        }
    }

    #[test]
    fn a_patch_past_the_end_is_refused() {
        let mut app = app_with_code();
        let limit = app.file_info.buffer_len();
        let bytes = parse_assemble_input("mov eax, 1", 64, 0).expect("mov");

        let result = stage_assembled_bytes(&mut app, limit - 2, &bytes);

        assert!(result.is_err(), "expected a refusal, got {result:?}");
        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "a refused patch must stage nothing at all"
        );
    }

    #[test]
    fn a_read_only_file_is_refused() {
        let mut app = app_with_code();
        app.file_info.is_read_only = true;
        let bytes = parse_assemble_input("nop", 64, 0).expect("nop");

        assert!(stage_assembled_bytes(&mut app, 1, &bytes).is_err());
        assert!(app.hex_view.changed_bytes.is_empty());
    }
}
