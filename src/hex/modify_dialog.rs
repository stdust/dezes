use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use tui_input::Input;
use crossterm::event::{Event, KeyCode};
use std::io::Result;

use crate::{app::App, editor::UIState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModifyOp {
    Add,
    Sub,
    Mul,
    Div,
    Xor,
    Or,
    And,
    Not,
    EndianSwap,
    ShiftLeft,
    ShiftRight,
    Random,
    RollingXor,
}

impl ModifyOp {
    /// Operation name in the interface language.
    pub fn label_for(&self, lang: crate::i18n::Lang) -> &'static str {
        use crate::i18n::M;
        let message = match self {
            ModifyOp::Add => M::OpAdd,
            ModifyOp::Sub => M::OpSub,
            ModifyOp::Mul => M::OpMul,
            ModifyOp::Div => M::OpDiv,
            ModifyOp::Xor => M::OpXor,
            ModifyOp::Or => M::OpOr,
            ModifyOp::And => M::OpAnd,
            ModifyOp::Not => M::OpNot,
            ModifyOp::EndianSwap => M::OpEndianSwap,
            ModifyOp::ShiftLeft => M::OpShiftLeft,
            ModifyOp::ShiftRight => M::OpShiftRight,
            ModifyOp::Random => M::OpRandom,
            ModifyOp::RollingXor => M::OpRollingXor,
        };
        message.tr(lang)
    }

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            ModifyOp::Add => "Add (+)",
            ModifyOp::Sub => "Subtract (-)",
            ModifyOp::Mul => "Multiply (*)",
            ModifyOp::Div => "Divide (/)",
            ModifyOp::Xor => "XOR (^)",
            ModifyOp::Or => "OR (|)",
            ModifyOp::And => "AND (&)",
            ModifyOp::Not => "Invert/NOT (~)",
            ModifyOp::EndianSwap => "Endian Swap",
            ModifyOp::ShiftLeft => "Shift Left (<<)",
            ModifyOp::ShiftRight => "Shift Right (>>)",
            ModifyOp::Random => "Random Fill (rand)",
            ModifyOp::RollingXor => "Rolling XOR (key+step)",
        }
    }

    pub fn all() -> &'static [ModifyOp] {
        &[
            ModifyOp::Add,
            ModifyOp::Sub,
            ModifyOp::Mul,
            ModifyOp::Div,
            ModifyOp::Xor,
            ModifyOp::Or,
            ModifyOp::And,
            ModifyOp::Not,
            ModifyOp::EndianSwap,
            ModifyOp::ShiftLeft,
            ModifyOp::ShiftRight,
            ModifyOp::Random,
            ModifyOp::RollingXor,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModifyFocus {
    Operation,
    Operand,
    Step,
    HexMode,
    BitSize,
}

#[derive(Debug)]
pub struct ModifyDialog {
    pub op_index: usize,
    pub operand_input: Input,
    pub step_input: Input,
    /// Character a Shift-selection started from, or `None`. One per dialog: only
    /// the focused field can hold a block.
    pub anchor: Option<usize>,
    pub is_hex: bool,
    pub bit_size: usize, // 8, 16, 32, 64
    pub focus: ModifyFocus,
}

impl Default for ModifyDialog {
    fn default() -> Self {
        Self {
            op_index: 4, // Default to XOR
            operand_input: Input::default(),
            step_input: Input::new("1".to_string()),
            anchor: None,
            is_hex: true,
            bit_size: 8,
            focus: ModifyFocus::Operation,
        }
    }
}

impl ModifyDialog {
    pub fn reset(&mut self) {
        self.op_index = 4; // XOR
        self.operand_input = Input::default();
        self.step_input = Input::new("1".to_string());
        self.anchor = None;
        self.is_hex = true;
        self.bit_size = 8;
        self.focus = ModifyFocus::Operation;
    }

    pub fn current_op(&self) -> ModifyOp {
        ModifyOp::all()[self.op_index % ModifyOp::all().len()]
    }

    pub fn move_op_grid(&mut self, dx: isize, dy: isize) {
        let cols = 3;
        let total = ModifyOp::all().len();
        let current = self.op_index;
        let row = (current / cols) as isize;
        let col = (current % cols) as isize;

        let new_col = (col + dx).rem_euclid(cols as isize);
        let mut new_row = row + dy;
        let max_rows = ((total + cols - 1) / cols) as isize;
        new_row = new_row.rem_euclid(max_rows);

        let mut next_idx = (new_row * cols as isize + new_col) as usize;
        if next_idx >= total {
            next_idx = total - 1;
        }
        self.op_index = next_idx;
    }

    pub fn next_bit_size(&mut self) {
        self.bit_size = match self.bit_size {
            8 => 16,
            16 => 32,
            32 => 64,
            _ => 8,
        };
    }
}

pub fn draw_modify_dialog(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let dialog_width = 72;
    let dialog_height = 13; // Option 2 Slim & Compact layout (5 rows grid)

    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width.min(area.width), dialog_height.min(area.height));

    frame.render_widget(Clear, dialog_area);

    let (range_start, range_end) = if app.hex_view.selection.end > app.hex_view.selection.start {
        (app.hex_view.selection.start, app.hex_view.selection.end)
    } else {
        (app.hex_view.offset, (app.hex_view.offset + 1).min(app.file_info.size))
    };
    let block_size = range_end.saturating_sub(range_start);

    let dialog_style = app.config.theme.dialog;
    let highlight_style = app.config.theme.highlight;
    let bold_style = dialog_style.add_modifier(Modifier::BOLD);

    let block = Block::default()
        .title(format!(
            " {} (0x{:08X}..0x{:08X} : {} bytes) ",
            crate::i18n::M::ModifyBlockTitle.tr(app.config.lang),
            range_start,
            range_end,
            block_size
        ))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style);

    frame.render_widget(block, dialog_area);

    let inner_area = dialog_area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 2,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Operation Grid (5 rows)
            Constraint::Length(2), // Value + Step + Hex Mode
            Constraint::Length(2), // Data Unit
        ])
        .split(inner_area);

    let dialog = &app.hex_view.modify_dialog;

    // 1. Operation Grid
    let op_box = Block::default()
        .title(crate::i18n::M::OperationTitle.tr(app.config.lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(if dialog.focus == ModifyFocus::Operation {
            bold_style
        } else {
            dialog_style
        });

    let grid_inner = op_box.inner(chunks[0]);
    frame.render_widget(op_box, chunks[0]);

    let grid_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(grid_inner);

    let ops = ModifyOp::all();
    for (idx, op) in ops.iter().enumerate() {
        let row_idx = idx / 3;
        let col_idx = idx % 3;
        if row_idx < grid_rows.len() {
            let col_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ])
                .split(grid_rows[row_idx]);

            let is_selected = idx == dialog.op_index;
            let radio = if is_selected { "(o)" } else { "( )" };
            let style = if is_selected && dialog.focus == ModifyFocus::Operation {
                highlight_style
            } else if is_selected {
                bold_style
            } else {
                dialog_style
            };

            let text = format!("{} {}", radio, op.label_for(app.config.lang));
            frame.render_widget(Paragraph::new(text).style(style), col_chunks[col_idx]);
        }
    }

    // 2. Operand Value, Step & Hex Mode
    let val_style = if dialog.focus == ModifyFocus::Operand {
        highlight_style
    } else {
        dialog_style
    };
    let step_style = if dialog.focus == ModifyFocus::Step {
        highlight_style
    } else {
        dialog_style
    };
    let hex_style = if dialog.focus == ModifyFocus::HexMode {
        highlight_style
    } else {
        dialog_style
    };

    let hex_check = if dialog.is_hex { "[x] Hexadecimal" } else { "[ ] Decimal" };

    let val_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Length(22), Constraint::Length(20)])
        .split(chunks[1]);

    let val_display = dialog.operand_input.value();
    let step_display = dialog.step_input.value();

    if dialog.focus == ModifyFocus::Operand {
        let cursor_pos = dialog.operand_input.cursor();
        // The focused row is drawn in the highlight style already, so a Shift-block
        // is marked by reversing that instead of by another colour.
        let sel = crate::text_field::selection(&dialog.operand_input, dialog.anchor);
        let chars: Vec<char> = val_display.chars().collect();
        let mut spans = vec![Span::styled(format!(" {}: [", crate::i18n::M::LblValue.tr(app.config.lang)), val_style)];

        for i in 0..chars.len() {
            let ch_str = chars[i].to_string();
            let in_block = sel.is_some_and(|(s, e)| i >= s && i < e);
            if i == cursor_pos {
                spans.push(Span::styled(ch_str, highlight_style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)));
            } else if in_block {
                spans.push(Span::styled(ch_str, val_style.add_modifier(Modifier::REVERSED)));
            } else {
                spans.push(Span::styled(ch_str, val_style));
            }
        }

        if cursor_pos == chars.len() {
            spans.push(Span::styled("_", highlight_style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)));
        }

        spans.push(Span::styled("]", val_style));
        frame.render_widget(Paragraph::new(Line::from(spans)), val_layout[0]);
    } else {
        frame.render_widget(Paragraph::new(format!(" {}: [{}]", crate::i18n::M::LblValue.tr(app.config.lang), val_display)).style(val_style), val_layout[0]);
    }

    if dialog.focus == ModifyFocus::Step {
        let cursor_pos = dialog.step_input.cursor();
        let sel = crate::text_field::selection(&dialog.step_input, dialog.anchor);
        let chars: Vec<char> = step_display.chars().collect();
        let mut spans = vec![Span::styled(format!(" {}: [", crate::i18n::M::LblStep.tr(app.config.lang)), step_style)];

        for i in 0..chars.len() {
            let ch_str = chars[i].to_string();
            let in_block = sel.is_some_and(|(s, e)| i >= s && i < e);
            if i == cursor_pos {
                spans.push(Span::styled(ch_str, highlight_style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)));
            } else if in_block {
                spans.push(Span::styled(ch_str, step_style.add_modifier(Modifier::REVERSED)));
            } else {
                spans.push(Span::styled(ch_str, step_style));
            }
        }

        if cursor_pos == chars.len() {
            spans.push(Span::styled("_", highlight_style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)));
        }

        spans.push(Span::styled("]", step_style));
        frame.render_widget(Paragraph::new(Line::from(spans)), val_layout[1]);
    } else {
        frame.render_widget(Paragraph::new(format!(" {}: [{}]", crate::i18n::M::LblStep.tr(app.config.lang), step_display)).style(step_style), val_layout[1]);
    }

    frame.render_widget(Paragraph::new(hex_check).style(hex_style), val_layout[2]);

    // 3. Unit Size
    let unit_style = if dialog.focus == ModifyFocus::BitSize {
        highlight_style
    } else {
        dialog_style
    };
    let unit_text = format!(" Data Unit : < {}-bit ({}) >", dialog.bit_size, match dialog.bit_size {
        8 => "1 byte",
        16 => "2 bytes",
        32 => "4 bytes",
        _ => "8 bytes",
    });
    frame.render_widget(Paragraph::new(unit_text).style(unit_style), chunks[2]);
}

