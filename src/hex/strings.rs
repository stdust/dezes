use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers},
    layout::{Alignment, Constraint, Direction, Layout},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
};

use std::io::Result;

use crate::{app::App, commands::Commands, editor::UIState, util::center_widget};

use regex::{Regex, RegexBuilder};

const STRINGS_PAGE_STEP: usize = 29;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StringEncoding {
    #[default]
    Ascii,
    Utf8,
    Cp949,
    Cp936,
    Utf16,
}

impl StringEncoding {
    pub fn codec(&self) -> &'static encoding_rs::Encoding {
        match self {
            Self::Ascii => encoding_rs::UTF_8,
            Self::Utf8 => encoding_rs::UTF_8,
            Self::Cp949 => encoding_rs::EUC_KR,
            Self::Cp936 => encoding_rs::GBK,
            Self::Utf16 => encoding_rs::UTF_16LE,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ascii => "ASCII",
            Self::Utf8 => "UTF-8",
            Self::Cp949 => "CP949(KO)",
            Self::Cp936 => "CP936(CN)",
            Self::Utf16 => "UTF-16LE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Ascii => Self::Utf8,
            Self::Utf8 => Self::Cp949,
            Self::Cp949 => Self::Cp936,
            Self::Cp936 => Self::Utf16,
            Self::Utf16 => Self::Ascii,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Ascii => Self::Utf16,
            Self::Utf8 => Self::Ascii,
            Self::Cp949 => Self::Utf8,
            Self::Cp936 => Self::Cp949,
            Self::Utf16 => Self::Cp936,
        }
    }
}

pub struct FoundString {
    pub offset: usize,
    pub size: usize,
    pub content: String,
    pub display: String,
}

impl FoundString {
    pub fn new(offset: usize, content: &str, size: usize) -> Self {
        FoundString {
            offset,
            size,
            content: content.to_string(),
            display: format!("{offset:08X}  {content}"),
        }
    }

    pub fn set_address(&mut self, addr: u64) {
        self.display = format!("{addr:08X}  {}", self.content);
    }
}

pub fn dialog_strings_draw(app: &mut App, frame: &mut Frame) {
    let dialog_style = app.config.theme.dialog;

    let width = (frame.area().width * 3 / 4).max(78).min(frame.area().width);
    let height = (frame.area().height * 3 / 4).max(20).min(frame.area().height);
    let dialog_area = center_widget(width, height, frame.area());

    let strings_count = if app.strings.len() == app.config.maximum_strings_to_show {
        format!("{}+", app.config.maximum_strings_to_show)
    } else {
        format!("{}", app.strings.len())
    };
    let shown = app.hex_view.strings_filtered.len();

    let title_bottom = format!(
        " {} = {} (+/-) ",
        crate::i18n::M::MinimumLength.tr(app.config.lang),
        app.config.minimum_string_length
    );

    let outer = Block::bordered()
        .title(format!(
            " {} ({} / {}) ",
            crate::i18n::M::StringsTitle.tr(app.config.lang),
            shown,
            strings_count
        ))
        .title_bottom(title_bottom)
        .title_alignment(Alignment::Center)
        .style(dialog_style);

    let inner = outer.inner(dialog_area);

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(outer, dialog_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);

    let visible = chunks[0].height as usize;
    let total = app.hex_view.strings_filtered.len();
    let selected = app.list_state.selected().unwrap_or(0).min(total.saturating_sub(1));

    let start = if total <= visible {
        0
    } else {
        let half = visible / 2;
        if selected > half {
            (selected - half).min(total - visible)
        } else {
            0
        }
    };
    let end = (start + visible).min(total);

    let items: Vec<ListItem> = app.hex_view.strings_filtered[start..end]
        .iter()
        .filter_map(|&i| app.strings.get(i))
        .map(|i| ListItem::from(i.display.as_str()))
        .collect();

    let list = List::new(items)
        .style(dialog_style)
        .block(Block::new().padding(Padding::horizontal(1)))
        .highlight_style(app.config.theme.highlight)
        .repeat_highlight_symbol(true);

    let mut window_state = ratatui::widgets::ListState::default();
    if total > 0 {
        window_state.select(Some(selected - start));
    }
    frame.render_stateful_widget(list, chunks[0], &mut window_state);

    if total == 0 && matches_the_empty_string(app.hex_view.strings_regex_input.value()) {
        let notice = ratatui::widgets::Paragraph::new(
            crate::i18n::M::WarnRegexEmptyOnly.tr(app.config.lang),
        )
        .style(app.config.theme.error)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .block(Block::new().padding(Padding::uniform(1)));
        frame.render_widget(notice, chunks[0]);
    }

    app.hex_view.strings_page_rows = visible;

    let focus = app.hex_view.strings_focus_filter;
    let border_style = if focus {
        app.config.theme.highlight
    } else {
        dialog_style
    };

    let filter_title = format!(
        "{}| {}: [ {} ] (F2) ",
        crate::i18n::M::FilterRegexTitle.tr(app.config.lang),
        crate::i18n::M::Encoding.tr(app.config.lang),
        app.hex_view.strings_encoding.as_str()
    );

    let filter_block = Block::default()
        .title(filter_title)
        .title_bottom(crate::i18n::M::StringsFooterKeys.tr(app.config.lang))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(dialog_style)
        .padding(Padding::horizontal(1));

    let para = Paragraph::new(crate::text_field::render_line(
        &app.hex_view.strings_regex_input,
        app.hex_view.strings_filter_anchor,
        dialog_style,
        app.config.theme.highlight,
    ))
    .style(dialog_style)
    .block(filter_block);
    frame.render_widget(para, chunks[1]);

    if focus {
        let x = chunks[1].x + 2 + app.hex_view.strings_regex_input.visual_cursor() as u16;
        frame.set_cursor_position((x, chunks[1].y + 1));
    }
}

pub const MAX_REGEX_PATTERN_LEN: usize = 500;
pub const MAX_REGEX_SIZE_LIMIT: usize = 1024 * 1024; // 1 MiB
pub const MAX_REGEX_NEST_LIMIT: u32 = 50;

pub fn build_safe_regex(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() || pattern.len() > MAX_REGEX_PATTERN_LEN {
        return None;
    }
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(MAX_REGEX_SIZE_LIMIT)
        .nest_limit(MAX_REGEX_NEST_LIMIT)
        .build()
        .ok()
}

pub fn matches_the_empty_string(pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    build_safe_regex(pattern)
        .map(|re| re.is_match(""))
        .unwrap_or(false)
}

pub fn update_strings_filter(app: &mut App) {
    let pattern = app.hex_view.strings_regex_input.value().trim().to_string();

    let compiled = if pattern.is_empty() {
        None
    } else {
        build_safe_regex(&pattern)
    };
    let lower = pattern.to_lowercase();

    let mut filtered = Vec::with_capacity(app.strings.len());
    for (idx, s) in app.strings.iter().enumerate() {
        let keep = if pattern.is_empty() {
            true
        } else if let Some(re) = &compiled {
            crate::util::has_nonempty_match(re, &s.content)
        } else {
            s.content.to_lowercase().contains(&lower)
        };
        if keep {
            filtered.push(idx);
        }
    }

    app.hex_view.strings_filtered = filtered;

    let len = app.hex_view.strings_filtered.len();
    match app.list_state.selected() {
        _ if len == 0 => app.list_state.select(None),
        Some(n) if n >= len => app.list_state.select(Some(len - 1)),
        None => app.list_state.select(Some(0)),
        _ => {}
    }
}

