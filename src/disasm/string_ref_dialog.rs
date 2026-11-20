use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers};
use std::io::Result;
use tui_input::Input;

use crate::{app::App, editor::{AppView, UIState}};
use super::string_ref::{scan_string_references, StringRefItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodingFilter {
    #[default]
    All,
    Ascii,
    Utf8,
    Cp949,
    Cp936,
    Utf16,
}

impl EncodingFilter {
    /// Label for the filter row.
    ///
    /// Only `All` is translated: the rest are codepage names, and a name reads the
    /// same in every language.
    pub fn label(&self, lang: crate::i18n::Lang) -> &'static str {
        match self {
            Self::All => crate::i18n::M::LblAllEncodings.tr(lang),
            _ => self.as_str(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Ascii => "ASCII",
            Self::Utf8 => "UTF-8",
            Self::Cp949 => "CP949(Korean)",
            Self::Cp936 => "CP936(Chinese)",
            Self::Utf16 => "UTF-16LE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::Ascii,
            Self::Ascii => Self::Utf8,
            Self::Utf8 => Self::Cp949,
            Self::Cp949 => Self::Cp936,
            Self::Cp936 => Self::Utf16,
            Self::Utf16 => Self::All,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::All => Self::Utf16,
            Self::Ascii => Self::All,
            Self::Utf8 => Self::Ascii,
            Self::Cp949 => Self::Utf8,
            Self::Cp936 => Self::Cp949,
            Self::Utf16 => Self::Cp936,
        }
    }
}

pub struct StringRefDialog {
    pub items: Vec<StringRefItem>,
    pub filtered_indices: Vec<usize>,
    pub filter_input: Input,
    /// Character a Shift-selection in the filter box started from, or `None`.
    pub filter_anchor: Option<usize>,
    pub focus_filter: bool,
    pub encoding_filter: EncodingFilter,
    pub selected_index: usize,
    /// Selection anchor for multi-row block selection in the table, or `None`.
    pub selection_anchor: Option<usize>,
}

impl Default for StringRefDialog {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            filtered_indices: Vec::new(),
            filter_input: Input::default(),
            filter_anchor: None,
            focus_filter: false,
            encoding_filter: EncodingFilter::All,
            selected_index: 0,
            selection_anchor: None,
        }
    }
}

impl StringRefDialog {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn scan_and_load(&mut self, app: &App) {
        self.items = scan_string_references(app);
        self.filter_input = Input::default();
        self.focus_filter = false;
        self.selected_index = 0;
        self.selection_anchor = None;
        self.update_filter();
    }

    pub fn update_filter(&mut self) {
        self.selection_anchor = None;
        let pattern = self.filter_input.value().trim();
        self.filtered_indices.clear();

        let compiled_re = crate::hex::strings::build_safe_regex(pattern);
        let lower_pat = pattern.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            let match_enc = match self.encoding_filter {
                EncodingFilter::All => true,
                EncodingFilter::Ascii => item.encoding_kind == "ASCII",
                EncodingFilter::Utf8 => item.encoding_kind == "UTF-8" || item.encoding_kind == "ASCII",
                EncodingFilter::Cp949 => item.encoding_kind == "CP949",
                EncodingFilter::Cp936 => item.encoding_kind == "CP936",
                EncodingFilter::Utf16 => item.encoding_kind == "UTF-16LE",
            };

            if !match_enc {
                continue;
            }

            if pattern.is_empty() {
                self.filtered_indices.push(idx);
            } else if let Some(re) = &compiled_re {
                // A non-empty match, not just any match: see
                // `util::has_nonempty_match`.
                let hit = |text: &str| crate::util::has_nonempty_match(re, text);
                if hit(&item.string_text)
                    || hit(&item.instr_text)
                    || hit(&item.va_str_64)
                    || hit(&item.va_str_32)
                {
                    self.filtered_indices.push(idx);
                }
            } else {
                if item.string_text.to_lowercase().contains(&lower_pat)
                    || item.instr_text.to_lowercase().contains(&lower_pat)
                    || item.va_str_64.to_lowercase().contains(&lower_pat)
                    || item.va_str_32.to_lowercase().contains(&lower_pat)
                {
                    self.filtered_indices.push(idx);
                }
            }
        }

        if self.filtered_indices.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len() - 1;
        }
    }

    pub fn reset(&mut self) {
        self.items.clear();
        self.filtered_indices.clear();
        self.filter_input = Input::default();
        self.filter_anchor = None;
        self.focus_filter = false;
        self.encoding_filter = EncodingFilter::All;
        self.selected_index = 0;
        self.selection_anchor = None;
    }
}

fn fixed_centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width, height)
}

