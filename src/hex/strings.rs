use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers},
    layout::{Alignment, Constraint, Direction, Layout},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
};


use std::io::Result;

use crate::{app::App, commands::Commands, editor::UIState, util::center_widget};

use regex::{Regex, RegexBuilder};

/// Rows the Home/End keys move by. The dialog shows 30 strings at a time, so a
/// step of 29 keeps one row of overlap.
const STRINGS_PAGE_STEP: usize = 29;

/// Which encoding a scan decodes byte runs as.
///
/// One encoding at a time rather than the Refs dialog's "All": merging the four
/// scans would list the ASCII fragments around every Korean string a second
/// time, and the `maximum_strings_to_show` cap would be spent on whichever scan
/// ran first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StringEncoding {
    #[default]
    Ascii,
    Cp949,
    Cp936,
    Utf16,
}

impl StringEncoding {
    /// The codec to encode a replacement string with.
    ///
    /// The same one the scan decoded the row with, so what is typed goes back as the
    /// same kind of bytes it came from - a CP949 row is written as CP949, a wide row
    /// as UTF-16LE.
    pub fn codec(&self) -> &'static encoding_rs::Encoding {
        match self {
            Self::Ascii => encoding_rs::UTF_8,
            Self::Cp949 => encoding_rs::EUC_KR,
            Self::Cp936 => encoding_rs::GBK,
            Self::Utf16 => encoding_rs::UTF_16LE,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ascii => "ASCII/UTF-8",
            Self::Cp949 => "CP949(KO)",
            Self::Cp936 => "CP936(ZH)",
            Self::Utf16 => "UTF-16LE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Ascii => Self::Cp949,
            Self::Cp949 => Self::Cp936,
            Self::Cp936 => Self::Utf16,
            Self::Utf16 => Self::Ascii,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Ascii => Self::Utf16,
            Self::Cp949 => Self::Ascii,
            Self::Cp936 => Self::Cp949,
            Self::Utf16 => Self::Cp936,
        }
    }
}

pub struct FoundString {
    pub offset: usize,
    /// Length in *bytes*, i.e. how much of the file the string occupies. Not the
    /// same as the character count for CP949/CP936/UTF-16, which is what the
    /// minimum-length filter compares against.
    pub size: usize,
    /// The string's text on its own, kept so [`FoundString::set_address`] can
    /// re-render `display` against a different address space without having to
    /// re-scan the file.
    pub content: String,
    /// `"0000ABCD  text"` as shown in the list, formatted once when the string is
    /// found (or when the address mode changes) instead of on every frame the
    /// dialog is open.
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

    /// Re-renders the list row with `addr` as the leading address, so the
    /// Disassembly view can list virtual addresses while the Hex view lists
    /// file offsets.
    pub fn set_address(&mut self, addr: u64) {
        self.display = format!("{addr:08X}  {}", self.content);
    }
}

pub fn dialog_strings_draw(app: &mut App, frame: &mut Frame) {
    let dialog_style = app.config.theme.dialog;

    // Same rect as before the filter row was added, so the dialog stays where
    // users expect it.
    let width = frame.area().width / 2;
    let height = frame.area().height / 2 + 4;
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
            Constraint::Min(1),    // the list
            Constraint::Length(3), // the filter box
        ])
        .split(inner);

    // Only the rows that are on screen are turned into widgets. Building one per
    // match was fine at the old 3,000-string cap and is not at 100,000: a
    // `ListItem` per match, every frame, is exactly the kind of per-frame
    // allocation the slow-frame log was put in to find.
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

    // The widget only knows about the window, so the highlight is addressed
    // relative to it while `app.list_state` keeps the absolute index.
    let mut window_state = ratatui::widgets::ListState::default();
    if total > 0 {
        window_state.select(Some(selected - start));
    }
    frame.render_stateful_widget(list, chunks[0], &mut window_state);

    // Nothing passed the filter, and the pattern is one that can only ever match an
    // empty string: say so where the rows would be.
    //
    // The command bar was not enough. It is cleared on the next key press, so
    // switching encoding with F2 - which re-scans and logs `52 found` - wiped the
    // explanation and left a blank list next to a log line saying there were 52
    // strings. This text stays until the pattern changes.
    if total == 0 && matches_the_empty_string(app.hex_view.strings_regex_input.value()) {
        let notice = ratatui::widgets::Paragraph::new(
            crate::i18n::M::WarnRegexEmptyOnly.tr(app.config.lang),
        )
        .style(app.config.theme.error)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .block(Block::new().padding(Padding::uniform(1)));
        frame.render_widget(notice, chunks[0]);
    }

    // Paging keys need the height of the list, which only the draw knows.
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
        // The dialog's own bottom border is taken by the minimum length, and at half
        // the terminal width all three hints together overflowed it - the label came
        // out as "imum length". This border was empty.
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

/// Rebuilds the visible subset from the regex box.
///
/// The filter runs over the already-scanned list, so typing is instant on a 27 MB
/// binary, and a pattern that matches nothing costs nothing - the scanned list is
/// untouched, so deleting a character brings the rows straight back.

/// True when `pattern` is not empty but still matches an empty string.
///
/// `[一-龥]*?` is the one that started this: `*?` is "zero or more, lazily", and
/// zero matches at the front of every row, so a Chinese search returned all 30,895
/// strings including the English ones. The pattern is doing exactly what it says;
/// what was missing was anything on screen admitting it.
pub fn matches_the_empty_string(pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map(|re| re.is_match(""))
        .unwrap_or(false)
}