pub fn dialog_strings_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Mouse(mouse) = event {
        if let ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) = mouse.kind {
            let now = std::time::Instant::now();
            let is_double_click = if let Some((last_time, last_row, last_col)) = app.last_left_click {
                now.duration_since(last_time).as_millis() < 400 && last_row == mouse.row && last_col == mouse.column
            } else {
                false
            };

            if is_double_click {
                app.last_left_click = None;
                open_string_edit(app);
            } else {
                let total = app.hex_view.strings_filtered.len();
                if total > 0 {
                    let height = 20u16.min(app.screen.height);
                    let dialog_y = app.screen.y + (app.screen.height.saturating_sub(height)) / 2;
                    let list_y = dialog_y + 1;
                    let visible = (height.saturating_sub(5)) as usize;
                    let selected = app.list_state.selected().unwrap_or(0).min(total.saturating_sub(1));

                    let start = if total <= visible {
                        0
                    } else {
                        let half = visible / 2;
                        if selected > half {
                            (selected - half).min(total - visible)
                        } else {
                            0
                        }
                    };

                    if mouse.row >= list_y && (mouse.row - list_y) < visible as u16 {
                        let clicked_row = (mouse.row - list_y) as usize;
                        let clicked_index = start + clicked_row;
                        if clicked_index < total {
                            app.list_state.select(Some(clicked_index));
                        }
                    }
                }
                app.last_left_click = Some((now, mouse.row, mouse.column));
            }
        }
        match mouse.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                move_selection(app, -3);
            }
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                move_selection(app, 3);
            }
            _ => {}
        }
        return Ok(false);
    }

    let Event::Key(key) = event else { return Ok(false) };
    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    let focus = app.hex_view.strings_focus_filter;

    match key.code {
        KeyCode::Esc => {
            if focus {
                app.hex_view.strings_focus_filter = false;
            } else {
                app.hex_view.strings_focus_filter = false;
                app.dialog_renderer = None;
                app.dialog_2nd_renderer = None;
                app.state = UIState::Normal;
            }
        }
        KeyCode::Tab => {
            app.hex_view.strings_focus_filter = !focus;
        }
        KeyCode::BackTab => {
            app.hex_view.strings_focus_filter = !focus;
        }
        KeyCode::F(2) => {
            app.hex_view.strings_encoding = if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.hex_view.strings_encoding.prev()
            } else {
                app.hex_view.strings_encoding.next()
            };
            Commands::rescan_strings(app);
        }
        KeyCode::Down => move_selection(app, 1),
        KeyCode::Up => move_selection(app, -1),
        KeyCode::PageDown => move_selection(app, page_step(app)),
        KeyCode::PageUp => move_selection(app, -page_step(app)),
        KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End if focus => {
            crate::text_field::handle_key(app, strings_filter_field, event);
        }
        KeyCode::Home if !focus => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                move_selection(app, isize::MIN / 2);
            } else {
                move_selection(app, -page_step(app));
            }
        }
        KeyCode::End if !focus => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                move_selection(app, isize::MAX / 2);
            } else {
                move_selection(app, page_step(app));
            }
        }
        KeyCode::Enter if focus => {
            app.hex_view.strings_focus_filter = false;
        }
        KeyCode::Enter => {
            if let Some(choice) = app.list_state.selected() {
                let Some(&real) = app.hex_view.strings_filtered.get(choice) else {
                    App::log(
                        app,
                        format!(
                            "selection {} is out of range (0..{})",
                            choice,
                            app.hex_view.strings_filtered.len()
                        ),
                    );
                    return Ok(true);
                };
                let Some(found) = app.strings.get(real) else {
                    App::log(
                        app,
                        format!("string {} is out of range (0..{})", real, app.strings.len()),
                    );
                    return Ok(true);
                };
                let offset = found.offset;
                let size = found.size;
                app.last_result = Some(UIState::DialogStrings);
                app.goto(offset);
                app.hex_view.selection.start = offset;
                app.hex_view.selection.end = (offset + size.saturating_sub(1))
                    .min(app.file_info.size.saturating_sub(1))
                    .max(offset);
                app.hex_view.selection.direction = None;
                app.hex_view.selection.is_mouse = false;
                app.hex_view.selection_target = crate::editor::EditingTarget::Hex;
                app.hex_view.strings_focus_filter = false;
                app.state = UIState::Normal;
                app.dialog_renderer = None;
                app.dialog_2nd_renderer = None;
            }
        }
        KeyCode::Char('+') if !focus => {
            app.config.minimum_string_length += 1;
            Commands::rescan_strings(app);
        }
        KeyCode::Char('-') if !focus && app.config.minimum_string_length > 1 => {
            app.config.minimum_string_length -= 1;
            Commands::rescan_strings(app);
        }
        KeyCode::Char('R') if !focus => {
            Commands::rescan_strings(app);
        }
        KeyCode::Char('f') | KeyCode::Char('/') if !focus => {
            app.hex_view.strings_focus_filter = true;
        }
        KeyCode::F(4) => open_string_edit(app),
        KeyCode::F(5) => {
            app.hex_view.strings_focus_filter = false;
            app.dialog_2nd_renderer = None;

            let selected_string_offset = app
                .list_state
                .selected()
                .and_then(|i| app.hex_view.strings_filtered.get(i).copied())
                .and_then(|i| app.strings.get(i))
                .map(|s| s.offset);

            if app.disasm_string_ref_dialog.items.is_empty() {
                let items = crate::disasm::string_ref::scan_string_references(app);
                app.disasm_string_ref_dialog.items = items;
                app.disasm_string_ref_dialog.filter_input = tui_input::Input::default();
                app.disasm_string_ref_dialog.focus_filter = false;
                app.disasm_string_ref_dialog.selected_index = 0;
                app.disasm_string_ref_dialog.update_filter();
            }
            app.state = UIState::DialogStringRef;
            app.dialog_renderer = Some(|app, frame| {
                crate::disasm::string_ref_dialog::draw_string_ref_dialog(app, frame, app.screen)
            });

            if let Some(target_ofs) = selected_string_offset {
                let dialog = &app.disasm_string_ref_dialog;
                if let Some(pos) = dialog.filtered_indices.iter().position(|&idx| {
                    dialog.items.get(idx).map(|item| item.string_offset) == Some(target_ofs)
                }) {
                    app.disasm_string_ref_dialog.selected_index = pos;
                }
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            let (text, count) = filtered_rows_as_tsv(app);
            app.copy_to_clipboard(text, format!("{} string(s)", count));
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if focus {
                let filter_val = app.hex_view.strings_regex_input.value().to_string();
                app.copy_to_clipboard(filter_val, "filter text".to_string());
            } else {
                let text = app
                    .list_state
                    .selected()
                    .and_then(|i| app.hex_view.strings_filtered.get(i).copied())
                    .and_then(|i| app.strings.get(i))
                    .map(|s| row_as_tsv(app, s))
                    .unwrap_or_default();
                app.copy_to_clipboard(text, "1 string".to_string());
            }
        }
        _ => {
            if focus && crate::text_field::handle_key(app, strings_filter_field, event) {
                update_strings_filter(app);
            }
        }
    }
    Ok(false)
}

fn strings_filter_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (
        &mut app.hex_view.strings_regex_input,
        &mut app.hex_view.strings_filter_anchor,
    )
}

#[derive(Default)]
pub struct StringEdit {
    pub offset: usize,
    pub budget: usize,
    pub total_capacity: usize,
    pub row: usize,
    pub encoding: StringEncoding,
    pub input: tui_input::Input,
    pub anchor: Option<usize>,
    pub error: Option<String>,
    pub return_state: Option<UIState>,
}

fn string_edit_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (
        &mut app.hex_view.string_edit.input,
        &mut app.hex_view.string_edit.anchor,
    )
}

pub fn count_trailing_zeros(buffer: &[u8], offset: usize, is_utf16: bool) -> usize {
    let mut count = 0;
    let max_extra = 512;
    while offset + count < buffer.len() && buffer[offset + count] == 0 && count < max_extra {
        count += 1;
    }
    if is_utf16 {
        count & !1
    } else {
        count
    }
}

pub fn open_string_edit(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoStringEdit);
        return;
    }

    let Some(row) = app
        .list_state
        .selected()
        .and_then(|i| app.hex_view.strings_filtered.get(i).copied())
    else {
        crate::beep!();
        return;
    };
    let Some(found) = app.strings.get(row) else {
        crate::beep!();
        return;
    };

    let initial_encoding = if app.hex_view.strings_encoding == StringEncoding::Ascii {
        StringEncoding::Cp949
    } else {
        app.hex_view.strings_encoding
    };

    let is_utf16 = initial_encoding == StringEncoding::Utf16;
    let term_size = if is_utf16 { 2 } else { 1 };
    let extra_zeros = {
        let buffer = app.file_info.get_buffer_ref();
        count_trailing_zeros(buffer, found.offset + found.size, is_utf16)
    };
    let total_capacity = found.size + extra_zeros;
    let budget = total_capacity.saturating_sub(term_size);

    let cursor = found.content.chars().count();
    app.hex_view.string_edit = StringEdit {
        offset: found.offset,
        budget,
        total_capacity,
        row,
        encoding: initial_encoding,
        input: tui_input::Input::new(found.content.clone()).with_cursor(cursor),
        anchor: Some(0),
        error: None,
        return_state: Some(UIState::DialogStrings),
    };
    app.state = UIState::DialogStringEdit;
    app.dialog_2nd_renderer = Some(dialog_string_edit_draw);
}

