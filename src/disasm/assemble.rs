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
        "r8w"  => Some(Register::R8W),
        "r9w"  => Some(Register::R9W),
        "r10w" => Some(Register::R10W),
        "r11w" => Some(Register::R11W),
        "r12w" => Some(Register::R12W),
        "r13w" => Some(Register::R13W),
        "r14w" => Some(Register::R14W),
        "r15w" => Some(Register::R15W),

        "al"  => Some(Register::AL),
        "cl"  => Some(Register::CL),
        "dl"  => Some(Register::DL),
        "bl"  => Some(Register::BL),
        "ah"  => Some(Register::AH),
        "ch"  => Some(Register::CH),
        "dh"  => Some(Register::DH),
        "bh"  => Some(Register::BH),
        "spl" => Some(Register::SPL),
        "bpl" => Some(Register::BPL),
        "sil" => Some(Register::SIL),
        "dil" => Some(Register::DIL),
        "r8b" | "r8l"   => Some(Register::R8L),
        "r9b" | "r9l"   => Some(Register::R9L),
        "r10b" | "r10l" => Some(Register::R10L),
        "r11b" | "r11l" => Some(Register::R11L),
        "r12b" | "r12l" => Some(Register::R12L),
        "r13b" | "r13l" => Some(Register::R13L),
        "r14b" | "r14l" => Some(Register::R14L),
        "r15b" | "r15l" => Some(Register::R15L),

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

fn parse_branch_target(target_str: &str, ip: u64) -> Option<u64> {
    let s = target_str.trim();
    if s.starts_with('+') || s.starts_with('-') {
        let imm = parse_imm(s)?;
        let imm_i64 = i64::try_from(imm).ok()?;
        Some(ip.wrapping_add_signed(imm_i64))
    } else {
        let imm = parse_imm(s)?;
        u64::try_from(imm).ok()
    }
}

fn parse_mem_operand(text: &str, is_64: bool, ip: u64, instr_len: usize) -> Option<iced_x86::MemoryOperand> {
    let s = text.trim();
    let inner = if let (Some(start), Some(end)) = (s.find('['), s.rfind(']')) {
        if end <= start {
            return None;
        }
        s[start + 1..end].trim()
    } else {
        s
    };
    if inner.is_empty() {
        return None;
    }

    let (base_part, displ_part, is_sub) = if let Some((b, d)) = inner.split_once('+') {
        (b.trim(), d.trim(), false)
    } else if let Some((b, d)) = inner.split_once('-') {
        (b.trim(), d.trim(), true)
    } else {
        (inner, "", false)
    };

    if let Some(base_reg) = parse_reg(base_part) {
        if displ_part.is_empty() {
            return Some(iced_x86::MemoryOperand::with_base(base_reg));
        } else if let Some(d) = parse_imm(displ_part) {
            let d_i64 = i64::try_from(d).ok()?;
            let displ = if is_sub { d_i64.checked_neg()? } else { d_i64 };
            return Some(iced_x86::MemoryOperand::with_base_displ(base_reg, displ));
        }
    }

    if base_part.eq_ignore_ascii_case("rip") {
        let displ = if displ_part.is_empty() {
            0
        } else if let Some(d) = parse_imm(displ_part) {
            let d_i64 = i64::try_from(d).ok()?;
            if d > i32::MAX as i128 {
                // If the operand exceeds 32-bit displacement range, it is an absolute target VA
                let target = if is_sub { d_i64.checked_neg()? } else { d_i64 };
                return Some(iced_x86::MemoryOperand::with_base_displ(Register::RIP, target));
            }
            if is_sub { d_i64.checked_neg()? } else { d_i64 }
        } else {
            return None;
        };
        let target = (ip as i64 + instr_len as i64).wrapping_add(displ);
        return Some(iced_x86::MemoryOperand::with_base_displ(Register::RIP, target));
    }

    if let Some(addr) = parse_imm(inner) {
        let addr_i64 = i64::try_from(addr).ok()?;
        if is_64 {
            return Some(iced_x86::MemoryOperand::with_base_displ(Register::RIP, addr_i64));
        } else {
            return Some(iced_x86::MemoryOperand::with_base_displ(Register::None, addr_i64));
        }
    }

    None
}