pub fn update_strings_filter(app: &mut App) {
    let pattern = app.hex_view.strings_regex_input.value().trim().to_string();

    let compiled = if pattern.is_empty() {
        None
    } else {
        RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .ok()
    };
    let lower = pattern.to_lowercase();

    let mut filtered = Vec::with_capacity(app.strings.len());
    for (idx, s) in app.strings.iter().enumerate() {
        let keep = if pattern.is_empty() {
            true
        } else if let Some(re) = &compiled {
            crate::util::has_nonempty_match(re, &s.content)
        } else {
            // Not a valid regex (yet) - treat it as a literal so the list keeps
            // narrowing while a pattern is half-typed.
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
    let Event::Key(key) = event else { return Ok(false) };
    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    let focus = app.hex_view.strings_focus_filter;

    match key.code {
        KeyCode::Esc => {
            app.hex_view.strings_focus_filter = false;
            app.dialog_renderer = None;
            app.state = UIState::Normal;
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
        // The list keeps the arrows and the paging keys in both focus modes: the
        // regex box is one line, so it has no use for them.
        //
        // Moved by hand rather than with `ListState::select_next` and friends:
        // those know nothing about how long the list is, so they walked the
        // selection past the end and Enter then indexed nothing.
        KeyCode::Down => move_selection(app, 1),
        KeyCode::Up => move_selection(app, -1),
        KeyCode::PageDown => move_selection(app, page_step(app)),
        KeyCode::PageUp => move_selection(app, -page_step(app)),
        // Shift+arrows and Shift+Home/End belong to the box, not to the list.
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
        // Enter in the box accepts the filter and hands the arrows back to the
        // list, so the results can be walked immediately.
        //
        // It used to install the pattern as a *scan-time* filter and re-scan. That
        // was there to make the 3,000-string cap apply to matching strings rather
        // than to the first 3,000 in the file; with the cap at 100,000 and the whole
        // file scanned, it bought nothing and cost this: a pattern that matches
        // nothing - `([一-龥]*?){1,}`, say, which is zero characters however many
        // times it repeats - emptied `app.strings` itself. The title then read
        // `(0 / 0)`, and because the scan-time pattern stayed set, every later
        // keystroke filtered an empty list. The window never recovered.
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
                app.goto(offset);
                // Highlight the string that was jumped to, the way the Find
                // dialog highlights its match: on a large file an 8-character
                // string is otherwise impossible to pick out of the dump.
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
        KeyCode::Char('f') if !focus => {
            app.hex_view.strings_focus_filter = true;
        }
        // Replace the selected string in place. This is what the whole dialog is for
        // on a translation job: find the string, put the new text in, keep going.
        KeyCode::Char('e') if !focus => open_string_edit(app),
        // `y` the selected row, `Y` everything the filter left. Tab-separated, so a
        // narrowed list pastes into a spreadsheet as an address column and a text
        // column - which is how a translation list gets started.
        //
        // `c` too: the help, About and Log panels all take either, and which one a
        // person reaches for depends on whether they think "yank" or "copy".
        KeyCode::Char('y') | KeyCode::Char('c') if !focus => {
            let text = app
                .list_state
                .selected()
                .and_then(|i| app.hex_view.strings_filtered.get(i).copied())
                .and_then(|i| app.strings.get(i))
                .map(|s| row_as_tsv(app, s))
                .unwrap_or_default();
            app.copy_to_clipboard(text, "1 string".to_string());
        }
        KeyCode::Char('Y') | KeyCode::Char('C') if !focus => {
            let (text, count) = filtered_rows_as_tsv(app);
            app.copy_to_clipboard(text, format!("{} string(s)", count));
        }
        _ => {
            if focus && crate::text_field::handle_key(app, strings_filter_field, event) {
                update_strings_filter(app);
            }
        }
    }
    Ok(false)
}

/// The strings dialog's regex box and its selection anchor.
fn strings_filter_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (
        &mut app.hex_view.strings_regex_input,
        &mut app.hex_view.strings_filter_anchor,
    )
}

/// The in-place string replacement box.
///
/// Opened with `e` on a row of the F6 list. The whole point is the byte budget: a
/// translated string has to fit exactly where the original sits, because moving it
/// would mean fixing up every pointer to it. So the box knows how many bytes it is
/// allowed and refuses anything longer instead of writing past the end of the string
/// into whatever follows.
#[derive(Default)]
pub struct StringEdit {
    /// File offset of the first byte of the string being replaced.
    pub offset: usize,
    /// Bytes the original occupies, i.e. the budget.
    pub budget: usize,
    /// Index into `App::strings` of the row being edited, so it can be relabelled
    /// without a re-scan.
    pub row: usize,
    /// Encoding the replacement is written in - the one the row was scanned with.
    pub encoding: StringEncoding,
    pub input: tui_input::Input,
    /// Character a Shift-selection started from, or `None`.
    pub anchor: Option<usize>,
    /// Why the last attempt was refused, shown under the box.
    pub error: Option<String>,
}

/// The replacement box and its selection anchor.
fn string_edit_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (
        &mut app.hex_view.string_edit.input,
        &mut app.hex_view.string_edit.anchor,
    )
}

/// Opens the replacement box for the selected row.
///
/// Refuses a read-only file up front rather than letting the user type a translation
/// and then dropping it: the write goes through `record_edit`, which a read-only file
/// cannot accept.
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

    let cursor = found.content.chars().count();
    app.hex_view.string_edit = StringEdit {
        offset: found.offset,
        budget: found.size,
        row,
        encoding: app.hex_view.strings_encoding,
        // Pre-filled with the original, cursor at the end: most replacements are an
        // edit of what is there rather than something unrelated, and Shift+Home
        // selects the lot for the cases that are not.
        input: tui_input::Input::new(found.content.clone()).with_cursor(cursor),
        anchor: None,
        error: None,
    };
    app.state = UIState::DialogStringEdit;
    app.dialog_2nd_renderer = Some(dialog_string_edit_draw);
}