fn commit_string_edit(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoStringEdit);
        return;
    }

    let text = app.hex_view.string_edit.input.value().to_string();
    let budget = app.hex_view.string_edit.budget;
    let total_capacity = app.hex_view.string_edit.total_capacity.max(budget);
    let offset = app.hex_view.string_edit.offset;
    let encoding = app.hex_view.string_edit.encoding;

    if encoding == StringEncoding::Ascii && !text.is_ascii() {
        let message = crate::i18n::M::ErrAsciiOnly.tr(app.config.lang).to_string();
        app.hex_view.string_edit.error = Some(message);
        crate::beep!();
        return;
    }

    let bytes = crate::util::encode_text(&text, encoding.codec());
    if bytes.len() > budget {
        let message = crate::i18n::fill(
            crate::i18n::M::ErrStringTooLong.tr(app.config.lang),
            &[&bytes.len().to_string(), &budget.to_string()],
        );
        app.hex_view.string_edit.error = Some(message);
        crate::beep!();
        return;
    }

    for (i, byte) in bytes.iter().enumerate() {
        if let Some(target_ofs) = offset.checked_add(i) {
            crate::hex::edit::record_edit(app, target_ofs, *byte);
        }
    }
    let padding = total_capacity.saturating_sub(bytes.len());
    for i in bytes.len()..total_capacity {
        if let Some(target_ofs) = offset.checked_add(i) {
            crate::hex::edit::record_edit(app, target_ofs, 0);
        }
    }

    let row = app.hex_view.string_edit.row;
    let use_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if use_va { app.get_va(offset) } else { offset as u64 };
    if let Some(found) = app.strings.get_mut(row) {
        found.content = text.clone();
        found.set_address(addr);
    }

    let message = crate::i18n::fill(
        crate::i18n::M::StringReplaced.tr(app.config.lang),
        &[
            &format!("0x{:X}", offset),
            &bytes.len().to_string(),
            &padding.to_string(),
        ],
    );
    App::log(app, message);

    for item in &mut app.disasm_string_ref_dialog.items {
        if item.string_offset == offset {
            item.string_text = text.clone();
            item.full_text_str = format!("\"{}\"", text);
        }
    }

    let return_state = app.hex_view.string_edit.return_state.unwrap_or(UIState::DialogStrings);
    app.hex_view.string_edit = StringEdit::default();
    app.dialog_2nd_renderer = None;
    app.state = return_state;
}

pub fn dialog_string_edit_draw(app: &mut App, frame: &mut Frame) {
    let edit = &app.hex_view.string_edit;
    let width = 72.min(frame.area().width.saturating_sub(4)).max(28);
    let error_rows = match &edit.error {
        None => 0,
        Some(error) => {
            use unicode_width::UnicodeWidthStr;
            let inner = width.saturating_sub(2).max(1) as usize;
            error.width().div_ceil(inner).clamp(1, 4) as u16
        }
    };
    let height = 3 + error_rows;
    let area = crate::hex::field_box::centered_rect_above(width, height, frame.area());

    let title = crate::i18n::fill(
        crate::i18n::M::StringEditTitle.tr(app.config.lang),
        &[
            &format!("0x{:X}", edit.offset),
            &edit.budget.to_string(),
            edit.encoding.as_str(),
        ],
    );

    let title_bottom = crate::i18n::M::StringEditFooterKeys.tr(app.config.lang);

    let block = Block::bordered()
        .title(title)
        .title_alignment(Alignment::Center)
        .title_bottom(title_bottom)
        .title_alignment(Alignment::Center)
        .style(app.config.theme.dialog);

    let value = crate::text_field::render_line(
        &edit.input,
        edit.anchor,
        app.config.theme.dialog,
        app.config.theme.highlight,
    );
    let mut lines = vec![value];
    if let Some(error) = &edit.error {
        lines.push(ratatui::text::Line::styled(
            error.clone(),
            app.config.theme.error,
        ));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .style(app.config.theme.dialog)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(block),
        area,
    );

    let cursor_x = area.x + 1 + edit.input.visual_cursor() as u16;
    if cursor_x < area.x + area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, area.y + 1));
    }
}

pub fn dialog_string_edit_events(app: &mut App, event: &Event) -> Result<bool> {
    let Event::Key(key) = event else { return Ok(false) };
    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            let return_state = app.hex_view.string_edit.return_state.unwrap_or(UIState::DialogStrings);
            app.hex_view.string_edit = StringEdit::default();
            app.dialog_2nd_renderer = None;
            app.state = return_state;
        }
        KeyCode::Enter => commit_string_edit(app),
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.hex_view.string_edit.encoding = app.hex_view.string_edit.encoding.next();
            app.hex_view.string_edit.error = None;
        }
        _ => {
            if crate::text_field::handle_key(app, string_edit_field, event) {
                app.hex_view.string_edit.error = None;
            }
        }
    }
    Ok(false)
}

fn row_as_tsv(app: &App, s: &FoundString) -> String {
    let use_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if use_va { app.get_va(s.offset) } else { s.offset as u64 };
    format!("{:08X}\t{}", addr, s.content)
}

fn filtered_rows_as_tsv(app: &App) -> (String, usize) {
    let rows: Vec<String> = app
        .hex_view
        .strings_filtered
        .iter()
        .filter_map(|&i| app.strings.get(i))
        .map(|s| row_as_tsv(app, s))
        .collect();
    (rows.join("\r\n"), rows.len())
}

fn page_step(app: &App) -> isize {
    let rows = app.hex_view.strings_page_rows;
    if rows < 2 {
        STRINGS_PAGE_STEP as isize
    } else {
        (rows - 1) as isize
    }
}

fn move_selection(app: &mut App, delta: isize) {
    let len = app.hex_view.strings_filtered.len();
    if len == 0 {
        app.list_state.select(None);
        return;
    }
    let current = app.list_state.selected().unwrap_or(0) as isize;
    let next = current.saturating_add(delta).clamp(0, len as isize - 1);
    app.list_state.select(Some(next as usize));
}

impl Commands {
    pub fn strings(app: &mut App) {
        Commands::load_strings(app, false);
        Commands::refresh_string_addresses(app);
        app.hex_view.strings_focus_filter = false;
        update_strings_filter(app);
        app.state = UIState::DialogStrings;
        app.dialog_renderer = Some(dialog_strings_draw);
        if !app.hex_view.strings_filtered.is_empty() {
            app.list_state.select(Some(0));
        }
    }

    pub fn rescan_strings(app: &mut App) {
        Commands::load_strings(app, true);
        Commands::refresh_string_addresses(app);
        update_strings_filter(app);
        App::log(
            app,
            format!(
                "Strings: {} found (min {} chars, {})",
                app.strings.len(),
                app.config.minimum_string_length,
                app.hex_view.strings_encoding.as_str()
            ),
        );
        if app.strings.is_empty() && matches_the_empty_string(&app.string_regex) {
            let message = crate::i18n::M::WarnRegexEmptyOnly.tr(app.config.lang).to_string();
            app.status_error = Some(message.clone());
            App::log(app, message);
        }
    }

    pub fn refresh_string_addresses(app: &mut App) {
        let use_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;

        let mut strings = std::mem::take(&mut app.strings);
        for s in strings.iter_mut() {
            let addr = if use_va { app.get_va(s.offset) } else { s.offset as u64 };
            s.set_address(addr);
        }
        app.strings = strings;
    }

    pub fn load_strings(app: &mut App, force_read: bool) {
        if force_read {
            app.strings.clear();
        }

        if !app.strings.is_empty() {
            return;
        }

        let re = if app.string_regex.trim().is_empty() {
            None
        } else {
            build_safe_regex(&app.string_regex)
        };
        let re = re.as_ref();

        let min = app.config.minimum_string_length;
        let cap = app.config.maximum_strings_to_show;
        let encoding = app.hex_view.strings_encoding;

        let buf_len = app.file_info.get_buffer_ref().len();
        let data = if encoding == StringEncoding::Ascii {
            Vec::new()
        } else {
            crate::disasm::sections::xref_sections(app, buf_len)
        };

        let buffer = app.file_info.get_buffer();
        let mut out = Vec::new();

        if encoding == StringEncoding::Ascii {
            scan_ascii(buffer, min, cap, re, &mut out);
        } else if data.is_empty() {
            let range = 0..buf_len;
            match encoding {
                StringEncoding::Ascii => unreachable!(),
                StringEncoding::Utf8 => scan_utf8(buffer, range, min, cap, re, &mut out),
                StringEncoding::Cp949 => scan_dbcs(buffer, range, &CP949, min, cap, re, &mut out),
                StringEncoding::Cp936 => scan_dbcs(buffer, range, &CP936, min, cap, re, &mut out),
                StringEncoding::Utf16 => scan_utf16(buffer, range, min, cap, re, &mut out),
            }
        } else {
            for s in &data {
                let range = s.start..s.end;
                match encoding {
                    StringEncoding::Ascii => unreachable!(),
                    StringEncoding::Utf8 => scan_utf8(buffer, range, min, cap, re, &mut out),
                    StringEncoding::Cp949 => scan_dbcs(buffer, range, &CP949, min, cap, re, &mut out),
                    StringEncoding::Cp936 => scan_dbcs(buffer, range, &CP936, min, cap, re, &mut out),
                    StringEncoding::Utf16 => scan_utf16(buffer, range, min, cap, re, &mut out),
                }
                if out.len() >= cap {
                    break;
                }
            }
        }

        app.strings = out;
    }
}