fn encode_mem_op1(
    code: Code,
    operand_text: &str,
    is_64: bool,
    bitness: u32,
    ip: u64,
) -> Option<Vec<u8>> {
    let mut trial_len = 6;
    let mem = parse_mem_operand(operand_text, is_64, ip, trial_len)?;
    let instr = Instruction::with1(code, mem).ok()?;
    let bytes = encode_instruction_at(&instr, bitness, ip)?;
    if bytes.len() != trial_len {
        trial_len = bytes.len();
        let mem = parse_mem_operand(operand_text, is_64, ip, trial_len)?;
        let instr = Instruction::with1(code, mem).ok()?;
        return encode_instruction_at(&instr, bitness, ip);
    }
    Some(bytes)
}

fn encode_mem_op2_reg_mem(
    code: Code,
    dst_reg: Register,
    operand_text: &str,
    is_64: bool,
    bitness: u32,
    ip: u64,
) -> Option<Vec<u8>> {
    let mut trial_len = 7;
    let mem = parse_mem_operand(operand_text, is_64, ip, trial_len)?;
    let instr = Instruction::with2(code, dst_reg, mem).ok()?;
    let bytes = encode_instruction_at(&instr, bitness, ip)?;
    if bytes.len() != trial_len {
        trial_len = bytes.len();
        let mem = parse_mem_operand(operand_text, is_64, ip, trial_len)?;
        let instr = Instruction::with2(code, dst_reg, mem).ok()?;
        return encode_instruction_at(&instr, bitness, ip);
    }
    Some(bytes)
}