pub fn dialog_modify_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.state = UIState::Normal;
                app.dialog_renderer = None;
                return Ok(false);
            }
            KeyCode::Tab => {
                let dialog = &mut app.hex_view.modify_dialog;
                // The block belonged to the field being left.
                dialog.anchor = None;
                dialog.focus = match dialog.focus {
                    ModifyFocus::Operation => ModifyFocus::Operand,
                    ModifyFocus::Operand => ModifyFocus::Step,
                    ModifyFocus::Step => ModifyFocus::HexMode,
                    ModifyFocus::HexMode => ModifyFocus::BitSize,
                    ModifyFocus::BitSize => ModifyFocus::Operation,
                };
                return Ok(false);
            }
            KeyCode::BackTab => {
                let dialog = &mut app.hex_view.modify_dialog;
                dialog.anchor = None;
                dialog.focus = match dialog.focus {
                    ModifyFocus::Operation => ModifyFocus::BitSize,
                    ModifyFocus::Operand => ModifyFocus::Operation,
                    ModifyFocus::Step => ModifyFocus::Operand,
                    ModifyFocus::HexMode => ModifyFocus::Step,
                    ModifyFocus::BitSize => ModifyFocus::HexMode,
                };
                return Ok(false);
            }
            KeyCode::Enter => {
                apply_block_modification(app);
                app.state = UIState::Normal;
                app.dialog_renderer = None;
                return Ok(false);
            }
            _ => {}
        }

        let focus = app.hex_view.modify_dialog.focus;
        match focus {
            ModifyFocus::Operation => match key.code {
                KeyCode::Left => app.hex_view.modify_dialog.move_op_grid(-1, 0),
                KeyCode::Right => app.hex_view.modify_dialog.move_op_grid(1, 0),
                KeyCode::Up => app.hex_view.modify_dialog.move_op_grid(0, -1),
                KeyCode::Down => app.hex_view.modify_dialog.move_op_grid(0, 1),
                _ => {}
            },
            // Shift+arrows, Shift+Home/End and Ctrl+C/X/V over the block.
            ModifyFocus::Operand | ModifyFocus::Step => {
                crate::text_field::handle_key(app, modify_field, event);
            }
            ModifyFocus::HexMode => {
                if key.code == KeyCode::Char(' ') {
                    app.hex_view.modify_dialog.is_hex = !app.hex_view.modify_dialog.is_hex;
                }
            }
            ModifyFocus::BitSize => {
                if key.code == KeyCode::Char(' ') {
                    app.hex_view.modify_dialog.next_bit_size();
                }
            }
        }
    }
    Ok(false)
}

