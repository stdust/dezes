use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers};
use regex::Regex;
use std::io::Result;
use tui_input::Input;

use crate::{app::App, editor::{AppView, UIState}};
use super::string_ref::{scan_string_references, StringRefItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingFilter {
    All,
    Ascii,
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
            Self::Ascii => "ASCII/UTF-8",
            Self::Cp949 => "CP949(Korean)",
            Self::Cp936 => "CP936(Chinese)",
            Self::Utf16 => "UTF-16LE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::Ascii,
            Self::Ascii => Self::Cp949,
            Self::Cp949 => Self::Cp936,
            Self::Cp936 => Self::Utf16,
            Self::Utf16 => Self::All,
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
        self.update_filter();
    }

    pub fn update_filter(&mut self) {
        let pattern = self.filter_input.value().trim();
        self.filtered_indices.clear();

        let compiled_re = if !pattern.is_empty() {
            Regex::new(&format!("(?i){}", pattern)).ok()
        } else {
            None
        };
        let lower_pat = pattern.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            let match_enc = match self.encoding_filter {
                EncodingFilter::All => true,
                EncodingFilter::Ascii => item.encoding_kind == "ASCII",
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
    let popup_area = fixed_centered_rect(96, 22, area);

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
    let addr_col_len = if is_64 { 16 } else { 8 };

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

    let mut rows = Vec::with_capacity(end_idx - start_idx);
    for idx in start_idx..end_idx {
        let item_idx = dialog.filtered_indices[idx];
        let item = &dialog.items[item_idx];
        let is_selected = idx == dialog.selected_index;
        let row_style = if is_selected {
            app.config.theme.highlight.add_modifier(Modifier::BOLD)
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

pub fn dialog_string_ref_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        let is_filter_focused = app.disasm_string_ref_dialog.focus_filter;

        match key.code {
            KeyCode::Esc => {
                app.disasm_string_ref_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            KeyCode::F(2) => {
                app.disasm_string_ref_dialog.encoding_filter =
                    app.disasm_string_ref_dialog.encoding_filter.next();
                app.disasm_string_ref_dialog.update_filter();
                return Ok(false);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                app.disasm_string_ref_dialog.focus_filter = !is_filter_focused;
                return Ok(false);
            }
            KeyCode::Up if !is_filter_focused => {
                if app.disasm_string_ref_dialog.selected_index > 0 {
                    app.disasm_string_ref_dialog.selected_index -= 1;
                }
                return Ok(false);
            }
            KeyCode::Down if !is_filter_focused => {
                let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                if cnt > 0 && app.disasm_string_ref_dialog.selected_index + 1 < cnt {
                    app.disasm_string_ref_dialog.selected_index += 1;
                }
                return Ok(false);
            }
            // `y` the selected row, `Y` everything the filter left. Four
            // tab-separated columns - instruction address, instruction, the string,
            // and the string's own file offset - which is the pair a translator
            // actually works from once it is in a spreadsheet.
            KeyCode::Char('y') | KeyCode::Char('c') if !is_filter_focused => {
                let dialog = &app.disasm_string_ref_dialog;
                let is_64 = app.is_64();
                let text = dialog
                    .filtered_indices
                    .get(dialog.selected_index)
                    .and_then(|&i| dialog.items.get(i))
                    .map(|item| item_as_tsv(item, is_64))
                    .unwrap_or_default();
                app.copy_to_clipboard(text, "1 reference".to_string());
                return Ok(false);
            }
            KeyCode::Char('Y') | KeyCode::Char('C') if !is_filter_focused => {
                let (text, count) = filtered_rows_as_tsv(app);
                app.copy_to_clipboard(text, format!("{} reference(s)", count));
                return Ok(false);
            }
            KeyCode::PageUp if !is_filter_focused => {
                if !app.disasm_string_ref_dialog.filtered_indices.is_empty() {
                    app.disasm_string_ref_dialog.selected_index =
                        app.disasm_string_ref_dialog.selected_index.saturating_sub(10);
                }
                return Ok(false);
            }
            KeyCode::PageDown if !is_filter_focused => {
                let cnt = app.disasm_string_ref_dialog.filtered_indices.len();
                if cnt > 0 {
                    app.disasm_string_ref_dialog.selected_index =
                        (app.disasm_string_ref_dialog.selected_index + 10).min(cnt - 1);
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
                app.disasm_string_ref_dialog.reset();
                app.dialog_renderer = None;
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
            va_str_64: format!("{:016X}", va),
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
            va_str_64: format!("{:016X}", va),
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
            "0000000140001234\tlea rdx,[0x140002000]\tASCII \"hello\"\t000A1A38"
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

        let press = |app: &mut App, code: KeyCode| {
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
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Char('Y'));
        assert_eq!(app.logs.len(), 2, "logs: {:?}", app.logs);
        assert!(app.logs.iter().all(|l| l.contains("clipboard")), "logs: {:?}", app.logs);
        assert!(app.state == UIState::DialogStringRef);
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
}