fn get_jcc_codes(op: &str, is_64: bool, bitness: u32) -> Option<(Code, Code)> {
    if is_64 {
        match op {
            "je" | "jz" => Some((Code::Je_rel8_64, Code::Je_rel32_64)),
            "jne" | "jnz" => Some((Code::Jne_rel8_64, Code::Jne_rel32_64)),
            "js" => Some((Code::Js_rel8_64, Code::Js_rel32_64)),
            "jns" => Some((Code::Jns_rel8_64, Code::Jns_rel32_64)),
            "jg" | "jnle" => Some((Code::Jg_rel8_64, Code::Jg_rel32_64)),
            "jge" | "jnl" => Some((Code::Jge_rel8_64, Code::Jge_rel32_64)),
            "jl" | "jnge" => Some((Code::Jl_rel8_64, Code::Jl_rel32_64)),
            "jle" | "jng" => Some((Code::Jle_rel8_64, Code::Jle_rel32_64)),
            "ja" | "jnbe" => Some((Code::Ja_rel8_64, Code::Ja_rel32_64)),
            "jae" | "jnb" | "jnc" => Some((Code::Jae_rel8_64, Code::Jae_rel32_64)),
            "jb" | "jnae" | "jc" => Some((Code::Jb_rel8_64, Code::Jb_rel32_64)),
            "jbe" | "jna" => Some((Code::Jbe_rel8_64, Code::Jbe_rel32_64)),
            "jo" => Some((Code::Jo_rel8_64, Code::Jo_rel32_64)),
            "jno" => Some((Code::Jno_rel8_64, Code::Jno_rel32_64)),
            "jp" | "jpe" => Some((Code::Jp_rel8_64, Code::Jp_rel32_64)),
            "jnp" | "jpo" => Some((Code::Jnp_rel8_64, Code::Jnp_rel32_64)),
            _ => None,
        }
    } else if bitness == 32 {
        match op {
            "je" | "jz" => Some((Code::Je_rel8_32, Code::Je_rel32_32)),
            "jne" | "jnz" => Some((Code::Jne_rel8_32, Code::Jne_rel32_32)),
            "js" => Some((Code::Js_rel8_32, Code::Js_rel32_32)),
            "jns" => Some((Code::Jns_rel8_32, Code::Jns_rel32_32)),
            "jg" | "jnle" => Some((Code::Jg_rel8_32, Code::Jg_rel32_32)),
            "jge" | "jnl" => Some((Code::Jge_rel8_32, Code::Jge_rel32_32)),
            "jl" | "jnge" => Some((Code::Jl_rel8_32, Code::Jl_rel32_32)),
            "jle" | "jng" => Some((Code::Jle_rel8_32, Code::Jle_rel32_32)),
            "ja" | "jnbe" => Some((Code::Ja_rel8_32, Code::Ja_rel32_32)),
            "jae" | "jnb" | "jnc" => Some((Code::Jae_rel8_32, Code::Jae_rel32_32)),
            "jb" | "jnae" | "jc" => Some((Code::Jb_rel8_32, Code::Jb_rel32_32)),
            "jbe" | "jna" => Some((Code::Jbe_rel8_32, Code::Jbe_rel32_32)),
            "jo" => Some((Code::Jo_rel8_32, Code::Jo_rel32_32)),
            "jno" => Some((Code::Jno_rel8_32, Code::Jno_rel32_32)),
            "jp" | "jpe" => Some((Code::Jp_rel8_32, Code::Jp_rel32_32)),
            "jnp" | "jpo" => Some((Code::Jnp_rel8_32, Code::Jnp_rel32_32)),
            _ => None,
        }
    } else {
        match op {
            "je" | "jz" => Some((Code::Je_rel8_16, Code::Je_rel16)),
            "jne" | "jnz" => Some((Code::Jne_rel8_16, Code::Jne_rel16)),
            "js" => Some((Code::Js_rel8_16, Code::Js_rel16)),
            "jns" => Some((Code::Jns_rel8_16, Code::Jns_rel16)),
            "jg" | "jnle" => Some((Code::Jg_rel8_16, Code::Jg_rel16)),
            "jge" | "jnl" => Some((Code::Jge_rel8_16, Code::Jge_rel16)),
            "jl" | "jnge" => Some((Code::Jl_rel8_16, Code::Jl_rel16)),
            "jle" | "jng" => Some((Code::Jle_rel8_16, Code::Jle_rel16)),
            "ja" | "jnbe" => Some((Code::Ja_rel8_16, Code::Ja_rel16)),
            "jae" | "jnb" | "jnc" => Some((Code::Jae_rel8_16, Code::Jae_rel16)),
            "jb" | "jnae" | "jc" => Some((Code::Jb_rel8_16, Code::Jb_rel16)),
            "jbe" | "jna" => Some((Code::Jbe_rel8_16, Code::Jbe_rel16)),
            "jo" => Some((Code::Jo_rel8_16, Code::Jo_rel16)),
            "jno" => Some((Code::Jno_rel8_16, Code::Jno_rel16)),
            "jp" | "jpe" => Some((Code::Jp_rel8_16, Code::Jp_rel16)),
            "jnp" | "jpo" => Some((Code::Jnp_rel8_16, Code::Jnp_rel16)),
            _ => None,
        }
    }
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

    const X86_NOP: u8 = 0x90;
    let padding = span - new_len;
    for i in new_len..span {
        let target = offset + i;
        crate::hex::edit::record_edit(app, target, X86_NOP);
    }

    let va = app.get_va(offset);
    let lang = app.config.lang;
    let new_len_str = new_len.to_string();
    let va_str = format!("{:X}", va);
    let padding_str = padding.to_string();
    Ok(if padding > 0 {
        crate::i18n::fill(
            crate::i18n::M::DoneAssembledPadded.tr(lang),
            &[&new_len_str, &va_str, &padding_str],
        )
    } else {
        crate::i18n::fill(
            crate::i18n::M::DoneAssembled.tr(lang),
            &[&new_len_str, &va_str],
        )
    })
}