/// Writes the replacement, or reports why it will not fit.
///
/// Padding is `00` rather than spaces: the string is C-terminated where it sits, so
/// a shorter replacement has to end the string, not merely blank the tail.
fn commit_string_edit(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoStringEdit);
        return;
    }

    let text = app.hex_view.string_edit.input.value().to_string();
    let budget = app.hex_view.string_edit.budget;
    let offset = app.hex_view.string_edit.offset;
    let encoding = app.hex_view.string_edit.encoding;

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

    // Through `record_edit`, so the bytes land in the same staged map as every other
    // edit: the hex view marks them, undo and Alt+F3 revert them, and `:w` is what
    // writes them.
    for (i, byte) in bytes.iter().enumerate() {
        crate::hex::edit::record_edit(app, offset + i, *byte);
    }
    let padding = budget - bytes.len();
    for i in bytes.len()..budget {
        crate::hex::edit::record_edit(app, offset + i, 0);
    }

    // Relabel the row in place. A re-scan would read the file rather than the staged
    // edits and put the old text straight back.
    let row = app.hex_view.string_edit.row;
    let use_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if use_va { app.get_va(offset) } else { offset as u64 };
    if let Some(found) = app.strings.get_mut(row) {
        found.content = text;
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

    app.hex_view.string_edit = StringEdit::default();
    app.dialog_2nd_renderer = None;
    app.state = UIState::DialogStrings;
}

pub fn dialog_string_edit_draw(app: &mut App, frame: &mut Frame) {
    let edit = &app.hex_view.string_edit;
    let width = 64.min(frame.area().width.saturating_sub(4)).max(28);
    // Tall enough for the refusal to be read: it names both byte counts and what the
    // rule is, which wraps to two or three lines in a box this wide. Sized to the
    // wrap instead of a fixed row, because a truncated explanation is no better than
    // none.
    let error_rows = match &edit.error {
        None => 0,
        Some(error) => {
            let inner = width.saturating_sub(2).max(1) as usize;
            ((error.chars().count() + inner - 1) / inner).clamp(1, 4) as u16
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

    let block = Block::bordered()
        .title(title)
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
            app.hex_view.string_edit = StringEdit::default();
            app.dialog_2nd_renderer = None;
            app.state = UIState::DialogStrings;
        }
        KeyCode::Enter => commit_string_edit(app),
        _ => {
            if crate::text_field::handle_key(app, string_edit_field, event) {
                // A refusal is about the text that was refused, so it goes as soon as
                // the text changes.
                app.hex_view.string_edit.error = None;
            }
        }
    }
    Ok(false)
}

/// One list row as `address<TAB>text`.
///
/// The address is the one the list is showing - a virtual address in the Disasm
/// view or in VA mode, a file offset otherwise - so what is pasted matches what
/// was on screen. Neither field can contain a tab: every scanner rejects control
/// bytes.
fn row_as_tsv(app: &App, s: &FoundString) -> String {
    let use_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if use_va { app.get_va(s.offset) } else { s.offset as u64 };
    format!("{:08X}\t{}", addr, s.content)
}

/// Every row the filter left, and how many there are.
///
/// Split out from the key handler so what gets copied can be checked without
/// touching the real clipboard - which is a shared OS resource that parallel tests
/// fight over, and is missing entirely on a bare TTY.
///
/// CRLF line endings: this is going to a Windows clipboard, and a spreadsheet
/// pasting LF-only text puts the lot in one cell.
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

/// Rows one PageUp/PageDown covers: a screenful less one row of overlap.
fn page_step(app: &App) -> isize {
    let rows = app.hex_view.strings_page_rows;
    if rows < 2 {
        STRINGS_PAGE_STEP as isize
    } else {
        (rows - 1) as isize
    }
}