fn is_hangul(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

fn accepts(re: Option<&Regex>, text: &str) -> bool {
    match re {
        None => true,
        Some(re) => crate::util::has_nonempty_match(re, text),
    }
}

fn is_ascii_text(b: u8) -> bool {
    b.is_ascii_graphic() || b == b' '
}

struct Dbcs {
    enc: &'static encoding_rs::Encoding,
    lead_ok: fn(u8) -> bool,
    trail_ok: fn(u8) -> bool,
    is_target: fn(char) -> bool,
}

fn gbk_lead(b: u8) -> bool {
    (0xA1..=0xA9).contains(&b) || (0xB0..=0xF7).contains(&b)
}

fn gbk_trail(b: u8) -> bool {
    (0xA1..=0xFE).contains(&b)
}

fn euckr_lead(b: u8) -> bool {
    (0xA1..=0xA2).contains(&b) || (0xB0..=0xC8).contains(&b)
}

fn euckr_trail(b: u8) -> bool {
    (0xA1..=0xFE).contains(&b)
}

const MIN_SCRIPT_RUN: usize = 2;

fn longest_script_run(text: &str, is_target: fn(char) -> bool) -> usize {
    let mut best = 0usize;
    let mut current = 0usize;
    for c in text.chars() {
        if is_target(c) {
            current += 1;
            best = best.max(current);
        } else {
            current = 0;
        }
    }
    best
}

fn is_one_char_repeated(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return false };
    text.chars().count() >= 3 && chars.all(|c| c == first)
}

fn scan_dbcs(
    buffer: &[u8],
    range: std::ops::Range<usize>,
    cp: &Dbcs,
    min: usize,
    cap: usize,
    re: Option<&Regex>,
    out: &mut Vec<FoundString>,
) {
    let len = range.end.min(buffer.len());
    let mut i = range.start.min(len);

    while i < len {
        let start = i;

        let mut end = i;
        while end < len {
            let b = buffer[end];
            if b < 0x80 {
                if is_ascii_text(b) {
                    end += 1;
                } else {
                    break;
                }
            } else if (cp.lead_ok)(b) && end + 1 < len && (cp.trail_ok)(buffer[end + 1]) {
                end += 2;
            } else {
                break;
            }
        }

        if end == start {
            i += 1;
            continue;
        }

        let run = &buffer[start..end];
        let terminated = end == buffer.len() || buffer[end] == 0;
        let (cow, had_errors) = cp.enc.decode_without_bom_handling(run);
        if !had_errors && terminated {
            let text = cow.as_ref();
            if text.chars().count() >= min
                && longest_script_run(text, cp.is_target) >= MIN_SCRIPT_RUN
                && !is_one_char_repeated(text)
                && accepts(re, text)
            {
                out.push(FoundString::new(start, text, run.len()));
                if out.len() >= cap {
                    return;
                }
            }
        }

        i = end;
    }
}

fn scan_ascii(buffer: &[u8], min: usize, cap: usize, re: Option<&Regex>, out: &mut Vec<FoundString>) {
    const MAX_CANDIDATE_LEN: usize = 4096;
    let mut siz = 0usize;
    let mut candidate = String::new();

    for (offset, byte) in buffer.iter().enumerate() {
        if is_ascii_text(*byte) {
            candidate.push(*byte as char);
            siz += 1;
            if siz >= MAX_CANDIDATE_LEN {
                if siz >= min && accepts(re, &candidate) {
                    out.push(FoundString::new(offset.saturating_add(1).saturating_sub(siz), &candidate, siz));
                    if out.len() >= cap {
                        return;
                    }
                }
                candidate.clear();
                siz = 0;
            }
        } else {
            if siz >= min && accepts(re, &candidate) {
                out.push(FoundString::new(offset.saturating_sub(siz), &candidate, siz));
                if out.len() >= cap {
                    return;
                }
            }
            candidate.clear();
            siz = 0;
        }
    }

    if siz >= min && accepts(re, &candidate) && out.len() < cap {
        out.push(FoundString::new(buffer.len().saturating_sub(siz), &candidate, siz));
    }
}

fn scan_utf8(
    buffer: &[u8],
    range: std::ops::Range<usize>,
    min: usize,
    cap: usize,
    re: Option<&Regex>,
    out: &mut Vec<FoundString>,
) {
    let len = range.end.min(buffer.len());
    let mut i = range.start.min(len);

    while i < len {
        let start = i;
        let mut end = i;

        while end < len {
            let b = buffer[end];
            if b < 0x80 {
                if is_ascii_text(b) {
                    end += 1;
                } else {
                    break;
                }
            } else {
                let expected_len = match b {
                    0xC2..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    0xF0..=0xF4 => 4,
                    _ => 0,
                };
                if expected_len > 0
                    && end + expected_len <= len
                    && let Ok(s) = std::str::from_utf8(&buffer[end..end + expected_len])
                    && let Some(c) = s.chars().next()
                    && !c.is_control()
                {
                    end += expected_len;
                    continue;
                }
                break;
            }
        }

        if end == start {
            i += 1;
            continue;
        }

        let run = &buffer[start..end];
        let terminated = end == buffer.len() || buffer[end] == 0;
        if terminated
            && let Ok(text) = std::str::from_utf8(run)
            && text.chars().count() >= min
            && accepts(re, text)
        {
            out.push(FoundString::new(start, text, run.len()));
            if out.len() >= cap {
                return;
            }
        }

        i = end.max(start + 1);
    }
}

const CP949: Dbcs = Dbcs {
    enc: encoding_rs::EUC_KR,
    lead_ok: euckr_lead,
    trail_ok: euckr_trail,
    is_target: is_hangul,
};

const CP936: Dbcs = Dbcs {
    enc: encoding_rs::GBK,
    lead_ok: gbk_lead,
    trail_ok: gbk_trail,
    is_target: is_cjk,
};

fn is_wide_text(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    matches!(c as u32,
        0x20..=0x7E
        | 0xA0..=0x24F
        | 0x370..=0x3FF
        | 0x400..=0x4FF
        | 0x2010..=0x203A
        | 0x20A0..=0x20BF
        | 0x3000..=0x30FF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7A3
        | 0xFF01..=0xFF60
    )
}

fn looks_like_single_byte_text(text: &str) -> bool {
    let mut wide = 0usize;
    let mut ascii_pairs = 0usize;
    for c in text.chars() {
        let u = c as u32;
        if u < 0x80 {
            continue;
        }
        wide += 1;
        let hi = ((u >> 8) & 0xFF) as u8;
        let lo = (u & 0xFF) as u8;
        if (0x20..=0x7E).contains(&hi) && (0x20..=0x7E).contains(&lo) {
            ascii_pairs += 1;
        }
    }
    wide >= 3 && ascii_pairs == wide
}

fn scan_utf16(
    buffer: &[u8],
    range: std::ops::Range<usize>,
    min: usize,
    cap: usize,
    re: Option<&Regex>,
    out: &mut Vec<FoundString>,
) {
    let len = range.end.min(buffer.len());
    let mut i = (range.start + (range.start & 1)).min(len);
    let mut text = String::new();

    while i + 1 < len {
        let start = i;
        text.clear();

        while i + 1 < len {
            let unit = u16::from_le_bytes([buffer[i], buffer[i + 1]]);
            match char::from_u32(unit as u32) {
                Some(c) if is_wide_text(c) => {
                    text.push(c);
                    i += 2;
                }
                _ => break,
            }
        }

        if i == start {
            i += 2;
            continue;
        }

        let terminated =
            i + 1 >= buffer.len() || u16::from_le_bytes([buffer[i], buffer[i + 1]]) == 0;
        let preceded = start < 2 || u16::from_le_bytes([buffer[start - 2], buffer[start - 1]]) == 0;

        if terminated
            && preceded
            && text.chars().count() >= min
            && !is_one_char_repeated(&text)
            && !looks_like_single_byte_text(&text)
            && accepts(re, &text)
        {
            out.push(FoundString::new(start, &text, i - start));
            if out.len() >= cap {
                return;
            }
        }

        i += 2;
    }
}