/// The focused value box of the Modify dialog, and its selection anchor.
fn modify_field(app: &mut App) -> (&mut Input, &mut Option<usize>) {
    let dialog = &mut app.hex_view.modify_dialog;
    let input = if dialog.focus == ModifyFocus::Step {
        &mut dialog.step_input
    } else {
        &mut dialog.operand_input
    };
    (input, &mut dialog.anchor)
}

pub fn apply_block_modification(app: &mut App) {
    let (start, end) = if app.hex_view.selection.end > app.hex_view.selection.start {
        (app.hex_view.selection.start, app.hex_view.selection.end)
    } else {
        (app.hex_view.offset, app.hex_view.offset)
    };

    if start > end || start >= app.file_info.size {
        return;
    }

    if app.file_info.is_read_only {
        // The opening shortcuts refuse read-only files already; this is the
        // backstop, and it says so rather than dropping the operation silently.
        app.read_only_error(crate::i18n::M::RoModifyBlock);
        return;
    }

    let (op, val_str, step_str, is_hex, bit_size) = {
        let dialog = &app.hex_view.modify_dialog;
        (
            dialog.current_op(),
            dialog.operand_input.value().to_string(),
            dialog.step_input.value().to_string(),
            dialog.is_hex,
            dialog.bit_size,
        )
    };

    let operand: u64 = if is_hex {
        u64::from_str_radix(&val_str, 16).unwrap_or(0)
    } else {
        val_str.parse::<u64>().unwrap_or(0)
    };

    let step: u64 = if is_hex {
        u64::from_str_radix(&step_str, 16).unwrap_or(1)
    } else {
        step_str.parse::<u64>().unwrap_or(1)
    };

    let mut bytes = Vec::new();
    for offset in start..=end {
        let b_opt = app.hex_view.changed_bytes.get(&offset).copied().or_else(|| app.read_u8(offset));

        if let Some(b) = b_opt {
            bytes.push((offset, b));
        }
    }

    if bytes.is_empty() {
        return;
    }

    match op {
        ModifyOp::EndianSwap => {
            let group_size = (bit_size / 8).max(1);
            for chunk in bytes.chunks_exact_mut(group_size) {
                let raw_vals: Vec<u8> = chunk.iter().map(|(_, b)| *b).rev().collect();
                for (i, val) in raw_vals.into_iter().enumerate() {
                    chunk[i].1 = val;
                }
            }
        }
        ModifyOp::Not => {
            for (_, b) in bytes.iter_mut() {
                *b = !*b;
            }
        }
        ModifyOp::Random => {
            let mut rng = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x123456789ABCDEF0);
            if rng == 0 {
                rng = 0x9268572b;
            }

            for (_, b) in bytes.iter_mut() {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                *b = rng as u8;
            }
        }
        ModifyOp::RollingXor => {
            let unit_bytes = (bit_size / 8).max(1);
            let mut current_key = operand;

            for chunk in bytes.chunks_exact_mut(unit_bytes) {
                match unit_bytes {
                    1 => {
                        let val = chunk[0].1 ^ (current_key as u8);
                        chunk[0].1 = val;
                        current_key = current_key.wrapping_add(step);
                    }
                    2 => {
                        let val = u16::from_le_bytes([chunk[0].1, chunk[1].1]) ^ (current_key as u16);
                        let res_bytes = val.to_le_bytes();
                        chunk[0].1 = res_bytes[0];
                        chunk[1].1 = res_bytes[1];
                        current_key = current_key.wrapping_add(step);
                    }
                    4 => {
                        let val = u32::from_le_bytes([chunk[0].1, chunk[1].1, chunk[2].1, chunk[3].1]) ^ (current_key as u32);
                        let res_bytes = val.to_le_bytes();
                        for i in 0..4 {
                            chunk[i].1 = res_bytes[i];
                        }
                        current_key = current_key.wrapping_add(step);
                    }
                    8 => {
                        if chunk.len() == 8 {
                            let mut arr = [0u8; 8];
                            for i in 0..8 {
                                arr[i] = chunk[i].1;
                            }
                            let val = u64::from_le_bytes(arr) ^ current_key;
                            let res_bytes = val.to_le_bytes();
                            for i in 0..8 {
                                chunk[i].1 = res_bytes[i];
                            }
                            current_key = current_key.wrapping_add(step);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {
            let unit_bytes = (bit_size / 8).max(1);
            for chunk in bytes.chunks_exact_mut(unit_bytes) {
                match unit_bytes {
                    1 => {
                        let mut val = chunk[0].1;
                        val = match op {
                            ModifyOp::Add => val.wrapping_add(operand as u8),
                            ModifyOp::Sub => val.wrapping_sub(operand as u8),
                            ModifyOp::Mul => val.wrapping_mul(operand as u8),
                            ModifyOp::Div => if operand != 0 { val / (operand as u8) } else { val },
                            ModifyOp::Xor => val ^ (operand as u8),
                            ModifyOp::Or => val | (operand as u8),
                            ModifyOp::And => val & (operand as u8),
                            ModifyOp::ShiftLeft => val << (operand as u8 & 7),
                            ModifyOp::ShiftRight => val >> (operand as u8 & 7),
                            _ => val,
                        };
                        chunk[0].1 = val;
                    }
                    2 => {
                        let mut val = u16::from_le_bytes([chunk[0].1, chunk[1].1]);
                        let op_val = operand as u16;
                        val = match op {
                            ModifyOp::Add => val.wrapping_add(op_val),
                            ModifyOp::Sub => val.wrapping_sub(op_val),
                            ModifyOp::Mul => val.wrapping_mul(op_val),
                            ModifyOp::Div => if op_val != 0 { val / op_val } else { val },
                            ModifyOp::Xor => val ^ op_val,
                            ModifyOp::Or => val | op_val,
                            ModifyOp::And => val & op_val,
                            ModifyOp::ShiftLeft => val << (op_val & 15),
                            ModifyOp::ShiftRight => val >> (op_val & 15),
                            _ => val,
                        };
                        let res_bytes = val.to_le_bytes();
                        chunk[0].1 = res_bytes[0];
                        chunk[1].1 = res_bytes[1];
                    }
                    4 => {
                        let mut val = u32::from_le_bytes([chunk[0].1, chunk[1].1, chunk[2].1, chunk[3].1]);
                        let op_val = operand as u32;
                        val = match op {
                            ModifyOp::Add => val.wrapping_add(op_val),
                            ModifyOp::Sub => val.wrapping_sub(op_val),
                            ModifyOp::Mul => val.wrapping_mul(op_val),
                            ModifyOp::Div => if op_val != 0 { val / op_val } else { val },
                            ModifyOp::Xor => val ^ op_val,
                            ModifyOp::Or => val | op_val,
                            ModifyOp::And => val & op_val,
                            ModifyOp::ShiftLeft => val << (op_val & 31),
                            ModifyOp::ShiftRight => val >> (op_val & 31),
                            _ => val,
                        };
                        let res_bytes = val.to_le_bytes();
                        for i in 0..4 {
                            chunk[i].1 = res_bytes[i];
                        }
                    }
                    8 => {
                        if chunk.len() == 8 {
                            let mut arr = [0u8; 8];
                            for i in 0..8 {
                                arr[i] = chunk[i].1;
                            }
                            let mut val = u64::from_le_bytes(arr);
                            let op_val = operand;
                            val = match op {
                                ModifyOp::Add => val.wrapping_add(op_val),
                                ModifyOp::Sub => val.wrapping_sub(op_val),
                                ModifyOp::Mul => val.wrapping_mul(op_val),
                                ModifyOp::Div => if op_val != 0 { val / op_val } else { val },
                                ModifyOp::Xor => val ^ op_val,
                                ModifyOp::Or => val | op_val,
                                ModifyOp::And => val & op_val,
                                ModifyOp::ShiftLeft => val << (op_val & 63),
                                ModifyOp::ShiftRight => val >> (op_val & 63),
                                _ => val,
                            };
                            let res_bytes = val.to_le_bytes();
                            for i in 0..8 {
                                chunk[i].1 = res_bytes[i];
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    app.state = UIState::HexEditing;
    for (offset, new_byte) in bytes {
        crate::hex::edit::record_edit(app, offset, new_byte);
    }
    app.hex_view.selection.clear();
}