pub fn parse_assemble_input(input: &str, bitness: u32, ip: u64) -> Option<Vec<u8>> {
    let clean = input.trim();
    if clean.is_empty() {
        return None;
    }

    let single_clean = clean.trim_start_matches("0x").trim_start_matches("0X");
    if single_clean.len().is_multiple_of(2) && single_clean.chars().all(|c| c.is_ascii_hexdigit()) {
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
        if tok_clean.len() <= 2
            && tok_clean.chars().all(|c| c.is_ascii_hexdigit())
            && let Ok(b) = u8::from_str_radix(tok_clean, 16)
        {
            hex_bytes.push(b);
            continue;
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
            if tokens.len() >= 2
                && let Some(reg) = parse_reg(tokens[1])
            {
                let code = match get_reg_size(reg) {
                    RegSize::R64 => Code::Pop_r64,
                    _ => Code::Pop_r32,
                };
                if let Ok(instr) = Instruction::with1(code, reg)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
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
                    if let Ok(instr) = Instruction::with1(code, reg)
                        && let Some(bytes) = encode_instruction(&instr, bitness)
                    {
                        return Some(bytes);
                    }
                } else if let Some(val) = parse_imm(target).and_then(|v| fit_imm(v, 32)) {
                    let val = val as i32;
                    if let Ok(instr) = Instruction::with1(if is_64 { Code::Pushq_imm32 } else { Code::Pushd_imm32 }, val)
                        && let Some(bytes) = encode_instruction(&instr, bitness)
                    {
                        return Some(bytes);
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
                    if let Ok(instr) = Instruction::with2(code, dst, src)
                        && let Some(bytes) = encode_instruction(&instr, bitness)
                    {
                        return Some(bytes);
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
                        if let Ok(instr) = res
                            && let Some(bytes) = encode_instruction(&instr, bitness)
                        {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        "xor" => {
            let rest = lower.trim_start_matches("xor").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2
                && let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1]))
            {
                let code = match get_reg_size(dst) {
                    RegSize::R64 => Code::Xor_rm64_r64,
                    RegSize::R32 => Code::Xor_rm32_r32,
                    RegSize::R16 => Code::Xor_rm16_r16,
                    RegSize::R8  => Code::Xor_rm8_r8,
                };
                if let Ok(instr) = Instruction::with2(code, dst, src)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "add" => {
            let rest = lower.trim_start_matches("add").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2
                && let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1]))
            {
                let code = match get_reg_size(dst) {
                    RegSize::R64 => Code::Add_rm64_r64,
                    RegSize::R32 => Code::Add_rm32_r32,
                    RegSize::R16 => Code::Add_rm16_r16,
                    RegSize::R8  => Code::Add_rm8_r8,
                };
                if let Ok(instr) = Instruction::with2(code, dst, src)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "sub" => {
            let rest = lower.trim_start_matches("sub").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2
                && let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1]))
            {
                let code = match get_reg_size(dst) {
                    RegSize::R64 => Code::Sub_rm64_r64,
                    RegSize::R32 => Code::Sub_rm32_r32,
                    RegSize::R16 => Code::Sub_rm16_r16,
                    RegSize::R8  => Code::Sub_rm8_r8,
                };
                if let Ok(instr) = Instruction::with2(code, dst, src)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "cmp" => {
            let rest = lower.trim_start_matches("cmp").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2
                && let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1]))
            {
                let code = match get_reg_size(dst) {
                    RegSize::R64 => Code::Cmp_rm64_r64,
                    RegSize::R32 => Code::Cmp_rm32_r32,
                    RegSize::R16 => Code::Cmp_rm16_r16,
                    RegSize::R8  => Code::Cmp_rm8_r8,
                };
                if let Ok(instr) = Instruction::with2(code, dst, src)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "test" => {
            let rest = lower.trim_start_matches("test").trim();
            let parts: Vec<&str> = rest.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2
                && let (Some(dst), Some(src)) = (parse_reg(parts[0]), parse_reg(parts[1]))
            {
                let code = match get_reg_size(dst) {
                    RegSize::R64 => Code::Test_rm64_r64,
                    RegSize::R32 => Code::Test_rm32_r32,
                    RegSize::R16 => Code::Test_rm16_r16,
                    RegSize::R8  => Code::Test_rm8_r8,
                };
                if let Ok(instr) = Instruction::with2(code, dst, src)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "inc" => {
            if tokens.len() >= 2
                && let Some(reg) = parse_reg(tokens[1])
            {
                let code = match get_reg_size(reg) {
                    RegSize::R64 => Code::Inc_rm64,
                    RegSize::R32 => Code::Inc_rm32,
                    RegSize::R16 => Code::Inc_rm16,
                    RegSize::R8  => Code::Inc_rm8,
                };
                if let Ok(instr) = Instruction::with1(code, reg)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "dec" => {
            if tokens.len() >= 2
                && let Some(reg) = parse_reg(tokens[1])
            {
                let code = match get_reg_size(reg) {
                    RegSize::R64 => Code::Dec_rm64,
                    RegSize::R32 => Code::Dec_rm32,
                    RegSize::R16 => Code::Dec_rm16,
                    RegSize::R8  => Code::Dec_rm8,
                };
                if let Ok(instr) = Instruction::with1(code, reg)
                    && let Some(bytes) = encode_instruction(&instr, bitness)
                {
                    return Some(bytes);
                }
            }
        }

        "lea" => {
            let rest = clean.get(3..).unwrap_or("").trim();
            if let Some((dst_str, src_str)) = rest.split_once(',')
                && let Some(dst_reg) = parse_reg(dst_str)
            {
                let code = match get_reg_size(dst_reg) {
                    RegSize::R64 => Code::Lea_r64_m,
                    _ => Code::Lea_r32_m,
                };
                if let Some(bytes) = encode_mem_op2_reg_mem(code, dst_reg, src_str, is_64, bitness, ip) {
                    return Some(bytes);
                }
            }
        }

        "call" => {
            let lower_rest = lower[op.len()..].trim();
            let operand_clean = lower_rest
                .strip_prefix("near ptr").map(str::trim)
                .or_else(|| lower_rest.strip_prefix("near").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("ptr").map(str::trim))
                .unwrap_or(lower_rest);

            // 1. Direct register: call rax, call eax, call r10
            if let Some(reg) = parse_reg(operand_clean) {
                let code = match get_reg_size(reg) {
                    RegSize::R64 => Code::Call_rm64,
                    _ => Code::Call_rm32,
                };
                if let Ok(instr) = Instruction::with1(code, reg)
                    && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                {
                    return Some(bytes);
                }
            }

            // 2. Memory operand: call [rax], call [rip+0x1000], call qword ptr [rip+0x1000]
            if lower_rest.contains('[') && lower_rest.contains(']') {
                let code = if is_64 { Code::Call_rm64 } else { Code::Call_rm32 };
                if let Some(bytes) = encode_mem_op1(code, lower_rest, is_64, bitness, ip) {
                    return Some(bytes);
                }
            }

            // 3. Target address: call 0x14004C6F0, call 14004c6f0, call +0x20, call near 0x...
            if let Some(target) = parse_branch_target(operand_clean, ip) {
                let code = if is_64 {
                    Code::Call_rel32_64
                } else if bitness == 32 {
                    Code::Call_rel32_32
                } else {
                    Code::Call_rel16
                };
                if let Ok(instr) = Instruction::with_branch(code, target)
                    && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                {
                    return Some(bytes);
                }
            }
        }

        "jmp" => {
            let lower_rest = lower[op.len()..].trim();
            let is_short = lower_rest.starts_with("short");
            let is_near = lower_rest.starts_with("near");
            let operand_clean = lower_rest
                .strip_prefix("short ptr").map(str::trim)
                .or_else(|| lower_rest.strip_prefix("near ptr").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("short").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("near").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("ptr").map(str::trim))
                .unwrap_or(lower_rest);

            // 1. Direct register: jmp rax, jmp rbx
            if let Some(reg) = parse_reg(operand_clean) {
                let code = match get_reg_size(reg) {
                    RegSize::R64 => Code::Jmp_rm64,
                    _ => Code::Jmp_rm32,
                };
                if let Ok(instr) = Instruction::with1(code, reg)
                    && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                {
                    return Some(bytes);
                }
            }

            // 2. Memory operand: jmp [rax], jmp [rip+0x1000], jmp qword ptr [rip+0x1000]
            if operand_clean.contains('[') && operand_clean.contains(']') {
                let code = if is_64 { Code::Jmp_rm64 } else { Code::Jmp_rm32 };
                if let Some(bytes) = encode_mem_op1(code, operand_clean, is_64, bitness, ip) {
                    return Some(bytes);
                }
            }

            // 3. Target address: jmp 0x14004c6f1, jmp 14004c6f1, jmp short ..., jmp near ...
            if let Some(target) = parse_branch_target(operand_clean, ip) {
                let (short_code, near_code) = if is_64 {
                    (Code::Jmp_rel8_64, Code::Jmp_rel32_64)
                } else if bitness == 32 {
                    (Code::Jmp_rel8_32, Code::Jmp_rel32_32)
                } else {
                    (Code::Jmp_rel8_16, Code::Jmp_rel16)
                };

                if is_short {
                    if let Ok(instr) = Instruction::with_branch(short_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                } else if is_near {
                    if let Ok(instr) = Instruction::with_branch(near_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                } else {
                    // Default: try short jump first (2 bytes). If out of range, use near jump (5 bytes).
                    if let Ok(instr) = Instruction::with_branch(short_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                    if let Ok(instr) = Instruction::with_branch(near_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                }
            }
        }

        _ if let Some((short_code, near_code)) = get_jcc_codes(op, is_64, bitness) => {
            let lower_rest = lower[op.len()..].trim();
            let is_short = lower_rest.starts_with("short");
            let is_near = lower_rest.starts_with("near");
            let operand_clean = lower_rest
                .strip_prefix("short ptr").map(str::trim)
                .or_else(|| lower_rest.strip_prefix("near ptr").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("short").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("near").map(str::trim))
                .or_else(|| lower_rest.strip_prefix("ptr").map(str::trim))
                .unwrap_or(lower_rest);

            if let Some(target) = parse_branch_target(operand_clean, ip) {
                if is_short {
                    if let Ok(instr) = Instruction::with_branch(short_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                } else if is_near {
                    if let Ok(instr) = Instruction::with_branch(near_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                } else {
                    // Default: try short jump first (2 bytes). If out of range, use near jump (6 bytes).
                    if let Ok(instr) = Instruction::with_branch(short_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
                    }
                    if let Ok(instr) = Instruction::with_branch(near_code, target)
                        && let Some(bytes) = encode_instruction_at(&instr, bitness, ip)
                    {
                        return Some(bytes);
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

            if !text_to_copy.is_empty()
                && let Ok(cb) = &mut app.clipboard
            {
                let _ = cb.set_text(text_to_copy);
                App::log(app, "Copied assembly text to clipboard".to_string());
            }
            return Ok(false);
        }

        if is_ctrl && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'))
            && let Ok(cb) = &mut app.clipboard
            && let Ok(pasted) = cb.get_text()
        {
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
            return Ok(false);
        }

        if is_shift {
            let cursor = app.assemble_input.cursor();
            let val_char_len = app.assemble_input.value().chars().count();
            app.assemble_selection_anchor.get_or_insert(cursor);
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

    #[test]
    fn test_parse_assemble_branch_instructions() {
        let ip = 0x14004C6E0;
        let bitness = 64;

        // User case 1: call 0x000000014004C6F0
        let bytes = parse_assemble_input("call 0x000000014004C6F0", bitness, ip).expect("call hex prefix");
        assert_eq!(bytes, vec![0xE8, 0x0B, 0x00, 0x00, 0x00]);

        // User case 2: jmp 14004c6f1
        let bytes_jmp = parse_assemble_input("jmp 14004c6f1", bitness, ip).expect("jmp hex without prefix");
        assert_eq!(bytes_jmp, vec![0xEB, 0x0F]);

        // User case 2b: jmp near 14004c6f1
        let bytes_near = parse_assemble_input("jmp near 14004c6f1", bitness, ip).expect("jmp near");
        assert_eq!(bytes_near, vec![0xE9, 0x0C, 0x00, 0x00, 0x00]);

        // Register calls/jmps
        let bytes_rcall = parse_assemble_input("call rax", bitness, ip).expect("call rax");
        assert_eq!(bytes_rcall, vec![0xFF, 0xD0]);

        let bytes_rjmp = parse_assemble_input("jmp rbx", bitness, ip).expect("jmp rbx");
        assert_eq!(bytes_rjmp, vec![0xFF, 0xE3]);

        // Memory call
        let bytes_mcall = parse_assemble_input("call [rax]", bitness, ip).expect("call [rax]");
        assert_eq!(bytes_mcall, vec![0xFF, 0x10]);

        let bytes_rip_call = parse_assemble_input("call [0x140001000]", 64, 0x140000000).expect("call [addr]");
        let disp = i32::from_le_bytes(*bytes_rip_call.last_chunk::<4>().expect("disp32"));
        let next_ip = 0x140000000u64 + bytes_rip_call.len() as u64;
        assert_eq!(
            next_ip.wrapping_add(disp as i64 as u64),
            0x140001000,
            "call [addr] must target the exact requested address"
        );

        // Conditional jumps
        let bytes_je = parse_assemble_input("je 14004c6f1", bitness, ip).expect("je");
        assert_eq!(bytes_je, vec![0x74, 0x0F]);

        let bytes_jne = parse_assemble_input("jne 14004c6f1", bitness, ip).expect("jne");
        assert_eq!(bytes_jne, vec![0x75, 0x0F]);

        let bytes_jz = parse_assemble_input("jz 14004c6f1", bitness, ip).expect("jz");
        assert_eq!(bytes_jz, vec![0x74, 0x0F]);

        // Prefix variations
        let bytes_near_call = parse_assemble_input("call near 0x14004C6F0", bitness, ip).expect("call near");
        assert_eq!(bytes_near_call, vec![0xE8, 0x0B, 0x00, 0x00, 0x00]);

        let bytes_short_ptr_jmp = parse_assemble_input("jmp short ptr 14004c6f1", bitness, ip).expect("jmp short ptr");
        assert_eq!(bytes_short_ptr_jmp, vec![0xEB, 0x0F]);

        let bytes_near_je = parse_assemble_input("je near 14004c6f1", bitness, ip).expect("je near");
        assert_eq!(bytes_near_je, vec![0x0F, 0x84, 0x0B, 0x00, 0x00, 0x00]);

        // 32-bit mode
        let bytes_32 = parse_assemble_input("call 0x401050", 32, 0x401000).expect("call 32-bit");
        assert_eq!(bytes_32, vec![0xE8, 0x4B, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_extended_registers_and_rip_addressing() {
        let ip = 0x140001000u64;
        let bitness = 64;

        // 8-bit extended and byte registers
        let bytes_r8b = parse_assemble_input("mov r8b, 0x12", bitness, ip).expect("mov r8b");
        assert_eq!(bytes_r8b, vec![0x41, 0xB0, 0x12]);

        let bytes_sil = parse_assemble_input("mov sil, dil", bitness, ip).expect("mov sil, dil");
        assert_eq!(bytes_sil, vec![0x40, 0x88, 0xFE]);

        let bytes_bpl = parse_assemble_input("mov bpl, spl", bitness, ip).expect("mov bpl, spl");
        assert_eq!(bytes_bpl, vec![0x40, 0x88, 0xE5]);

        let bytes_r15w = parse_assemble_input("mov r15w, ax", bitness, ip).expect("mov r15w, ax");
        assert_eq!(bytes_r15w, vec![0x66, 0x41, 0x89, 0xC7]);

        // RIP relative memory operand in call and jmp
        let bytes_rip_call = parse_assemble_input("call [rip + 0x20]", bitness, ip).expect("call [rip+0x20]");
        assert_eq!(bytes_rip_call.len(), 6);
        assert_eq!(bytes_rip_call, vec![0xFF, 0x15, 0x20, 0x00, 0x00, 0x00]);

        let bytes_rip_jmp = parse_assemble_input("jmp [rip + 0x30]", bitness, ip).expect("jmp [rip+0x30]");
        assert_eq!(bytes_rip_jmp.len(), 6);
        assert_eq!(bytes_rip_jmp, vec![0xFF, 0x25, 0x30, 0x00, 0x00, 0x00]);

        // LEA with RIP relative displacement (7 bytes!)
        let bytes_lea = parse_assemble_input("lea rax, [rip + 0x20]", bitness, ip).expect("lea rax, [rip+0x20]");
        assert_eq!(bytes_lea.len(), 7);
        assert_eq!(bytes_lea, vec![0x48, 0x8D, 0x05, 0x20, 0x00, 0x00, 0x00]);
    }
}