#[cfg(test)]
mod strings_scan_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn any() -> Regex {
        Regex::new(".*").unwrap()
    }

    fn euckr(text: &str) -> Vec<u8> {
        encoding_rs::EUC_KR.encode(text).0.into_owned()
    }

    #[test]
    fn cp949_scan_keeps_korean_and_drops_plain_ascii() {
        let mut buffer = vec![0u8; 0x60];
        buffer[0x08..0x0F].copy_from_slice(b"license");
        let korean = euckr("한글문자");
        buffer[0x20..0x20 + korean.len()].copy_from_slice(&korean);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP949, 3, 100, Some(&any()), &mut out);

        assert_eq!(out.len(), 1, "found {:?}", out.iter().map(|s| &s.content).collect::<Vec<_>>());
        assert_eq!(out[0].offset, 0x20);
        assert_eq!(out[0].content, "한글문자");
        assert_eq!(out[0].size, korean.len(), "size is the byte length, not the char count");
    }

    #[test]
    fn minimum_length_counts_characters_not_bytes() {
        let mut buffer = vec![0u8; 0x40];
        let korean = euckr("한글");
        buffer[0x10..0x10 + korean.len()].copy_from_slice(&korean);
        assert_eq!(korean.len(), 4);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP949, 3, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "2 characters must not pass a minimum of 3");

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP949, 2, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "2 characters must pass a minimum of 2");
    }

    #[test]
    fn utf16_scan_finds_what_the_ascii_scan_cannot() {
        let mut buffer = vec![0u8; 0x40];
        let wide: Vec<u8> = "Hello".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        buffer[0x10..0x10 + wide.len()].copy_from_slice(&wide);

        let mut ascii = Vec::new();
        scan_ascii(&buffer, 4, 100, Some(&any()), &mut ascii);
        assert!(ascii.is_empty(), "single-byte scan should see only 1-char runs here");

        let mut out = Vec::new();
        scan_utf16(&buffer, 0..buffer.len(), 4, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "found {:?}", out.iter().map(|s| &s.content).collect::<Vec<_>>());
        assert_eq!(out[0].offset, 0x10);
        assert_eq!(out[0].content, "Hello");
        assert_eq!(out[0].size, 10);
    }

    #[test]
    fn scan_stops_at_the_cap() {
        let mut buffer = Vec::new();
        for _ in 0..50 {
            buffer.extend_from_slice(b"abcdef\0");
        }
        let mut out = Vec::new();
        scan_ascii(&buffer, 4, 7, Some(&any()), &mut out);
        assert_eq!(out.len(), 7);
    }

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("dz6_strings_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = dialog_strings_events(app, &Event::Key(key));
    }

    #[test]
    fn enter_maps_the_selection_through_the_filter() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        bytes[0x20..0x27].copy_from_slice(b"WORLD!!");
        let (dir, mut app) = app_with(&bytes, "filter_map");

        Commands::strings(&mut app);
        assert_eq!(app.strings.len(), 2, "both strings should be found");

        app.hex_view.strings_regex_input = tui_input::Input::new("world".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered, vec![1]);

        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Enter);

        let offset = app.hex_view.offset;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(offset, 0x20, "Enter must jump to the filtered row, not to strings[0]");
    }

    #[test]
    fn f2_switches_the_encoding_and_rescans() {
        let mut bytes = vec![0u8; 0x60];
        bytes[0x08..0x0F].copy_from_slice(b"license");
        let korean = encoding_rs::EUC_KR.encode("한글문자").0.into_owned();
        bytes[0x20..0x20 + korean.len()].copy_from_slice(&korean);
        let (dir, mut app) = app_with(&bytes, "f2_encoding");

        Commands::strings(&mut app);
        assert_eq!(app.hex_view.strings_encoding, StringEncoding::Ascii);
        assert!(app.strings.iter().any(|s| s.content == "license"));

        press(&mut app, KeyCode::F(2));
        assert_eq!(app.hex_view.strings_encoding, StringEncoding::Utf8);

        press(&mut app, KeyCode::F(2));

        let encoding = app.hex_view.strings_encoding;
        let contents: Vec<String> = app.strings.iter().map(|s| s.content.clone()).collect();
        let filtered = app.hex_view.strings_filtered.len();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(encoding, StringEncoding::Cp949);
        assert_eq!(contents, vec!["한글문자".to_string()], "the list must be re-scanned");
        assert_eq!(filtered, 1, "the filter has to be rebuilt against the new list");
    }

    #[test]
    fn ascii_mode_refuses_non_ascii_replacements() {
        let mut app = App::new();
        app.hex_view.string_edit = StringEdit {
            offset: 0,
            budget: 10,
            total_capacity: 10,
            row: 0,
            encoding: StringEncoding::Ascii,
            input: tui_input::Input::new("한글".to_string()),
            anchor: None,
            error: None,
            return_state: None,
        };
        commit_string_edit(&mut app);
        assert!(app.hex_view.string_edit.error.is_some(), "non-ASCII replacement in ASCII mode must be refused");
    }

    #[test]
    fn ascii_scan_defaults_string_edit_to_cp949() {
        let mut app = App::new();
        app.strings = vec![FoundString::new(0x10, "license", 7)];
        app.hex_view.strings_filtered = vec![0];
        app.list_state.select(Some(0));
        app.hex_view.strings_encoding = StringEncoding::Ascii;

        open_string_edit(&mut app);

        assert!(matches!(app.state, crate::editor::UIState::DialogStringEdit));
        assert_eq!(app.hex_view.string_edit.encoding, StringEncoding::Cp949);
    }

    #[test]
    fn typing_in_the_filter_box_is_not_a_list_shortcut() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        let (dir, mut app) = app_with(&bytes, "typing");

        Commands::strings(&mut app);
        let min_before = app.config.minimum_string_length;

        press(&mut app, KeyCode::Tab);
        assert!(app.hex_view.strings_focus_filter, "Tab must move focus to the box");

        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('+'));
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Char('Y'));

        let typed = app.hex_view.strings_regex_input.value().to_string();
        let min_after = app.config.minimum_string_length;
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(typed, "f+yY");
        assert_eq!(min_after, min_before, "'+' must not change the minimum length while typing");
    }
}

#[cfg(test)]
mod dbcs_false_positive_tests {
    use super::*;

    fn any() -> Regex {
        Regex::new(".*").unwrap()
    }

    #[test]
    fn cp936_ignores_x64_code_that_decodes_cleanly() {
        let mut buffer = vec![
            0x48, 0x8B, 0xCB, 0x48, 0x8A, 0xDA, 0x48, 0x8B, 0xD3, 0x00,
        ];
        buffer.extend_from_slice(&[0xCC; 10]);
        buffer.push(0x00);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP936, 4, 100, Some(&any()), &mut out);

        assert!(
            out.is_empty(),
            "code should not be text: {:?}",
            out.iter().map(|s| &s.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn repeated_filler_is_not_a_string() {
        let mut buffer = vec![0x00];
        buffer.extend_from_slice(&[0xCC; 12]);
        buffer.push(0x00);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP936, 4, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "0xCC filler must not be reported");
    }

    #[test]
    fn dbcs_requires_a_nul_terminator() {
        let korean = encoding_rs::EUC_KR.encode("한글문자").0.into_owned();

        let mut terminated = vec![0u8; 0x20];
        terminated[0x10..0x10 + korean.len()].copy_from_slice(&korean);
        let mut out = Vec::new();
        scan_dbcs(&terminated, 0..terminated.len(), &CP949, 3, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "a NUL-terminated string is text");

        let mut unterminated = vec![0u8; 0x20];
        unterminated[0x10..0x10 + korean.len()].copy_from_slice(&korean);
        unterminated[0x10 + korean.len()] = 0x01;
        let mut out = Vec::new();
        scan_dbcs(&unterminated, 0..unterminated.len(), &CP949, 3, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "a run cut off by a control byte is not text");
    }

    #[test]
    fn a_single_stray_character_is_not_a_word() {
        let single = encoding_rs::GBK.encode("镜").0.into_owned();
        let mut buffer = vec![0u8; 0x20];
        buffer[0x08..0x0C].copy_from_slice(b"abc ");
        buffer[0x0C..0x0C + single.len()].copy_from_slice(&single);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP936, 4, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "one hanzi among ASCII is not enough");
    }