pub fn draw_string_ref_dialog(app: &mut App, frame: &mut Frame, area: Rect) {
    let width = (area.width * 9 / 10).max(96).min(area.width);
    let height = (area.height * 9 / 10).max(22).min(area.height);
    let popup_area = fixed_centered_rect(width, height, area);

    // Clear background
    frame.render_widget(Clear, popup_area);

    let dialog_style = app.config.theme.dialog;
    let dialog = &app.disasm_string_ref_dialog;

    let title = format!(
        " {} ({} / {} {}) ",
        crate::i18n::M::StringRefsTitle.tr(app.config.lang),
        dialog.filtered_indices.len(),
        dialog.items.len(),
        crate::i18n::M::FoundCount.tr(app.config.lang)
    );

    let outer_block = Block::default()
        .title(title)
        // Which key goes where. Two destinations for one list is not guessable, and
        // the bottom border is empty space that costs nothing.
        .title_bottom(crate::i18n::M::RefsFooterKeys.tr(app.config.lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = outer_block.inner(popup_area);
    frame.render_widget(outer_block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(1),    // String References Table (Top)
            Constraint::Length(1), // Blank Gap Line
            Constraint::Length(3), // Filter Input Box & Encoding Filter (Bottom)
        ])
        .split(inner_area);

    // 1. String References Table (Address | Disassembly | Text string)
    let is_64 = app.is_64();
    let addr_col_len = if is_64 {
        let max_len = dialog.items.iter()
            .map(|item| item.va_str_64.len())
            .max()
            .unwrap_or(9);
        (max_len.max(crate::i18n::M::LblAddress.tr(app.config.lang).chars().count())) as u16
    } else {
        8
    };

    let header_cells = [
        Cell::new(crate::i18n::M::LblAddress.tr(app.config.lang))
            .style(dialog_style.add_modifier(Modifier::BOLD)),
        Cell::new(crate::i18n::M::LblDisassembly.tr(app.config.lang))
            .style(dialog_style.add_modifier(Modifier::BOLD)),
        Cell::new(crate::i18n::M::LblTextString.tr(app.config.lang))
            .style(dialog_style.add_modifier(Modifier::BOLD)),
    ];
    let header = Row::new(header_cells).style(dialog_style).bottom_margin(1);

    let visible_rows = chunks[0].height.saturating_sub(2) as usize;
    let total_count = dialog.filtered_indices.len();

    let (start_idx, end_idx, select_offset) = if total_count == 0 {
        (0, 0, None)
    } else {
        let sel = dialog.selected_index.min(total_count - 1);
        let half = visible_rows / 2;
        let start = if sel > half {
            (sel - half).min(total_count.saturating_sub(visible_rows))
        } else {
            0
        };
        let end = (start + visible_rows + 5).min(total_count);
        let rel_sel = sel.saturating_sub(start);
        (start, end, Some(rel_sel))
    };

    let (sel_min, sel_max) = match dialog.selection_anchor {
        Some(anchor) => (
            anchor.min(dialog.selected_index),
            anchor.max(dialog.selected_index),
        ),
        None => (dialog.selected_index, dialog.selected_index),
    };

    let mut rows = Vec::with_capacity(end_idx - start_idx);
    for idx in start_idx..end_idx {
        let item_idx = dialog.filtered_indices[idx];
        let item = &dialog.items[item_idx];
        let in_selection = total_count > 0 && idx >= sel_min && idx <= sel_max;
        let is_selected = idx == dialog.selected_index;
        let row_style = if in_selection {
            let mut style = app.config.theme.highlight;
            if is_selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            style
        } else {
            dialog_style
        };

        let va_str = if is_64 {
            &item.va_str_64
        } else {
            &item.va_str_32
        };

        let cells = vec![
            Cell::new(va_str.as_str()).style(row_style),
            Cell::new(item.instr_text.as_str()).style(row_style),
            Cell::new(item.full_text_str.as_str()).style(row_style),
        ];

        rows.push(Row::new(cells).style(row_style));
    }

    let widths = [
        Constraint::Length(addr_col_len),
        Constraint::Length(26),
        Constraint::Min(30),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(2)
        .style(dialog_style);

    let mut table_state = TableState::default();
    table_state.select(select_offset);

    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    // Same notice the strings dialog draws, in the same place and for the same
    // reason: the command bar version is cleared by the next key press, and an empty
    // table says nothing about why it is empty.
    if total_count == 0
        && crate::hex::strings::matches_the_empty_string(dialog.filter_input.value())
    {
        let notice = Paragraph::new(crate::i18n::M::WarnRegexEmptyOnly.tr(app.config.lang))
            .style(app.config.theme.error)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(Block::new().padding(ratatui::widgets::Padding::uniform(1)));
        frame.render_widget(notice, chunks[0]);
    }

    // 2. Filter Input Box (Bottom)
    let filter_border_style = if dialog.focus_filter {
        app.config.theme.highlight
    } else {
        dialog_style
    };

    // The same two labels the strings dialog uses, so both boxes read the same and
    // both follow `:set lang`. This row was written out in English by hand.
    let filter_title = format!(
        "{}| {}: [ {} ] (F2) ",
        crate::i18n::M::FilterRegexTitle.tr(app.config.lang),
        crate::i18n::M::Encoding.tr(app.config.lang),
        dialog.encoding_filter.label(app.config.lang)
    );

    let filter_block = Block::default()
        .title(filter_title)
        .borders(Borders::ALL)
        .border_style(filter_border_style)
        .style(dialog_style);

    let filter_para = Paragraph::new(crate::text_field::render_line(
        &dialog.filter_input,
        dialog.filter_anchor,
        dialog_style,
        app.config.theme.highlight,
    ))
    .style(dialog_style)
    .block(filter_block);
    frame.render_widget(filter_para, chunks[2]);

    if dialog.focus_filter {
        let cursor_x = chunks[2].x + 1 + dialog.filter_input.visual_cursor() as u16;
        let cursor_y = chunks[2].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn measure_string_capacity(app: &App, offset: usize, is_utf16: bool) -> (usize, usize) {
    let buf_len = app.file_info.buffer_len();
    if offset >= buf_len {
        return (0, 0);
    }
    let max_read = (buf_len - offset).min(1024);
    let mut temp = vec![0u8; max_read];
    app.read_effective_slice(offset, &mut temp);
    let slice = &temp[..];
    let term_size = if is_utf16 { 2 } else { 1 };
    if is_utf16 {
        let mut len = 0;
        for chunk in slice.chunks_exact(2) {
            if chunk[0] == 0 && chunk[1] == 0 {
                break;
            }
            len += 2;
            if len >= 512 {
                break;
            }
        }
        let mut total_len = len;
        // Count contiguous trailing 00 padding bytes
        while total_len + 1 < slice.len() && slice[total_len] == 0 && slice[total_len + 1] == 0 {
            total_len += 2;
            if total_len >= 1024 {
                break;
            }
        }
        let mut budget = total_len.saturating_sub(term_size);
        budget -= budget % 2;
        (budget, total_len)
    } else {
        let mut len = 0;
        for &b in slice {
            if b == 0 {
                break;
            }
            len += 1;
            if len >= 512 {
                break;
            }
        }
        let mut total_len = len;
        // Count contiguous trailing 00 padding bytes
        while total_len < slice.len() && slice[total_len] == 0 {
            total_len += 1;
            if total_len >= 1024 {
                break;
            }
        }
        let budget = total_len.saturating_sub(term_size);
        (budget, total_len)
    }
}

pub fn open_string_ref_edit(app: &mut App) {
    let dialog = &app.disasm_string_ref_dialog;
    let Some(&real_idx) = dialog.filtered_indices.get(dialog.selected_index) else {
        crate::beep!();
        return;
    };
    let Some(item) = dialog.items.get(real_idx).cloned() else {
        crate::beep!();
        return;
    };

    let is_utf16 = item.encoding_kind == "UTF-16LE";
    let raw_text = if is_utf16 && item.string_text.starts_with("L\"") && item.string_text.ends_with('"') {
        item.string_text[2..item.string_text.len() - 1].to_string()
    } else {
        item.string_text.trim_matches('"').to_string()
    };

    let (budget, total_capacity) = measure_string_capacity(app, item.string_offset, is_utf16);
    let (budget, total_capacity) = if total_capacity == 0 {
        let raw_len = crate::util::encode_text(
            &raw_text,
            match item.encoding_kind {
                "CP949" => encoding_rs::EUC_KR,
                "CP936" => encoding_rs::GBK,
                "UTF-16LE" => encoding_rs::UTF_16LE,
                _ => encoding_rs::UTF_8,
            },
        )
        .len();
        let term_size = if is_utf16 { 2 } else { 1 };
        (raw_len, raw_len + term_size)
    } else {
        (budget, total_capacity)
    };

    let encoding = match item.encoding_kind {
        "ASCII" => crate::hex::strings::StringEncoding::Ascii,
        "UTF-8" => crate::hex::strings::StringEncoding::Utf8,
        "CP949" => crate::hex::strings::StringEncoding::Cp949,
        "CP936" => crate::hex::strings::StringEncoding::Cp936,
        "UTF-16LE" => crate::hex::strings::StringEncoding::Utf16,
        _ => crate::hex::strings::StringEncoding::Ascii,
    };

    let cursor = raw_text.chars().count();
    app.hex_view.string_edit = crate::hex::strings::StringEdit {
        offset: item.string_offset,
        budget,
        total_capacity,
        row: usize::MAX,
        encoding,
        input: tui_input::Input::new(raw_text).with_cursor(cursor),
        anchor: Some(0),
        error: None,
        return_state: Some(UIState::DialogStringRef),
    };

    app.state = UIState::DialogStringEdit;
    app.dialog_2nd_renderer = Some(crate::hex::strings::dialog_string_edit_draw);
}

pub fn dialog_string_ref_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Mouse(mouse) = event {
        if let ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) = mouse.kind {
            let now = std::time::Instant::now();
            let is_double_click = if let Some((last_time, last_row, last_col)) = app.last_left_click {
                now.duration_since(last_time).as_millis() < 400 && last_row == mouse.row && last_col == mouse.column
            } else {
                false
            };

            let total = app.disasm_string_ref_dialog.filtered_indices.len();
            let height = (app.screen.height * 9 / 10).max(22).min(app.screen.height);
            let dialog_y = app.screen.y + (app.screen.height.saturating_sub(height)) / 2;
            let table_start_y = dialog_y + 4;
            let visible_rows = (height.saturating_sub(10) as usize).max(1);
            let is_in_table = mouse.row >= table_start_y && (mouse.row - table_start_y) < visible_rows as u16;

            if is_double_click && is_in_table {
                // First click already selected the correct row — just open edit.
                app.last_left_click = None;
                open_string_ref_edit(app);
                return Ok(true);
            } else if total > 0 {
                let dialog = &mut app.disasm_string_ref_dialog;
                let sel = dialog.selected_index.min(total - 1);
                let half = visible_rows / 2;
                let start = if sel > half {
                    (sel - half).min(total.saturating_sub(visible_rows))
                } else {
                    0
                };

                if mouse.row >= table_start_y && (mouse.row - table_start_y) < visible_rows as u16 {
                    let clicked_row = (mouse.row - table_start_y) as usize;
                    let clicked_index = (start + clicked_row).min(total - 1);
                    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                        if dialog.selection_anchor.is_none() {
                            dialog.selection_anchor = Some(dialog.selected_index);
                        }
                    } else {
                        dialog.selection_anchor = None;
                    }
                    dialog.selected_index = clicked_index;
                }
            }
            app.last_left_click = Some((now, mouse.row, mouse.column));
        }
        if let ratatui::crossterm::event::MouseEventKind::Drag(ratatui::crossterm::event::MouseButton::Left) = mouse.kind {
            let dialog = &mut app.disasm_string_ref_dialog;
            let total = dialog.filtered_indices.len();
            if total > 0 {
                let height = (app.screen.height * 9 / 10).max(22).min(app.screen.height);
                let dialog_y = app.screen.y + (app.screen.height.saturating_sub(height)) / 2;
                let table_start_y = dialog_y + 4;
                let visible_rows = (height.saturating_sub(10) as usize).max(1);

                let sel = dialog.selected_index.min(total - 1);
                let half = visible_rows / 2;
                let start = if sel > half {
                    (sel - half).min(total.saturating_sub(visible_rows))
                } else {
                    0
                };

                if mouse.row >= table_start_y && (mouse.row - table_start_y) < visible_rows as u16 {
                    let clicked_row = (mouse.row - table_start_y) as usize;
                    let clicked_index = (start + clicked_row).min(total - 1);
                    if dialog.selection_anchor.is_none() {
                        dialog.selection_anchor = Some(dialog.selected_index);
                    }
                    dialog.selected_index = clicked_index;
                }
            }
        }
        match mouse.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                app.disasm_string_ref_dialog.selection_anchor = None;
                if app.disasm_string_ref_dialog.selected_index > 0 {
                    app.disasm_string_ref_dialog.selected_index =
                        app.disasm_string_ref_dialog.selected_index.saturating_sub(3);
                }
            }
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                app.disasm_string_ref_dialog.selection_anchor = None;
                let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                if cnt > 0 {
                    app.disasm_string_ref_dialog.selected_index =
                        (app.disasm_string_ref_dialog.selected_index + 3).min(cnt - 1);
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    if let Event::Key(key) = event {

        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        let is_filter_focused = app.disasm_string_ref_dialog.focus_filter;

        match key.code {
            KeyCode::Esc => {
                if is_filter_focused {
                    app.disasm_string_ref_dialog.focus_filter = false;
                } else if app.disasm_string_ref_dialog.selection_anchor.is_some() {
                    app.disasm_string_ref_dialog.selection_anchor = None;
                } else {
                    app.disasm_string_ref_dialog.reset();
                    app.dialog_renderer = None;
                    app.dialog_2nd_renderer = None;
                    app.state = UIState::Normal;
                }
                return Ok(false);
            }
            KeyCode::F(4) => {
                open_string_ref_edit(app);
                return Ok(false);
            }
            KeyCode::F(2) => {
                let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.disasm_string_ref_dialog.encoding_filter = if is_shift {
                    app.disasm_string_ref_dialog.encoding_filter.prev()
                } else {
                    app.disasm_string_ref_dialog.encoding_filter.next()
                };
                app.disasm_string_ref_dialog.update_filter();
                return Ok(false);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                app.disasm_string_ref_dialog.focus_filter = !is_filter_focused;
                return Ok(false);
            }
            KeyCode::Home if !is_filter_focused => {
                set_string_ref_selection(app, 0, key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::End if !is_filter_focused => {
                let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                set_string_ref_selection(app, cnt.saturating_sub(1), key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::Up if !is_filter_focused => {
                move_string_ref_selection(app, -1, key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::Down if !is_filter_focused => {
                move_string_ref_selection(app, 1, key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::PageUp if !is_filter_focused => {
                move_string_ref_selection(app, -12, key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::PageDown if !is_filter_focused => {
                move_string_ref_selection(app, 12, key.modifiers.contains(KeyModifiers::SHIFT));
                return Ok(false);
            }
            KeyCode::Char('a') | KeyCode::Char('A') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if is_filter_focused {
                    if crate::text_field::handle_key(app, refs_filter_field, event) {
                        app.disasm_string_ref_dialog.update_filter();
                    }
                } else {
                    let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                    if cnt > 0 {
                        app.disasm_string_ref_dialog.selection_anchor = Some(0);
                        app.disasm_string_ref_dialog.selected_index = cnt - 1;
                    }
                }
                return Ok(false);
            }
            KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
                let (text, count) = filtered_rows_as_tsv(app);
                app.copy_to_clipboard(text, format!("{} reference(s)", count));
                return Ok(false);
            }
            KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if is_filter_focused {
                    let (input, anchor) = refs_filter_field(app);
                    let (_, selected, _) = crate::text_field::split(input, *anchor);
                    let text = if selected.is_empty() {
                        input.value().to_string()
                    } else {
                        selected.to_string()
                    };
                    app.copy_to_clipboard(text, "filter text".to_string());
                } else {
                    let (text, count) = selected_rows_as_tsv(app);
                    let label = if count == 1 {
                        "1 reference".to_string()
                    } else {
                        format!("{} reference(s)", count)
                    };
                    app.copy_to_clipboard(text, label);
                }
                return Ok(false);
            }
            // Switch to F6 Strings dialog while preserving state & matching cursor (F6).
            KeyCode::F(6) => {
                app.dialog_renderer = None;

                let selected_string_offset = {
                    let dialog = &app.disasm_string_ref_dialog;
                    dialog
                        .filtered_indices
                        .get(dialog.selected_index)
                        .and_then(|&i| dialog.items.get(i))
                        .map(|item| item.string_offset)
                };

                if app.strings.is_empty() {
                    crate::commands::Commands::strings(app);
                } else {
                    app.state = UIState::DialogStrings;
                    app.dialog_renderer = Some(crate::hex::strings::dialog_strings_draw);
                    app.dialog_2nd_renderer = None;
                }

                if let Some(target_ofs) = selected_string_offset
                    && let Some(pos) = app.hex_view.strings_filtered.iter().position(|&idx| {
                        app.strings.get(idx).map(|s| s.offset) == Some(target_ofs)
                    })
                {
                    app.list_state.select(Some(pos));
                }
                return Ok(false);
            }
            KeyCode::Enter => {
                let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                if cnt > 0 {
                    let sel = app.disasm_string_ref_dialog.selected_index;
                    let real_idx = app.disasm_string_ref_dialog.filtered_indices[sel];
                    let (instr_offset, instr_va, str_offset, str_va) = {
                        let item = &app.disasm_string_ref_dialog.items[real_idx];
                        (item.offset, item.va, item.string_offset, item.string_va)
                    };

                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        follow_to_string(app, str_offset, str_va);
                    } else {
                        follow_to_code(app, instr_offset, instr_va);
                    }
                }
                // The results are kept, not reset: Alt+Left reopens this list.
                app.last_result = Some(UIState::DialogStringRef);
                app.dialog_renderer = None;
                app.dialog_2nd_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            // Shift+arrows and Shift+Home/End edit the box; the list's own arrow
            // handling above is gated on the box not having focus.
            _ => {
                if is_filter_focused {
                    if crate::text_field::handle_key(app, refs_filter_field, event) {
                        app.disasm_string_ref_dialog.update_filter();
                    }
                    return Ok(false);
                }
            }
        }
    }
    Ok(false)
}

/// The refs dialog's filter box and its selection anchor.
fn refs_filter_field(app: &mut App) -> (&mut Input, &mut Option<usize>) {
    (
        &mut app.disasm_string_ref_dialog.filter_input,
        &mut app.disasm_string_ref_dialog.filter_anchor,
    )
}

/// One row as `instruction address<TAB>instruction<TAB>string<TAB>string offset`.
fn item_as_tsv(item: &StringRefItem, is_64: bool) -> String {
    let va = if is_64 { &item.va_str_64 } else { &item.va_str_32 };
    format!(
        "{}\t{}\t{}\t{:08X}",
        va, item.instr_text, item.full_text_str, item.string_offset
    )
}

/// Every row the filter left, and how many there are.
///
/// A function rather than inline in the key handler so the result can be asserted
/// without the real clipboard, which parallel tests contend over.
fn filtered_rows_as_tsv(app: &App) -> (String, usize) {
    let is_64 = app.is_64();
    let dialog = &app.disasm_string_ref_dialog;
    let rows: Vec<String> = dialog
        .filtered_indices
        .iter()
        .filter_map(|&i| dialog.items.get(i))
        .map(|item| item_as_tsv(item, is_64))
        .collect();
    // CRLF: a Windows clipboard, and a spreadsheet pasting LF-only text puts every
    // row in one cell.
    (rows.join("\r\n"), rows.len())
}

/// Selected row or block of rows as TSV lines, and how many rows there are.
pub fn selected_rows_as_tsv(app: &App) -> (String, usize) {
    let dialog = &app.disasm_string_ref_dialog;
    let cnt = dialog.filtered_indices.len();
    if cnt == 0 {
        return (String::new(), 0);
    }
    let is_64 = app.is_64();
    let (s, e) = match dialog.selection_anchor {
        Some(anchor) => (
            anchor.min(dialog.selected_index),
            anchor.max(dialog.selected_index).min(cnt - 1),
        ),
        None => (dialog.selected_index.min(cnt - 1), dialog.selected_index.min(cnt - 1)),
    };
    let rows: Vec<String> = (s..=e)
        .filter_map(|idx| dialog.filtered_indices.get(idx))
        .filter_map(|&i| dialog.items.get(i))
        .map(|item| item_as_tsv(item, is_64))
        .collect();
    let count = rows.len();
    (rows.join("\r\n"), count)
}

fn move_string_ref_selection(app: &mut App, delta: isize, is_shift: bool) {
    let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
    if cnt == 0 {
        app.disasm_string_ref_dialog.selected_index = 0;
        app.disasm_string_ref_dialog.selection_anchor = None;
        return;
    }
    if is_shift {
        if app.disasm_string_ref_dialog.selection_anchor.is_none() {
            app.disasm_string_ref_dialog.selection_anchor = Some(app.disasm_string_ref_dialog.selected_index);
        }
    } else {
        app.disasm_string_ref_dialog.selection_anchor = None;
    }
    let current = app.disasm_string_ref_dialog.selected_index as isize;
    let next = current.saturating_add(delta).clamp(0, cnt as isize - 1);
    app.disasm_string_ref_dialog.selected_index = next as usize;
}

fn set_string_ref_selection(app: &mut App, target: usize, is_shift: bool) {
    let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
    if cnt == 0 {
        app.disasm_string_ref_dialog.selected_index = 0;
        app.disasm_string_ref_dialog.selection_anchor = None;
        return;
    }
    if is_shift {
        if app.disasm_string_ref_dialog.selection_anchor.is_none() {
            app.disasm_string_ref_dialog.selection_anchor = Some(app.disasm_string_ref_dialog.selected_index);
        }
    } else {
        app.disasm_string_ref_dialog.selection_anchor = None;
    }
    app.disasm_string_ref_dialog.selected_index = target.min(cnt - 1);
}

/// Switches to `view`, keeping the way back and the page in order.
fn enter_view(app: &mut App, view: AppView) {
    if app.editor_view != view {
        // The same bookkeeping the F4 / F7 view switches do, so Esc and the jump
        // history still know where the user came from.
        app.prev_editor_view = app.editor_view;
        app.editor_view = view;
        app.last_primary_view = view;
    }
}

/// Enter: the code that references the string.
///
/// Goes to the address the row is named after - the instruction - in the
/// Disassembly view. That is the address in the list's first column, and this list
/// exists to answer "which code touches this string". It used to jump in whichever
/// view happened to be open, which for a list of virtual addresses was usually the
/// hex dump.
///
/// A non-executable file has no Disassembly view, so it lands in Hex instead.
fn follow_to_code(app: &mut App, offset: usize, va: u64) {
    let view = if app.is_executable() { AppView::Disasm } else { AppView::Hex };
    enter_view(app, view);

    if view == AppView::Disasm {
        // `goto` deliberately leaves `page_start` alone in this view, because
        // instruction boundaries are not arithmetic - so the page is anchored here
        // instead. `offset` is a boundary by construction: it is where the
        // instruction the row describes begins.
        app.reader.page_start = offset;
    }

    app.goto(offset);
    // `page_start` means different things in the two views, and the jump may have
    // just crossed between them.
    app.align_page_for_view();

    let view_name = if view == AppView::Hex { "Hex" } else { "Disassembly" };
    App::log(
        app,
        format!(
            "Jumped to the referencing instruction at 0x{:X} (offset 0x{:X}) in the {} view",
            va, offset, view_name
        ),
    );
}

/// Ctrl+Enter: the string itself, in the Hex view.
///
/// The bytes worth editing are the string's, not the instruction's - following the
/// reference is what Ctrl+Enter already means in the Disassembly view. The
/// instruction is one Ctrl+Left away if it was wanted after all.
fn follow_to_string(app: &mut App, offset: usize, va: u64) {
    enter_view(app, AppView::Hex);
    app.goto(offset);
    app.align_page_for_view();
    App::log(
        app,
        format!(
            "Jumped to the string at 0x{:X} (offset 0x{:X}) in the Hex view",
            va, offset
        ),
    );
}
#[cfg(test)]
mod follow_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    /// The running test binary is itself a PE, so it is the cheapest executable to
    /// hand the parser. Opened read-only; nothing here writes.
    fn app_with_pe() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, false).ok()?;
        app.header_view.pe.as_ref()?;
        app.config.hex_mode_bytes_per_line = 16;
        app.reader.page_current_size = 16 * 20;
        Some(app)
    }

    fn one_item(app: &mut App, offset: usize, va: u64) {
        one_item_at(app, offset, va, offset + 0x40, va + 0x40)
    }

    fn one_item_at(app: &mut App, offset: usize, va: u64, string_offset: usize, string_va: u64) {
        app.disasm_string_ref_dialog.items = vec![StringRefItem {
            offset,
            va,
            string_offset,
            string_va,
            va_str_64: if va >= 0x1_0000_0000 { format!("{:X}", va) } else { format!("{:08X}", va) },
            va_str_32: format!("{:08X}", va),
            instr_text: "lea rdx,[0x140002000]".to_string(),
            string_text: "\"hello\"".to_string(),
            encoding_kind: "ASCII",
            full_text_str: "ASCII \"hello\"".to_string(),
        }];
        app.disasm_string_ref_dialog.selected_index = 0;
        app.disasm_string_ref_dialog.focus_filter = false;
        app.disasm_string_ref_dialog.update_filter();
        app.state = UIState::DialogStringRef;
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = dialog_string_ref_events(app, &Event::Key(key));
    }

    /// Enter goes to the address in the list's first column, which is a virtual
    /// address - so it belongs in the Disassembly view. It used to land in whatever
    /// view was open, normally the hex dump.
    #[test]
    fn enter_goes_to_the_disassembly_view() {
        let Some(mut app) = app_with_pe() else { return };
        app.editor_view = AppView::Hex;
        app.reader.page_start = 0;

        let target = 0x600usize;
        assert!(target < app.file_info.size);
        one_item(&mut app, target, 0x140001234);

        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

        assert!(app.editor_view == AppView::Disasm, "Enter must land in the Disassembly view");
        assert_eq!(app.hex_view.offset, target, "the cursor must be on the instruction");
        // The listing starts at the selected row, or at the instruction boundary just
        // before it. Not `== target`: `align_page_for_view` re-syncs the page to a
        // boundary, and whether a given offset in this binary happens to be one
        // changes every time the test binary is recompiled.
        let start = app.reader.page_start;
        assert!(
            // 16 is the longest x86 instruction.
            start <= target && target - start < 16,
            "the listing starts at 0x{:X}, nowhere near the selected row 0x{:X}",
            start,
            target
        );
        assert_eq!(
            crate::disasm::nav::containing_instruction(&app, start),
            start,
            "0x{:X} is not an instruction boundary",
            start
        );
        assert!(app.state == UIState::Normal, "the dialog has to close");
        assert!(app.dialog_renderer.is_none());
        assert!(
            app.prev_editor_view == AppView::Hex,
            "the way back has to be remembered"
        );
    }

    /// Ctrl+Enter goes to the *string*, not to the instruction, and the byte grid
    /// has to stay aligned when it gets there.
    ///
    /// The bytes worth editing are the string's. Landing on the `lea` that points at
    /// it - which is what this used to do - puts the cursor on the one address in the
    /// file a translator has no use for.
    #[test]
    fn ctrl_enter_goes_to_the_string_in_hex() {
        let Some(mut app) = app_with_pe() else { return };
        app.editor_view = AppView::Disasm;
        app.reader.page_start = 0x100;
        // A `page_end` left over from somewhere else, the way the Disasm view leaves
        // it: this is what used to keep the page from following the cursor.
        app.reader.page_end = 0x20;

        let instr = 0x800usize;
        let target = 0x1234usize;
        if target >= app.file_info.size {
            return;
        }
        one_item_at(&mut app, instr, 0x140000800, target, 0x140004321);

        press(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);

        let page_size = app.reader.page_current_size;
        assert!(app.editor_view == AppView::Hex, "Ctrl+Enter must land in the Hex view");
        assert_eq!(
            app.hex_view.offset, target,
            "the cursor must be on the string, not on the instruction at 0x{:X}",
            instr
        );
        assert_eq!(app.reader.page_start % 16, 0, "the hex grid must stay aligned");
        assert!(
            app.reader.page_start <= target && target < app.reader.page_start + page_size,
            "the target 0x{:X} is off screen: page 0x{:X}..0x{:X}",
            target,
            app.reader.page_start,
            app.reader.page_start + page_size
        );
    }

    /// A file with no code has no Disassembly view, so plain Enter goes to Hex.
    #[test]
    fn a_non_executable_always_lands_in_hex() {
        let dir = std::env::temp_dir().join(format!("dz6_follow_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("plain.bin");
        std::fs::write(&path, vec![0x41u8; 0x800]).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        assert!(!app.is_executable());

        one_item(&mut app, 0x200, 0x200);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

        let view = app.editor_view;
        let offset = app.hex_view.offset;
        let _ = std::fs::remove_dir_all(&dir);

        assert!(view == AppView::Hex);
        assert_eq!(offset, 0x200);
    }

    #[test]
    fn string_ref_64bit_renders_without_leading_zeros() {
        let mut app = match app_with_pe() {
            Some(a) => a,
            None => return,
        };
        one_item_at(&mut app, 0x5EB18, 0x14005EB18, 0x2000, 0x140002000);

        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            let area = f.area();
            draw_string_ref_dialog(&mut app, f, area);
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("14005EB18"));
        assert!(!rendered.contains("000000014005EB18"));
    }
}
#[cfg(test)]
mod copy_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn item(offset: usize, va: u64, string_offset: usize, text: &str) -> StringRefItem {
        StringRefItem {
            offset,
            va,
            string_offset,
            string_va: 0x140000000 + string_offset as u64,
            va_str_64: if va >= 0x1_0000_0000 { format!("{:X}", va) } else { format!("{:08X}", va) },
            va_str_32: format!("{:08X}", va),
            instr_text: "lea rdx,[0x140002000]".to_string(),
            string_text: format!("\"{}\"", text),
            encoding_kind: "ASCII",
            full_text_str: format!("ASCII \"{}\"", text),
        }
    }

    /// Four columns: instruction address, instruction, string, string offset. The
    /// last one is the point - it is the address a translator opens.
    #[test]
    fn a_row_has_the_string_offset_in_it() {
        let it = item(0x600, 0x140001234, 0xA1A38, "hello");

        assert_eq!(
            item_as_tsv(&it, true),
            "140001234\tlea rdx,[0x140002000]\tASCII \"hello\"\t000A1A38"
        );
        // A 32-bit image gets the narrow column, the same as the table.
        let it32 = item(0x600, 0x40001234, 0xA1A38, "hello");
        assert_eq!(item_as_tsv(&it32, false).split('\t').next(), Some("40001234"));
        assert_eq!(item_as_tsv(&it, true).split('\t').count(), 4);
    }

    /// `y` copies the selected row, `Y` copies what the filter left.
    #[test]
    fn y_and_shift_y_report_what_they_copied() {
        let mut app = App::new();
        app.disasm_string_ref_dialog.items = vec![
            item(0x600, 0x140001000, 0x1000, "alpha"),
            item(0x610, 0x140001010, 0x1010, "beta"),
            item(0x620, 0x140001020, 0x1020, "betamax"),
        ];
        app.disasm_string_ref_dialog.update_filter();
        app.state = UIState::DialogStringRef;

        let _press = |app: &mut App, code: KeyCode| {
            let key = KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = dialog_string_ref_events(app, &Event::Key(key));
        };

        // What `Y` hands to the clipboard. Asserted here rather than through the
        // clipboard itself: that is a shared OS resource, and parallel tests
        // fighting over it made this fail at random.
        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 3);
        assert_eq!(text.lines().count(), 3);

        // Narrowed by the filter box: Y follows it.
        app.disasm_string_ref_dialog.filter_input = tui_input::Input::new("beta".to_string());
        app.disasm_string_ref_dialog.update_filter();
        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 2, "Y must follow the filter");
        assert!(text.contains("beta") && text.contains("betamax"));
        assert!(!text.contains("alpha"));

        // Every row still carries its string offset.
        for line in text.lines() {
            assert_eq!(line.split('\t').count(), 4, "row: {:?}", line);
        }

        // The keys report something and must not close the dialog: the point is to
        // keep working through the list. Either outcome is logged, because a machine
        // without a clipboard is a legitimate case.
        app.logs.clear();
        let key_c = KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let key_shift_c = KeyEvent { code: KeyCode::Char('C'), modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_string_ref_events(&mut app, &Event::Key(key_c));
        let _ = dialog_string_ref_events(&mut app, &Event::Key(key_shift_c));
        assert_eq!(app.logs.len(), 2, "logs: {:?}", app.logs);
        assert!(app.logs.iter().all(|l| l.contains("clipboard")), "logs: {:?}", app.logs);
        assert!(app.state == UIState::DialogStringRef);
    }

    #[test]
    fn f6_key_switches_to_strings_dialog_preserving_state() {
        let mut app = App::new();
        app.disasm_string_ref_dialog.items = vec![item(0x600, 0x140001000, 0x10, "license")];
        app.disasm_string_ref_dialog.update_filter();
        app.state = UIState::DialogStringRef;

        let key_f6 = KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(key_f6));

        assert!(matches!(app.state, UIState::DialogStrings));
    }

    /// While the filter box has focus, `y` is text.
    #[test]
    fn typing_in_the_filter_box_still_types() {
        let mut app = App::new();
        app.disasm_string_ref_dialog.items = vec![item(0x600, 0x140001000, 0x1000, "yes")];
        app.disasm_string_ref_dialog.update_filter();
        app.disasm_string_ref_dialog.focus_filter = true;
        app.state = UIState::DialogStringRef;

        for code in [KeyCode::Char('y'), KeyCode::Char('c'), KeyCode::Char('Y')] {
            let key = KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = dialog_string_ref_events(&mut app, &Event::Key(key));
        }

        assert_eq!(app.disasm_string_ref_dialog.filter_input.value(), "ycY");
    }

    #[test]
    fn string_ref_edit_budget_includes_trailing_nulls_on_scy() {
        let path = "C:\\Users\\Administrator\\Desktop\\pecmd\\dumped_SCY.exe";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut app = App::new();
        app.config.database = false;
        if app.load_file(path, 0, false).is_err() {
            return;
        }
        let items = crate::disasm::string_ref::scan_string_references(&app);
        app.disasm_string_ref_dialog.items = items;
        app.disasm_string_ref_dialog.update_filter();

        let pos = app.disasm_string_ref_dialog.filtered_indices.iter().position(|&idx| {
            app.disasm_string_ref_dialog.items[idx].string_offset == 0x1206A8
        });
        assert!(pos.is_some(), "Expected string ref for 0x1206A8");
        app.disasm_string_ref_dialog.selected_index = pos.unwrap();

        let key_f4 = KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(key_f4));

        assert!(app.state == UIState::DialogStringEdit);
        assert_eq!(app.hex_view.string_edit.budget, 46);
        assert_eq!(app.hex_view.string_edit.total_capacity, 48);
        assert_eq!(app.hex_view.string_edit.input.value(), "分辨率 %d×%d 颜色%d位 刷新率%d");
    }

    #[test]
    fn string_ref_block_selection_and_ctrl_a_ctrl_c() {
        let mut app = App::new();
        app.disasm_string_ref_dialog.items = vec![
            item(0x600, 0x140001000, 0x1000, "first"),
            item(0x610, 0x140001010, 0x1010, "second"),
            item(0x620, 0x140001020, 0x1020, "third"),
            item(0x630, 0x140001030, 0x1030, "fourth"),
        ];
        app.disasm_string_ref_dialog.update_filter();
        app.state = UIState::DialogStringRef;

        // Initially row 0 selected, no anchor
        assert_eq!(app.disasm_string_ref_dialog.selected_index, 0);
        assert_eq!(app.disasm_string_ref_dialog.selection_anchor, None);

        // Single row copy
        let (text1, count1) = selected_rows_as_tsv(&app);
        assert_eq!(count1, 1);
        assert!(text1.contains("first"));

        // Shift+Down extends selection to row 1
        let shift_down = KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(shift_down));
        assert_eq!(app.disasm_string_ref_dialog.selection_anchor, Some(0));
        assert_eq!(app.disasm_string_ref_dialog.selected_index, 1);

        // Block copy copies 2 rows
        let (text2, count2) = selected_rows_as_tsv(&app);
        assert_eq!(count2, 2);
        assert_eq!(text2.lines().count(), 2);
        assert!(text2.contains("first") && text2.contains("second"));

        // Plain Down moves to row 2 and clears anchor
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(down));
        assert_eq!(app.disasm_string_ref_dialog.selection_anchor, None);
        assert_eq!(app.disasm_string_ref_dialog.selected_index, 2);

        // Ctrl+A selects all 4 rows
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(ctrl_a));
        assert_eq!(app.disasm_string_ref_dialog.selection_anchor, Some(0));
        assert_eq!(app.disasm_string_ref_dialog.selected_index, 3);

        // Block copy after Ctrl+A copies all 4 rows
        let (text_all, count_all) = selected_rows_as_tsv(&app);
        assert_eq!(count_all, 4);
        assert_eq!(text_all.lines().count(), 4);

        // Esc clears block selection without closing dialog
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let _ = dialog_string_ref_events(&mut app, &Event::Key(esc));
        assert_eq!(app.disasm_string_ref_dialog.selection_anchor, None);
        assert_eq!(app.state, UIState::DialogStringRef);

        // Second Esc closes dialog
        let _ = dialog_string_ref_events(&mut app, &Event::Key(esc));
        assert_eq!(app.state, UIState::Normal);
    }
}