/// Moves the highlight by `delta`, clamped to the filtered list.
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

    /// Re-runs the scan and reports the outcome.
    ///
    /// Every setting on this dialog (encoding, minimum length, the regex when it
    /// is applied at scan time) needs the same four steps, and a scan that finds
    /// nothing used to leave an empty list with no explanation.
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
        // Nothing in the interface sets `string_regex` any more, but if something
        // ever does and it cannot match, the count above is a bare zero with no
        // reason attached - which is exactly how the `(0 / 0)` title read.
        if app.strings.is_empty() && matches_the_empty_string(&app.string_regex) {
            let message = crate::i18n::M::WarnRegexEmptyOnly.tr(app.config.lang).to_string();
            app.status_error = Some(message.clone());
            App::log(app, message);
        }
    }

    /// Relabels every row of the strings list with the address space the
    /// current view thinks in: virtual addresses in the Disassembly view (or
    /// when the Hex view is in VA mode), file offsets otherwise.
    ///
    /// Done once when the dialog opens rather than per frame, and `strings` is
    /// moved out first because `get_va` needs to borrow `app` immutably while
    /// the list is being mutated.
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
        // If the string list is already filled, just reuse it
        if force_read {
            app.strings.clear();
        }

        if !app.strings.is_empty() {
            return;
        }

        // `None` means "no filter". It used to be an empty `Regex`, which under the
        // old `is_match` rule matched everything and under the new one would match
        // nothing at all - the scan would come back empty on every file.
        let re = if app.string_regex.trim().is_empty() {
            None
        } else {
            RegexBuilder::new(&app.string_regex)
                .case_insensitive(true)
                .build()
                .ok()
        };
        let re = re.as_ref();

        let min = app.config.minimum_string_length;
        let cap = app.config.maximum_strings_to_show;
        let encoding = app.hex_view.strings_encoding;

        // Where to look. The multi-byte scans only run over the sections that do
        // not hold code, because that is where the last of their false positives
        // come from: `48 8B` is a perfectly good CP936 character, and a stretch of
        // x64 between two zero words is a perfectly good UTF-16 string. Files with
        // no section table (raw dumps, non-PE) are scanned whole, and the ASCII
        // scan always is - a 4-character printable run means something wherever it
        // sits.
        let buf_len = app.file_info.get_buffer_ref().len();
        let ranges: Vec<std::ops::Range<usize>> = if encoding == StringEncoding::Ascii {
            vec![0..buf_len]
        } else {
            let data = crate::disasm::sections::data_sections(app, buf_len);
            if data.is_empty() {
                vec![0..buf_len]
            } else {
                data.iter().map(|s| s.start..s.end).collect()
            }
        };

        let buffer = app.file_info.get_buffer();
        let mut out = Vec::new();

        for range in ranges {
            match encoding {
                StringEncoding::Ascii => scan_ascii(buffer, min, cap, re, &mut out),
                StringEncoding::Cp949 => scan_dbcs(buffer, range, &CP949, min, cap, re, &mut out),
                StringEncoding::Cp936 => scan_dbcs(buffer, range, &CP936, min, cap, re, &mut out),
                StringEncoding::Utf16 => scan_utf16(buffer, range, min, cap, re, &mut out),
            }
            if out.len() >= cap {
                break;
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

/// Whether a candidate string passes the scan-time regex.
///
/// `None` is no filter at all. A non-empty match is required, for the reason in
/// `util::has_nonempty_match`.
fn accepts(re: Option<&Regex>, text: &str) -> bool {
    match re {
        None => true,
        Some(re) => crate::util::has_nonempty_match(re, text),
    }
}

/// True for a byte that can appear inside a single-byte printable run.
fn is_ascii_text(b: u8) -> bool {
    b.is_ascii_graphic() || b == b' '
}

/// A double-byte codepage, narrowed to the byte pairs real text actually uses.
///
/// Deciding "is this a Chinese string?" by whether GBK can decode it does not
/// work: GBK assigns almost the whole 0x81..0xFE lead space, so a stretch of x64
/// machine code decodes without a single error. On a Korean binary containing no
/// Chinese at all, that test reported 11,586 strings. Restricting the pairs to
/// the GB2312 / KS X 1001 blocks below is what separates text from code, because
/// code bytes only rarely land two valid pairs in a row inside them.
struct Dbcs {
    enc: &'static encoding_rs::Encoding,
    /// Valid lead byte of a pair.
    lead_ok: fn(u8) -> bool,
    /// Valid trail byte of a pair.
    trail_ok: fn(u8) -> bool,
    /// A character of the script being looked for.
    is_target: fn(char) -> bool,
}

/// GB2312 hanzi (lead 0xB0..0xF7) plus the fullwidth punctuation and alphabet
/// rows (0xA1..0xA9). GBK's extension blocks are deliberately left out: they hold
/// rare characters that almost never appear in shipped strings, and they are
/// where machine code lands.
fn gbk_lead(b: u8) -> bool {
    (0xA1..=0xA9).contains(&b) || (0xB0..=0xF7).contains(&b)
}

fn gbk_trail(b: u8) -> bool {
    (0xA1..=0xFE).contains(&b)
}

/// KS X 1001 precomposed Hangul (lead 0xB0..0xC8) plus the punctuation rows.
/// The CP949 extension area (0x81..0xA0) and the hanja rows are excluded for the
/// same reason.
fn euckr_lead(b: u8) -> bool {
    (0xA1..=0xA2).contains(&b) || (0xB0..=0xC8).contains(&b)
}

fn euckr_trail(b: u8) -> bool {
    (0xA1..=0xFE).contains(&b)
}

/// How many script characters have to sit next to each other for a run to count
/// as text.
///
/// A single stray pair is what machine code produces; words in either language
/// are two or more characters. This one rule removes most of what was left after
/// the byte ranges above, and unlike a "at least half the characters" ratio it
/// still accepts a mixed string such as `Copyright (C) 2011 北京公司`.
const MIN_SCRIPT_RUN: usize = 2;

/// Longest streak of `is_target` characters in `text`.
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

/// True for filler such as `CC CC CC CC`, which GBK decodes as the same hanzi
/// over and over. Three or more is padding, not text; two is left alone because
/// Chinese does have real doubled words.
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

        // Walk as far as the codepage stays valid: printable single bytes, or a
        // lead/trail pair from the restricted blocks.
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
        // A C string ends in a NUL. Machine code that happens to decode ends at
        // whatever byte broke the run, so this alone throws out most of what the
        // byte ranges let through.
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
    let mut siz = 0usize;
    let mut candidate = String::new();

    for (offset, byte) in buffer.iter().enumerate() {
        if is_ascii_text(*byte) {
            candidate.push(*byte as char);
            siz += 1;
        } else {
            if siz >= min && accepts(re, &candidate) {
                out.push(FoundString::new(offset - siz, &candidate, siz));
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
/// True for a UTF-16 code unit worth showing.
///
/// A whitelist of blocks rather than "anything above U+009F": two random bytes
/// land in the CJK block a third of the time, so the loose test reported 54,552
/// wide strings in a binary that has a few hundred. Surrogates never reach here
/// because `char::from_u32` rejects them.
fn is_wide_text(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    matches!(c as u32,
        0x20..=0x7E          // ASCII printable
        | 0xA0..=0x24F       // Latin-1 supplement, Latin Extended-A/B
        | 0x370..=0x3FF      // Greek
        | 0x400..=0x4FF      // Cyrillic
        | 0x2010..=0x203A    // general punctuation
        | 0x20A0..=0x20BF    // currency symbols
        | 0x3000..=0x30FF    // CJK punctuation, Hiragana, Katakana
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xAC00..=0xD7A3    // Hangul syllables
        | 0xFF01..=0xFF60    // fullwidth forms
    )
}

/// True when every non-ASCII character in `text` is a pair of printable ASCII
/// bytes.
///
/// That is the signature of single-byte text being read two bytes at a time:
/// `CollectExceptionInfo` comes back as `佃敬硅散瑰潩`, and on a real binary this
/// was every one of the 331 hits a Chinese search returned. Real CJK does not
/// look like this for long, because the low byte of a hanzi is arbitrary - the
/// chance that three in a row are all printable is under one in a hundred - and
/// Hangul cannot look like it at all, since U+AC00..U+D7A3 puts 0xAC..0xD7 in the
/// high byte. Three characters is where it starts being applied, so a short real
/// string is never judged by it.
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

/// Scans 2-byte-aligned UTF-16LE runs.
///
/// Only the even alignment is scanned: compilers emit wide literals aligned, and
/// scanning both parities would double the work to report each string twice.
///
/// Both ends have to be NUL: a wide literal is NUL-terminated, and the unit
/// before it is the previous literal's terminator or alignment padding. Without
/// that pair of checks this mode is unusable on an executable, because a run of
/// machine code decodes into perfectly printable-looking ideographs.
fn scan_utf16(
    buffer: &[u8],
    range: std::ops::Range<usize>,
    min: usize,
    cap: usize,
    re: Option<&Regex>,
    out: &mut Vec<FoundString>,
) {
    let len = range.end.min(buffer.len());
    // Code units are read at even file offsets, so a section starting on an odd
    // one is entered one byte later rather than knocked out of step.
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
            // Nothing printable here; step over the offending unit.
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

        // Step past the terminator that ended the run.
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

    /// The Korean scan must not turn into a second ASCII scan: a run only counts
    /// when it decodes cleanly *and* contains Hangul.
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

    /// Minimum length is a character count, so a 4-character Korean string (8
    /// bytes) is judged as 4.
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

    /// UTF-16LE text is invisible to the ASCII scan, which is the whole reason
    /// for the encoding switch.
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

    /// The cap is honoured so a scan of a large file cannot grow without bound.
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
        // The pid keeps parallel test binaries off each other's fixtures.
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

    /// The selected row is an index into the *filtered* list. Enter used to index
    /// `app.strings` with it directly, which jumped to the wrong string as soon
    /// as anything was typed in the filter box.
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

    /// Switching the encoding re-scans, and the list that comes back is the one
    /// for the new encoding.
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

        let encoding = app.hex_view.strings_encoding;
        let contents: Vec<String> = app.strings.iter().map(|s| s.content.clone()).collect();
        let filtered = app.hex_view.strings_filtered.len();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(encoding, StringEncoding::Cp949);
        assert_eq!(contents, vec!["한글문자".to_string()], "the list must be re-scanned");
        assert_eq!(filtered, 1, "the filter has to be rebuilt against the new list");
    }

    /// Typing in the regex box must not be swallowed by the list bindings: '+'
    /// and 'f' are list shortcuts, and both are legal regex input.
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
        // The copy keys are list shortcuts too, and both are legal regex input.
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

    /// x64 machine code must not be reported as Chinese.
    ///
    /// This is the bug this whole set of rules exists for: GBK assigns nearly the
    /// whole lead-byte space, so `decode_without_bom_handling` accepts a stretch
    /// of code without a single error. Measured on a 2.6 MB Korean binary that
    /// contains exactly one Chinese string, the old test reported 11,586 of them.
    #[test]
    fn cp936_ignores_x64_code_that_decodes_cleanly() {
        // mov rcx,rbx / mov bl,dl / mov rdx,rbx, then the MSVC 0xCC filler.
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

    /// `CC CC CC CC` decodes as the same hanzi four times over. It is stack
    /// filler, and it was the single most common false positive left after the
    /// byte ranges were narrowed.
    #[test]
    fn repeated_filler_is_not_a_string() {
        let mut buffer = vec![0x00];
        buffer.extend_from_slice(&[0xCC; 12]);
        buffer.push(0x00);

        let mut out = Vec::new();
        scan_dbcs(&buffer, 0..buffer.len(), &CP936, 4, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "0xCC filler must not be reported");
    }

    /// A run that does not end in a NUL is not a C string.
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
        unterminated[0x10 + korean.len()] = 0x01; // not printable, not a NUL
        let mut out = Vec::new();
        scan_dbcs(&unterminated, 0..unterminated.len(), &CP949, 3, 100, Some(&any()), &mut out);
        assert!(out.is_empty(), "a run cut off by a control byte is not text");
    }

    /// A lone valid pair inside ASCII is what code produces; a word is two or
    /// more characters.
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

    /// A real mixed string still comes through, so the rules above did not just
    /// turn the mode off.
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

    /// The scan only covers the ranges it is given, which is how the multi-byte
    /// modes stay out of the code sections.
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

    /// UTF-16 units outside the blocks real text uses are rejected. Two arbitrary
    /// bytes land in the CJK block a third of the time, so without this the mode
    /// reported 54,552 strings in the same 2.6 MB binary.
    #[test]
    fn utf16_rejects_units_outside_the_text_blocks() {
        let mut buffer = vec![0u8; 0x40];
        // U+0EBA is unassigned Lao - a pair of bytes, not text.
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

    /// A wide literal has a NUL on both sides: its own terminator, and the
    /// previous literal's terminator or the alignment padding in front of it.
    #[test]
    fn utf16_requires_a_nul_on_both_sides() {
        let wide: Vec<u8> = "Hello".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();

        let mut clean = vec![0u8; 0x40];
        clean[0x10..0x10 + wide.len()].copy_from_slice(&wide);
        let mut out = Vec::new();
        scan_utf16(&clean, 0..clean.len(), 4, 100, Some(&any()), &mut out);
        assert_eq!(out.len(), 1, "a padded wide literal is text");

        let mut crowded = vec![0u8; 0x40];
        // A control unit, so it ends the run rather than joining it.
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

    /// Wide literals, one after another, with `tail` last.
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

    /// The scan has to reach the end of the file.
    ///
    /// `maximum_strings_to_show` was 3,000 and it stops the *scan*, not the
    /// display: on a 2.6 MB binary the sweep gave up 12% in, so a filter regex
    /// that was perfectly correct - `[가-힣]`, say - matched nothing at all,
    /// because the strings it was looking for had never been collected. Nothing
    /// on screen said so either; the list simply came back empty.
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

    /// A selection far down a long list has to be on screen.
    ///
    /// The list is windowed now - only the visible rows are turned into widgets -
    /// so the window has to follow the selection rather than always starting at
    /// row zero.
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

    /// The arrows and the paging keys stop at both ends of the filtered list.
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

    /// Single-byte text read two bytes at a time is not a wide string.
    ///
    /// `CollectExceptionInfo` sits in the packed metadata of a managed binary as
    /// plain ASCII. Read at 2-byte width it decodes to `佃敬硅散瑰潩`, and a
    /// Chinese search on a real 2.6 MB executable returned 331 of these against a
    /// single genuine hit.
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

    /// Real Chinese UI text still comes through.
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

    /// Korean is never judged by that rule: U+AC00..U+D7A3 puts 0xAC..0xD7 in the
    /// high byte, which is outside printable ASCII, so a Hangul string can never
    /// look like misread single-byte text.
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

    /// Two characters is below the threshold, so a short real string is never
    /// judged by the rule even if both its bytes happen to be printable.
    #[test]
    fn the_rule_needs_three_characters() {
        // 版 is U+7248 and 本 is U+672C - every byte printable ASCII.
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

    /// A row is `address<TAB>text`, with the address the list is actually showing.
    #[test]
    fn a_row_is_tab_separated_and_uses_the_displayed_address() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        let (dir, mut app) = app_with(&bytes, "row_format");
        Commands::strings(&mut app);

        let found = &app.strings[0];
        assert_eq!(row_as_tsv(&app, found), "00000010\tHELLO");

        // VA mode has to change what is copied, the same way it changes the list.
        app.hex_view.show_va = true;
        let va = app.get_va(found.offset);
        assert_eq!(row_as_tsv(&app, found), format!("{:08X}\tHELLO", va));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `y` copies one row, `Y` copies what the filter left - not the whole scan.
    #[test]
    fn y_copies_one_row_and_shift_y_copies_the_filtered_list() {
        let mut bytes = vec![0u8; 0x80];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        bytes[0x20..0x27].copy_from_slice(b"WORLD!!");
        bytes[0x30..0x39].copy_from_slice(b"WORLDWIDE");
        let (dir, mut app) = app_with(&bytes, "counts");
        Commands::strings(&mut app);
        assert_eq!(app.strings.len(), 3);

        // What `Y` hands to the clipboard. Checked here rather than by reading the
        // clipboard back: it is a shared OS resource, and parallel tests fighting
        // over it turned this into a coin flip.
        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 3);
        assert_eq!(text.lines().count(), 3);
        assert!(text.starts_with("00000010\tHELLO"), "got {:?}", text);

        // Narrow the list: Y must follow the filter.
        app.hex_view.strings_regex_input = tui_input::Input::new("^WORLD".to_string());
        update_strings_filter(&mut app);
        let (text, count) = filtered_rows_as_tsv(&app);
        assert_eq!(count, 2, "Y must copy the filtered rows");
        assert!(!text.contains("HELLO"), "got {:?}", text);

        // And both keys report without closing the dialog.
        app.logs.clear();
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Char('Y'));
        let logs = app.logs.clone();
        let state_kept = app.state == UIState::DialogStrings;

        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(logs.len(), 2, "logs: {:?}", logs);
        assert!(logs.iter().all(|l| l.contains("clipboard")), "logs: {:?}", logs);
        assert!(state_kept, "copying closed the dialog");
    }

    /// `c` is an alias, as it is on the help, About and Log panels.
    #[test]
    fn c_copies_like_y() {
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        let (dir, mut app) = app_with(&bytes, "alias");
        Commands::strings(&mut app);

        for key in [KeyCode::Char('c'), KeyCode::Char('C')] {
            app.logs.clear();
            press(&mut app, key);
            let last = app.logs.last().cloned().unwrap_or_default();
            assert!(last.contains("clipboard"), "{:?} logged {:?}", key, last);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An empty list has nothing to copy, and says so rather than reporting a
    /// successful copy of nothing.
    #[test]
    fn copying_an_empty_list_is_reported() {
        let (dir, mut app) = app_with(&[0u8; 0x40], "empty");
        Commands::strings(&mut app);
        assert!(app.strings.is_empty());

        app.logs.clear();
        press(&mut app, KeyCode::Char('y'));
        let last = app.logs.last().cloned().unwrap_or_default();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(last.contains("Nothing to copy"), "got {:?}", last);
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

    /// With the box focused, Shift+Left blocks text instead of touching the list,
    /// and Home/End move the cursor instead of paging.
    #[test]
    fn the_filter_box_owns_the_movement_keys_while_focused() {
        let (dir, mut app) = app_with(&sample(), "owns_keys");
        Commands::strings(&mut app);
        app.list_state.select(Some(1));

        // Set the pattern outright rather than typing it: a half-typed `[` matches
        // nothing, which empties the list and legitimately resets the selection -
        // that is not what this test is about.
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

        // Plain End is the box's too while it has focus, not a page of the list.
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        let after = app.list_state.selected();
        let cursor = app.hex_view.strings_regex_input.cursor();
        let anchor = app.hex_view.strings_filter_anchor;
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(cursor, 5);
        assert_eq!(anchor, None, "a plain End drops the block");
        assert_eq!(after, Some(1));
    }

    /// Typing over the block replaces the whole pattern, which is the point.
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

    /// Without focus the paging keys still belong to the list.
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

    /// A match has to be non-empty, so `*` no longer means "everything".
    ///
    /// `([一-龥]*?)` used to pass all 30,895 rows including the English ones, because
    /// `is_match` counts the zero-length match every engine finds at position 0. It
    /// now selects nothing - which is what the pattern really says - and the command
    /// bar explains it. The greedy form is the one that works.
    #[test]
    fn an_empty_match_no_longer_counts() {
        let (dir, mut app) = app_with(&sample(), "nonempty");
        Commands::strings(&mut app);
        app.hex_view.strings_focus_filter = true;

        // Lazy: only ever matches nothing.
        app.hex_view.strings_regex_input = tui_input::Input::new("(WORLD)*?".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0, "a zero-length match is not a hit");

        // Greedy: matches the rows that actually contain it, and no others.
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

    /// An empty scan-time pattern still means "no filter".
    ///
    /// It used to be compiled to an empty `Regex`, which under the new rule matches
    /// nothing at all - every scan would have come back empty.
    #[test]
    fn an_empty_scan_pattern_is_not_a_filter() {
        let (dir, mut app) = app_with(&sample(), "empty_scan");
        app.string_regex = String::new();
        Commands::rescan_strings(&mut app);
        let count = app.strings.len();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(count, 3, "an empty pattern must not filter anything out");
    }

    /// A pattern that matches nothing must not empty the *scan*.
    ///
    /// Enter in the box used to install the pattern as a scan-time filter and
    /// re-scan. `([一-龥]*?){1,}` matches zero characters however many times it
    /// repeats, so the scan came back empty: the title read `(0 / 0)`, and since the
    /// pattern stayed installed, every later keystroke filtered an empty list. The
    /// window could not recover without being closed and reopened.
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

        // And the rows come straight back when the pattern is fixed - the point of
        // filtering the scanned list rather than re-scanning.
        app.hex_view.strings_regex_input = tui_input::Input::new("WORLD".to_string());
        update_strings_filter(&mut app);
        let rows = app.hex_view.strings_filtered.len();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(rows, 2, "the list did not recover");
    }

    /// A pattern that can only match nothing says so where the rows would be.
    ///
    /// The command bar was not enough: it is cleared on the next key press, so F2
    /// (switch encoding) wiped the explanation and left a blank list next to a log
    /// line reporting 52 strings found.
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

        // A pattern whose only match is the empty one.
        app.hex_view.strings_regex_input = tui_input::Input::new("([一-龥]*?){4,}".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0);
        let screen = render(&mut app);
        // A fragment that survives the wrap: the notice is spread over three lines at
        // this width, so anything longer straddles a line break.
        assert!(
            screen.contains("only ever matched an"),
            "no explanation where the rows would be:\n{}",
            screen
        );

        // A pattern that simply has no hits says nothing extra: the count in the
        // title already covers it.
        app.hex_view.strings_regex_input = tui_input::Input::new("ZZZZ".to_string());
        update_strings_filter(&mut app);
        assert_eq!(app.hex_view.strings_filtered.len(), 0);
        let screen = render(&mut app);
        assert!(!screen.contains("only ever matched an"), "the notice is not about this case");

        // And with rows on screen there is nothing to explain.
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

    /// A writable fixture: the write path is the whole point here.
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
        bytes[0x10..0x1B].copy_from_slice(b"Hello world"); // 11 bytes
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
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
    }

    /// `e` opens the box pre-filled, and knows how many bytes it may use.
    #[test]
    fn e_opens_the_box_with_the_budget() {
        let (dir, mut app) = app_with(&sample(), "open");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));

        press(&mut app, KeyCode::Char('e'));

        assert!(app.state == UIState::DialogStringEdit);
        assert_eq!(app.hex_view.string_edit.input.value(), "Hello world");
        assert_eq!(app.hex_view.string_edit.offset, 0x10);
        assert_eq!(app.hex_view.string_edit.budget, 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A shorter replacement is padded with 00, because the string ends where it
    /// sits - blanking the tail with spaces would leave the old length behind.
    #[test]
    fn a_shorter_replacement_is_padded_with_nul() {
        let (dir, mut app) = app_with(&sample(), "shorter");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));

        // Select everything and type the replacement.
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
        for offset in 0x13..0x1B {
            assert_eq!(byte_at(&app, offset), Some(0), "offset 0x{:X} was not padded", offset);
        }
        assert_eq!(byte_at(&app, 0x1B), None, "the write ran past the budget");

        // The row is relabelled without a re-scan, which would read the file and put
        // the old text straight back.
        assert_eq!(app.strings[0].content, "Bye");
        assert!(app.strings[0].display.contains("Bye"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A replacement that does not fit is refused, and says by how much.
    #[test]
    fn a_longer_replacement_is_refused() {
        let (dir, mut app) = app_with(&sample(), "longer");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));

        typed(&mut app, " and then some more");
        press_edit(&mut app, KeyCode::Enter);

        assert!(app.state == UIState::DialogStringEdit, "the box must stay open");
        assert!(app.hex_view.changed_bytes.is_empty(), "bytes were written anyway");
        let error = app.hex_view.string_edit.error.clone().expect("no reason given");
        assert!(error.contains("11"), "the budget is not named: {:?}", error);

        // The refusal is about the text that was refused, so it goes when it changes.
        press_edit(&mut app, KeyCode::Backspace);
        assert!(app.hex_view.string_edit.error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The budget is in bytes, not characters: a CP949 row is written as CP949.
    #[test]
    fn the_budget_counts_bytes_in_the_rows_encoding() {
        // Twelve bytes of EUC-KR Korean, so the row is six characters wide.
        let korean = encoding_rs::EUC_KR.encode("한글문자입니").0.into_owned();
        assert_eq!(korean.len(), 12);
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x10 + korean.len()].copy_from_slice(&korean);

        let (dir, mut app) = app_with(&bytes, "cp949");
        app.hex_view.strings_encoding = StringEncoding::Cp949;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.hex_view.string_edit.budget, 12);

        // Seven Korean characters is fourteen bytes: too long, even though the
        // original was six characters and this is only one more.
        app.hex_view.string_edit.input = tui_input::Input::new("한글문자입니다".to_string());
        press_edit(&mut app, KeyCode::Enter);
        assert!(app.hex_view.string_edit.error.is_some(), "14 bytes should not fit in 12");
        assert!(app.hex_view.changed_bytes.is_empty());

        // Five characters is ten bytes, and the remaining two are zeroed.
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

    /// A wide row is written back as UTF-16LE, not as UTF-8.
    #[test]
    fn a_utf16_row_is_written_as_utf16() {
        let wide: Vec<u8> = "Hello".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x10 + wide.len()].copy_from_slice(&wide);

        let (dir, mut app) = app_with(&bytes, "utf16");
        app.hex_view.strings_encoding = StringEncoding::Utf16;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.hex_view.string_edit.budget, 10, "five wide characters are ten bytes");

        app.hex_view.string_edit.input = tui_input::Input::new("Hi".to_string());
        press_edit(&mut app, KeyCode::Enter);

        assert_eq!(byte_at(&app, 0x10), Some(b'H'));
        assert_eq!(byte_at(&app, 0x11), Some(0));
        assert_eq!(byte_at(&app, 0x12), Some(b'i'));
        assert_eq!(byte_at(&app, 0x13), Some(0));
        for offset in 0x14..0x1A {
            assert_eq!(byte_at(&app, offset), Some(0), "0x{:X} was not padded", offset);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only file refuses before the user types anything.
    #[test]
    fn a_read_only_file_refuses_up_front() {
        let (dir, mut app) = app_with(&sample(), "readonly");
        app.file_info.is_read_only = true;
        Commands::strings(&mut app);
        app.list_state.select(Some(0));

        press(&mut app, KeyCode::Char('e'));

        assert!(app.state == UIState::DialogStrings, "the box opened on a read-only file");
        assert!(app.status_error.is_some(), "the refusal was not reported");
        assert!(app.hex_view.changed_bytes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Esc leaves the file alone.
    #[test]
    fn esc_writes_nothing() {
        let (dir, mut app) = app_with(&sample(), "esc");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));
        typed(&mut app, "!");

        press_edit(&mut app, KeyCode::Esc);

        assert!(app.state == UIState::DialogStrings);
        assert!(app.hex_view.changed_bytes.is_empty(), "Esc wrote bytes");
        assert_eq!(app.strings[0].content, "Hello world", "the row was changed anyway");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The written bytes are staged like any other edit, so undo reaches them.
    #[test]
    fn the_edit_is_undoable() {
        let (dir, mut app) = app_with(&sample(), "undo");
        Commands::strings(&mut app);
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('e'));
        app.hex_view.string_edit.input = tui_input::Input::new("Bye".to_string());
        press_edit(&mut app, KeyCode::Enter);

        assert_eq!(app.hex_view.changed_bytes.len(), 11, "every byte of the budget is staged");
        assert_eq!(
            app.hex_view.changed_history.len(),
            11,
            "the undo history has to carry them too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