    #[test]
    fn a_real_mixed_string_survives_every_rule() {
        let text = "AutoEye(乾坤镜)";
        let encoded = encoding_rs::GBK.encode(text).0.into_owned();
        let mut buffer = vec![0u8; 0x40];
        buffer[0x10..0x10 + encoded.len()].copy_from_slice(&encoded);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP936, 4, 100, Some(&any()), &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, text);
        assert_eq!(out[0].offset, 0x10);
    }

    #[test]
    fn scan_honours_the_range_it_is_given() {
        let korean = encoding_rs::EUC_KR.encode("한글문자").0.into_owned();
        let mut buffer = vec![0u8; 0x60];
        buffer[0x10..0x10 + korean.len()].copy_from_slice(&korean);
        buffer[0x40..0x40 + korean.len()].copy_from_slice(&korean);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0x30..0x60, &CP949, 3, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "only the string inside the range counts");
        assert_eq!(out[0].offset, 0x40);
    }

    #[test]
    fn utf16_rejects_units_outside_the_text_blocks() {
        let mut buffer = vec![0u8; 0x40];
        for k in 0..5 {
            buffer[0x10 + k * 2] = 0xBA;
            buffer[0x11 + k * 2] = 0x0E;
        }

        let mut out = Vec::new();
        scan_utf16(&buffer, 0..buffer.len(), 4, 100, Some(&any()), &mut out);
        assert!(
            out.is_empty(),
            "unassigned code points are not text: {:?}",
            out.iter().map(|s| &s.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn utf16_requires_a_nul_on_both_sides() {
        let wide: Vec<u8> = "Hello".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();

        let mut clean = vec![0u8; 0x40];
        clean[0x10..0x10 + wide.len()].copy_from_slice(&wide);
        let mut out = Vec::new();
        scan_utf16(&clean, 0..clean.len(), 4, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "a padded wide literal is text");

        let mut crowded = vec![0u8; 0x40];
        crowded[0x0E] = 0x01;
        crowded[0x10..0x10 + wide.len()].copy_from_slice(&wide);
        let mut out = Vec::new();
        scan_utf16(&crowded, 0..crowded.len(), 4, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "a run that starts mid-data is not a wide literal");
    }
}
#[cfg(test)]
mod scan_reach_tests {
    use super::*;
    use crate::app::App;

    fn wide_file(count: usize, tail: &str) -> Vec<u8> {
        let mut buffer = vec![0u8, 0u8];
        for i in 0..count {
            let s = format!("string-{:05}", i);
            for unit in s.encode_utf16() {
                buffer.extend_from_slice(&unit.to_le_bytes());
            }
            buffer.extend_from_slice(&[0, 0]);
        }
        for unit in tail.encode_utf16() {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
        buffer.extend_from_slice(&[0, 0]);
        buffer
    }

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("dz6_reach_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        (dir, app)
    }

    #[test]
    fn a_string_past_the_old_cap_is_still_found() {
        let bytes = wide_file(4000, "한글 파일 이름");
        let (dir, mut app) = app_with(&bytes, "late_hangul");
        app.hex_view.strings_encoding = StringEncoding::Utf16;

        Commands::strings(&mut app);
        let scanned = app.strings.len();

        app.hex_view.strings_regex_input = tui_input::Input::new("[가-힣]{2,}".to_string());
        update_strings_filter(&mut app);
        let hits: Vec<String> = app
            .hex_view
            .strings_filtered
            .iter()
            .filter_map(|&i| app.strings.get(i))
            .map(|s| s.content.clone())
            .collect();

        let _ = std::fs::remove_dir_all(&dir);

        assert!(scanned > 4000, "the scan stopped early: only {} strings", scanned);
        assert_eq!(hits, vec!["한글 파일 이름".to_string()], "the late string was not reachable");
    }

    #[test]
    fn the_window_follows_the_selection() {
        use ratatui::{Terminal, backend::TestBackend};

        let bytes = wide_file(4000, "tail");
        let (dir, mut app) = app_with(&bytes, "window");
        app.hex_view.strings_encoding = StringEncoding::Utf16;
        Commands::strings(&mut app);

        let target = 3000;
        let wanted = app
            .strings
            .get(app.hex_view.strings_filtered[target])
            .map(|s| s.content.clone())
            .expect("row 3000");
        app.list_state.select(Some(target));

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("backend");
        terminal.draw(|f| dialog_strings_draw(&mut app, f)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let screen: String = (0..30)
            .map(|y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect();

        let rows = app.hex_view.strings_page_rows;
        let _ = std::fs::remove_dir_all(&dir);

        assert!(rows > 0, "the draw did not record the list height");
        assert!(
            screen.contains(&wanted),
            "row {} ({:?}) is not on screen",
            target,
            wanted
        );
    }

    #[test]
    fn the_selection_stays_inside_the_list() {
        let bytes = wide_file(3, "tail");
        let (dir, mut app) = app_with(&bytes, "clamp");
        app.hex_view.strings_encoding = StringEncoding::Utf16;
        Commands::strings(&mut app);

        let len = app.hex_view.strings_filtered.len();
        assert!(len >= 2, "fixture should hold a few strings, got {}", len);

        for _ in 0..len + 10 {
            super::move_selection(&mut app, 1);
        }
        assert_eq!(app.list_state.selected(), Some(len - 1), "walked past the end");

        let step = super::page_step(&app) * 5;
        super::move_selection(&mut app, step);
        assert_eq!(app.list_state.selected(), Some(len - 1));

        for _ in 0..len + 10 {
            super::move_selection(&mut app, -1);
        }
        assert_eq!(app.list_state.selected(), Some(0), "walked past the start");

        super::move_selection(&mut app, isize::MAX / 2);
        assert_eq!(app.list_state.selected(), Some(len - 1), "Ctrl+End must reach the last row");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
#[cfg(test)]
mod utf16_width_tests {
    use super::*;

    fn any() -> Regex {
        Regex::new(".*").unwrap()
    }

    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn utf16_ignores_single_byte_text_read_at_the_wrong_width() {
        let mut buffer = vec![0u8, 0u8];
        buffer.extend_from_slice(b"CollectExceptionInfo");
        buffer.extend_from_slice(&[0, 0]);

        let mut out = Vec::new();
        scan_utf16(&buffer, 0..buffer.len(), 4, 100, Some(&any()), &mut out);
        assert!(
            out.is_empty(),
            "ASCII read at the wrong width is not text: {:?}",
            out.iter().map(|s| &s.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn real_chinese_wide_strings_survive() {
        for text in ["文件属性错误", "反编译失败", "AutoEye(乾坤镜) 2.0.0.1000"] {
            let mut buffer = vec![0u8; 4];
            buffer.extend_from_slice(&wide(text));
            buffer.extend_from_slice(&[0, 0]);

            let mut out = Vec::new();
            scan_utf16(&buffer, 0..buffer.len(), 4, 100, Some(&any()), &mut out);
            assert_eq!(out.len(), 1, "{:?} was rejected", text);
            assert_eq!(out[0].content, text);
        }
    }

    #[test]
    fn korean_wide_strings_are_unaffected() {
        for text in ["파일크기", "한글 파일 이름"] {
            let mut buffer = vec![0u8; 4];
            buffer.extend_from_slice(&wide(text));
            buffer.extend_from_slice(&[0, 0]);

            let mut out = Vec::new();
            scan_utf16(&buffer, 0..buffer.len(), 4, 100, Some(&any()), &mut out);
            assert_eq!(out.len(), 1, "{:?} was rejected", text);
        }
        assert!(!looks_like_single_byte_text("파일크기"));
    }

    #[test]
    fn the_rule_needs_three_characters() {
        assert!(!looks_like_single_byte_text("版本"));
        assert!(looks_like_single_byte_text("佃敬硅"));
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("dz6_copy_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = dialog_strings_events(app, &Event::Key(key));
    }

    #[test]
    fn a_row_is_tab_separated_and_uses_the_displayed_address() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        let (dir, mut app) = app_with(&bytes, "row_format");
        Commands::strings(&mut app);

        let found = &app.strings[0];
        assert_eq!(row_as_tsv(&app, found), "00000010\tHELLO");

        app.hex_view.show_va = true;
        let va = app.get_va(found.offset);
        assert_eq!(row_as_tsv(&app, found), format!("{:08X}\tHELLO", va));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn y_copies_one_row_and_shift_y_copies_the_filtered_list() {
        let mut bytes = vec![0u8; 0x80];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        bytes[0x20..0x27].copy_from_slice(b"WORLD!!");
        bytes[0x30..0x39].copy_from_slice(b"WORLDWIDE");
        let (dir, mut app) = app_with(&bytes, "counts");
        Commands::strings(&mut app);
        assert_eq!(app.strings.len(), 3);

        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 3);
        assert_eq!(text.lines().count(), 3);
        assert!(text.starts_with("00000010\tHELLO"), "got {:?}", text);

        app.hex_view.strings_regex_input = tui_input::Input::new("^WORLD".to_string());
        update_strings_filter(&mut app);
        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 2, "Y must copy the filtered rows");
        assert!(!text.contains("HELLO"), "got {:?}", text);

        app.logs.clear();
        app.logs.clear();
        let key_c = KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let key_shift_c = KeyEvent { code: KeyCode::Char('C'), modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_strings_events(&mut app, &Event::Key(key_c));
        let _ = dialog_strings_events(&mut app, &Event::Key(key_shift_c));
        let logs = app.logs.clone();
        let state_kept = app.state == UIState::DialogStrings;

        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(logs.len(), 2, "logs: {:?}", logs);
        assert!(logs.iter().all(|l| l.contains("clipboard")), "logs: {:?}", logs);
        assert!(state_kept, "copying closed the dialog");
    }

    #[test]
    fn copying_an_empty_list_is_reported() {
        let (dir, mut app) = app_with(&[0u8; 0x40], "empty");
        Commands::strings(&mut app);
        assert!(app.strings.is_empty());

        app.logs.clear();
        let key_c = KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_strings_events(&mut app, &Event::Key(key_c));
        let last = app.logs.last().cloned().unwrap_or_default();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(last.contains("Nothing to copy"), "got {:?}", last);
    }

    #[test]
    fn f5_key_switches_to_string_ref_dialog_preserving_state() {
        let mut app = App::new();
        app.strings = vec![FoundString::new(0x10, "license", 7)];
        app.hex_view.strings_filtered = vec![0];
        app.list_state.select(Some(0));
        app.state = UIState::DialogStrings;

        let key_f5 = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
        let _ = dialog_strings_events(&mut app, &Event::Key(key_f5));

        assert!(matches!(app.state, UIState::DialogStringRef));
    }
}

#[cfg(test)]
mod filter_box_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("dz6_fbox_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_strings_events(app, &Event::Key(key));
    }

    fn sample() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x80];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        bytes[0x20..0x27].copy_from_slice(b"WORLD!!");
        bytes[0x30..0x39].copy_from_slice(b"WORLDWIDE");
        bytes
    }

    #[test]
    fn the_filter_box_owns_the_movement_keys_while_focused() {
        let (dir, mut app) = app_with(&sample(), "owns_keys");
        Commands::strings(&mut app);
        app.list_state.select(Some(1));

        app.hex_view.strings_regex_input = tui_input::Input::new("[a-z]".to_string()).with_cursor(5);
        update_strings_filter(&mut app);
        app.list_state.select(Some(1));

        press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert!(app.hex_view.strings_focus_filter);
        press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);

        let anchor = app.hex_view.strings_filter_anchor;
        let cursor = app.hex_view.strings_regex_input.cursor();
        let selected = app.list_state.selected();

        assert_eq!(anchor, Some(5), "Shift+Home has to start a block at the cursor");
        assert_eq!(cursor, 0, "and move the cursor to the front");
        assert_eq!(selected, Some(1), "the list must not have moved");

        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        let after = app.list_state.selected();
        let cursor = app.hex_view.strings_regex_input.cursor();
        let anchor = app.hex_view.strings_filter_anchor;
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(cursor, 5);
        assert_eq!(anchor, None, "a plain End drops the block");
        assert_eq!(after, Some(1));
    }

    #[test]
    fn a_block_is_replaced_by_the_next_character() {
        let (dir, mut app) = app_with(&sample(), "replace");
        Commands::strings(&mut app);
        app.hex_view.strings_focus_filter = true;
        for c in "WORLD".chars() {
            press(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(app.hex_view.strings_filtered.len(), 2, "the filter is live");

        press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);
        press(&mut app, KeyCode::Char('H'), KeyModifiers::NONE);

        let value = app.hex_view.strings_regex_input.value().to_string();
        let rows = app.hex_view.strings_filtered.len();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(value, "H");
        assert_eq!(rows, 1, "the filter has to be re-run after the replacement");
    }

    #[test]
    fn the_list_keeps_the_paging_keys_when_the_box_is_not_focused() {
        let (dir, mut app) = app_with(&sample(), "list_keys");
        Commands::strings(&mut app);
        assert!(!app.hex_view.strings_focus_filter);

        press(&mut app, KeyCode::End, KeyModifiers::CONTROL);
        let last = app.list_state.selected();
        press(&mut app, KeyCode::Home, KeyModifiers::CONTROL);
        let first = app.list_state.selected();
        let value = app.hex_view.strings_regex_input.value().to_string();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(last, Some(2), "Ctrl+End must reach the last row");
        assert_eq!(first, Some(0));
        assert!(value.is_empty(), "the box must not have been typed into");
    }

    #[test]
    fn an_empty_match_no_longer_counts() {
        let (dir, mut app) = app_with(&sample(), "nonempty");
        Commands::strings(&mut app);
        app.hex_view.strings_focus_filter = true;

        app.hex_view.strings_regex_input = tui_input::Input::new("(WORLD)*?".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0, "a zero-length match is not a hit");

        app.hex_view.strings_regex_input = tui_input::Input::new("(WORLD)*".to_string());
        update_strings_filter(&mut app);
        let rows: Vec<String> = app
            .hex_view
            .strings_filtered
            .iter()
            .filter_map(|&i| app.strings.get(i))
            .map(|s| s.content.clone())
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(rows, vec!["WORLD!!".to_string(), "WORLDWIDE".to_string()]);
    }

    #[test]
    fn an_empty_scan_pattern_is_not_a_filter() {
        let (dir, mut app) = app_with(&sample(), "empty_scan");
        app.string_regex = String::new();
        Commands::rescan_strings(&mut app);
        let count = app.strings.len();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(count, 3, "an empty pattern must not filter anything out");
    }

    #[test]
    fn the_filter_box_cannot_empty_the_scan() {
        let (dir, mut app) = app_with(&sample(), "no_poison");
        Commands::strings(&mut app);
        let scanned = app.strings.len();
        assert_eq!(scanned, 3);

        app.hex_view.strings_focus_filter = true;
        app.hex_view.strings_regex_input = tui_input::Input::new("([一-龥]*?){1,}".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0, "the pattern really matches nothing");

        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.strings.len(), scanned, "Enter emptied the scanned list");
        assert!(app.string_regex.is_empty(), "the box installed a scan-time filter");
        assert!(
            !app.hex_view.strings_focus_filter,
            "Enter should hand the arrows back to the list"
        );

        app.hex_view.strings_regex_input = tui_input::Input::new("WORLD".to_string());
        update_strings_filter(&mut app);
        let rows = app.hex_view.strings_filtered.len();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(rows, 2, "the list did not recover");
    }

    #[test]
    fn a_pattern_that_matches_nothing_is_reported() {
        use ratatui::{Terminal, backend::TestBackend};

        assert!(matches_the_empty_string("[a-z]*"));
        assert!(matches_the_empty_string("([一-龥]*?){4,}"));
        assert!(matches_the_empty_string("(x?)"));
        assert!(!matches_the_empty_string("[a-z]+"));
        assert!(!matches_the_empty_string("[一-龥]{2,}"));
        assert!(!matches_the_empty_string(""), "an empty box is not a warning");
        assert!(!matches_the_empty_string("["), "a half-typed pattern is not a warning");

        let (dir, mut app) = app_with(&sample(), "warn");
        Commands::strings(&mut app);
        app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);

        let render = |app: &mut App| -> String {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
            terminal.draw(|f| dialog_strings_draw(app, f)).expect("draw");
            let buffer = terminal.backend().buffer().clone();
            (0..30)
                .map(|y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect()
        };

        app.hex_view.strings_regex_input = tui_input::Input::new("([一-龥]*?){4,}".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0);
        let screen = render(&mut app);
        assert!(
            screen.contains("only ever matched an"),
            "no explanation where the rows would be:\n{}",
            screen
        );

        app.hex_view.strings_regex_input = tui_input::Input::new("ZZZZ".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0);
        let screen = render(&mut app);
        assert!(!screen.contains("only ever matched an"), "the notice is not about this case");

        app.hex_view.strings_regex_input = tui_input::Input::new("WORLD".to_string());
        update_strings_filter(&mut app);
        let screen = render(&mut app);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(screen.contains("WORLD!!"), "the rows are not being drawn");
        assert!(!screen.contains("only ever matched an"));
    }
}
#[cfg(test)]
mod string_edit_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("dz6_sedit_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, false).expect("open");
        app.file_info.is_read_only = false;
        (dir, app)
    }

    fn sample() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x60];
        bytes[0x10..0x1B].copy_from_slice(b"Hello world");
        bytes[0x1B] = 0; // null terminator
        bytes[0x1C] = 0xFF; // next data boundary
        bytes[0x30..0x35].copy_from_slice(b"Short");
        bytes
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_strings_events(app, &Event::Key(key));
    }

    fn press_edit(app: &mut App, code: KeyCode) {
        let key = KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE };
        let _ = dialog_string_edit_events(app, &Event::Key(key));
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            press_edit(app, KeyCode::Char(c));
        }
    }

    fn byte_at(app: &App, offset: usize) -> Option<u8> {
        app.hex_view
            .changed_bytes
            .get(&offset)
            .copied()
    }

    #[test]
    fn e_opens_the_box_with_the_budget() {
        let (dir, mut app) = app_with(&sample(), "open");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));

        press(&mut app, KeyCode::F(4));

        assert!(app.state == UIState::DialogStringEdit);
        assert_eq!(app.hex_view.string_edit.input.value(), "Hello world");
        assert_eq!(app.hex_view.string_edit.offset, 0x10);
        assert_eq!(app.hex_view.string_edit.budget, 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shorter_replacement_is_padded_with_nul() {
        let (dir, mut app) = app_with(&sample(), "shorter");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));

        press_edit(&mut app, KeyCode::Home);
        let key = KeyEvent {
            code: KeyCode::End,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = dialog_string_edit_events(&mut app, &Event::Key(key));
        typed(&mut app, "Bye");
        press_edit(&mut app, KeyCode::Enter);

        assert!(app.state == UIState::DialogStrings, "the box should have closed");
        assert_eq!(byte_at(&app, 0x10), Some(b'B'));
        assert_eq!(byte_at(&app, 0x11), Some(b'y'));
        assert_eq!(byte_at(&app, 0x12), Some(b'e'));
        for offset in 0x13..0x1C {
            assert_eq!(byte_at(&app, offset), Some(0), "offset 0x{:X} was not padded", offset);
        }
        assert_eq!(byte_at(&app, 0x1C), None, "the write ran past the budget");

        assert_eq!(app.strings[0].content, "Bye");
        assert!(app.strings[0].display.contains("Bye"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_longer_replacement_is_refused() {
        let (dir, mut app) = app_with(&sample(), "longer");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));

        typed(&mut app, " and then some more");
        press_edit(&mut app, KeyCode::Enter);

        assert!(app.state == UIState::DialogStringEdit, "the box must stay open");
        assert!(app.hex_view.changed_bytes.is_empty(), "bytes were written anyway");
        let error = app.hex_view.string_edit.error.clone().expect("no reason given");
        assert!(error.contains("11"), "the budget is not named: {:?}", error);

        press_edit(&mut app, KeyCode::Backspace);
        assert!(app.hex_view.string_edit.error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_budget_counts_bytes_in_the_rows_encoding() {
        let korean = encoding_rs::EUC_KR.encode("한글문자입니").0.into_owned();
        assert_eq!(korean.len(), 12);
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x10 + korean.len()].copy_from_slice(&korean);
        bytes[0x10 + korean.len()] = 0; // null terminator
        bytes[0x10 + korean.len() + 1] = 0xFF; // immediately followed by non-zero

        let (dir, mut app) = app_with(&bytes, "cp949");
        app.hex_view.strings_encoding = StringEncoding::Cp949;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));
        assert_eq!(app.hex_view.string_edit.budget, 12);

        app.hex_view.string_edit.input = tui_input::Input::new("한글문자입니다".to_string());
        press_edit(&mut app, KeyCode::Enter);
        assert!(app.hex_view.string_edit.error.is_some(), "14 bytes should not fit in 12");
        assert!(app.hex_view.changed_bytes.is_empty());

        app.hex_view.string_edit.error = None;
        app.hex_view.string_edit.input = tui_input::Input::new("한글문자입".to_string());
        press_edit(&mut app, KeyCode::Enter);

        let expected = encoding_rs::EUC_KR.encode("한글문자입").0.into_owned();
        for (i, byte) in expected.iter().enumerate() {
            assert_eq!(byte_at(&app, 0x10 + i), Some(*byte), "byte {} differs", i);
        }
        assert_eq!(byte_at(&app, 0x10 + 10), Some(0));
        assert_eq!(byte_at(&app, 0x10 + 11), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_utf16_row_is_written_as_utf16() {
        let wide: Vec<u8> = "Hello".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x10 + wide.len()].copy_from_slice(&wide);
        bytes[0x10 + wide.len()] = 0; // null terminator low
        bytes[0x10 + wide.len() + 1] = 0; // null terminator high
        bytes[0x10 + wide.len() + 2] = 0xFF; // immediately followed by non-zero

        let (dir, mut app) = app_with(&bytes, "utf16");
        app.hex_view.strings_encoding = StringEncoding::Utf16;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));
        assert_eq!(app.hex_view.string_edit.budget, 10, "five wide characters are ten bytes");

        app.hex_view.string_edit.input = tui_input::Input::new("Hi".to_string());
        press_edit(&mut app, KeyCode::Enter);

        assert_eq!(byte_at(&app, 0x10), Some(b'H'));
        assert_eq!(byte_at(&app, 0x11), Some(0));
        assert_eq!(byte_at(&app, 0x12), Some(b'i'));
        assert_eq!(byte_at(&app, 0x13), Some(0));
        for offset in 0x14..0x1C {
            assert_eq!(byte_at(&app, offset), Some(0), "0x{:X} was not padded", offset);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_only_file_refuses_up_front() {
        let (dir, mut app) = app_with(&sample(), "readonly");
        app.file_info.is_read_only = true;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));

        press(&mut app, KeyCode::F(4));

        assert!(app.state == UIState::DialogStrings, "the box opened on a read-only file");
        assert!(app.status_error.is_some(), "the refusal was not reported");
        assert!(app.hex_view.changed_bytes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esc_writes_nothing() {
        let (dir, mut app) = app_with(&sample(), "esc");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));
        typed(&mut app, "!");

        press_edit(&mut app, KeyCode::Esc);

        assert!(app.state == UIState::DialogStrings);
        assert!(app.hex_view.changed_bytes.is_empty(), "Esc wrote bytes");
        assert_eq!(app.strings[0].content, "Hello world", "the row was changed anyway");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_edit_is_undoable() {
        let (dir, mut app) = app_with(&sample(), "undo");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::F(4));
        app.hex_view.string_edit.input = tui_input::Input::new("Bye".to_string());
        press_edit(&mut app, KeyCode::Enter);

        assert_eq!(app.hex_view.changed_bytes.len(), 12, "every byte of the total capacity is staged");
        assert_eq!(
            app.hex_view.changed_history.len(),
            12,
            "the undo history has to carry them too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn utf16_scans_across_executable_sections() {
        let path = "C:\\Users\\Administrator\\Desktop\\pecmd\\dumped_SCY.exe";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut app = App::new();
        app.config.database = false;
        if app.load_file(path, 0, false).is_err() {
            return;
        }
        app.hex_view.strings_encoding = StringEncoding::Utf16;
        Commands::rescan_strings(&mut app);

        let found = app.strings.iter().position(|s| s.offset == 0x1206A8 && s.content.contains("分辨率"));
        assert!(found.is_some(), "Expected to find UTF-16 string at 0x1206A8 in .MPRESS1 section");

        app.list_state.select(found);
        press(&mut app, KeyCode::F(4));
        assert!(app.state == UIState::DialogStringEdit);
        // Original string 42 bytes + 6 trailing 00 bytes = 48 bytes total.
        // Reserving 2 bytes for null terminator (00 00) leaves 46 bytes budget!
        assert_eq!(app.hex_view.string_edit.budget, 46);
        assert_eq!(app.hex_view.string_edit.total_capacity, 48);
    }
}
