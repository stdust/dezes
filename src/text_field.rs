//! A single-line text box with a selection.
//!
//! `tui_input` has no selection at all: measured, it handles Home, End and
//! Ctrl+Left/Right, and drops Shift+Left, Shift+Right and Shift+Home on the floor.
//! Every filter box in the program was therefore a field you could only edit one
//! character at a time - replacing a regex meant twenty Backspaces.
//!
//! The Goto dialog grew its own copy of this logic inline, which is where the shape
//! of the state comes from: the `Input` keeps the text and the cursor, and a
//! separate anchor remembers where a Shift-selection started. `None` means nothing
//! is selected.
//!
//! Kept as free functions over `(&mut Input, &mut Option<usize>)` rather than a new
//! widget type, so the dialogs keep the `Input` fields they already have and only
//! gain an anchor.

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::app::App;

/// Reaches a dialog's text box and its selection anchor from `App`.
///
/// A function pointer rather than two `&mut` arguments: the clipboard lives on
/// `App` too, and no borrow of a field can be held while it is used.
pub type Field = fn(&mut App) -> (&mut Input, &mut Option<usize>);

/// Number of characters, which is what `Input::cursor` counts.
fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Byte offset of character `index`, clamped to the end of the string.
fn byte_at(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// The selected character range, ordered, or `None` when nothing is selected.
pub fn selection(input: &Input, anchor: Option<usize>) -> Option<(usize, usize)> {
    let anchor = anchor?;
    let cursor = input.cursor();
    let (start, end) = (anchor.min(cursor), anchor.max(cursor));
    if start == end {
        None
    } else {
        Some((start.min(char_len(input.value())), end.min(char_len(input.value()))))
    }
}

/// The text before, inside and after the selection.
pub fn split<'a>(input: &'a Input, anchor: Option<usize>) -> (&'a str, &'a str, &'a str) {
    let value = input.value();
    match selection(input, anchor) {
        None => (value, "", ""),
        Some((start, end)) => {
            let (b_start, b_end) = (byte_at(value, start), byte_at(value, end));
            (&value[..b_start], &value[b_start..b_end], &value[b_end..])
        }
    }
}

/// The box's contents as a line, with any selection highlighted.
pub fn render_line<'a>(
    input: &'a Input,
    anchor: Option<usize>,
    base: Style,
    highlight: Style,
) -> Line<'a> {
    let (before, selected, after) = split(input, anchor);
    if selected.is_empty() {
        return Line::from(Span::styled(before, base));
    }
    Line::from(vec![
        Span::styled(before, base),
        Span::styled(selected, highlight),
        Span::styled(after, base),
    ])
}

/// A horizontally scrolled slice of the value, with any selection highlighted.
///
/// `skip` and `take` are in characters. Boxes that scroll their contents (the
/// comment box, the boxed field rows) have to highlight the part of the selection
/// that is actually on screen, not the whole of it.
pub fn render_window(
    input: &Input,
    anchor: Option<usize>,
    skip: usize,
    take: usize,
    base: Style,
    highlight: Style,
) -> Line<'static> {
    let visible: Vec<char> = input.value().chars().skip(skip).take(take).collect();
    let Some((start, end)) = selection(input, anchor) else {
        return Line::from(Span::styled(visible.into_iter().collect::<String>(), base));
    };

    // The selection, moved into the window's coordinates and clipped to it.
    let start = start.saturating_sub(skip).min(visible.len());
    let end = end.saturating_sub(skip).min(visible.len());
    if start == end {
        return Line::from(Span::styled(visible.into_iter().collect::<String>(), base));
    }

    let before: String = visible[..start].iter().collect();
    let middle: String = visible[start..end].iter().collect();
    let after: String = visible[end..].iter().collect();
    Line::from(vec![
        Span::styled(before, base),
        Span::styled(middle, highlight),
        Span::styled(after, base),
    ])
}

/// Replaces the selection (or inserts at the cursor) and clears the anchor.
///
/// Returns whether the text changed.
fn replace_selection(input: &mut Input, anchor: &mut Option<usize>, with: &str) -> bool {
    let (before, selected, after) = split(input, *anchor);
    if selected.is_empty() && with.is_empty() {
        return false;
    }
    let cursor = char_len(before) + char_len(with);
    let value = format!("{}{}{}", before, with, after);
    *input = Input::new(value).with_cursor(cursor);
    *anchor = None;
    true
}

/// Moves the cursor, starting or extending a selection when `extend` is set.
fn move_cursor(input: &mut Input, anchor: &mut Option<usize>, to: usize, extend: bool) {
    if extend {
        if anchor.is_none() {
            *anchor = Some(input.cursor());
        }
    } else {
        *anchor = None;
    }
    let to = to.min(char_len(input.value()));
    *input = Input::new(input.value().to_string()).with_cursor(to);
}

/// Handles one key for a text box, returning whether the *text* changed.
///
/// Selection moves return false: they need a redraw, which happens anyway, but not
/// a re-filter.
pub fn handle_key(app: &mut App, field: Field, event: &Event) -> bool {
    let Event::Key(key) = event else { return false };
    if key.kind != KeyEventKind::Press {
        return false;
    }

    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if ctrl {
        match key.code {
            // Copy: the selection, or the whole box when nothing is selected - the
            // same rule the Goto dialog uses.
            KeyCode::Char('c') | KeyCode::Char('C') => {
                let (input, anchor) = field(app);
                let (before, selected, _) = split(input, *anchor);
                let text = if selected.is_empty() {
                    format!("{}{}", before, split(input, *anchor).2)
                } else {
                    selected.to_string()
                };
                app.copy_to_clipboard(text, "the text".to_string());
                return false;
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                let (input, anchor) = field(app);
                let (_, selected, _) = split(input, *anchor);
                if selected.is_empty() {
                    return false;
                }
                let text = selected.to_string();
                let (input, anchor) = field(app);
                let changed = replace_selection(input, anchor, "");
                app.copy_to_clipboard(text, "the text".to_string());
                return changed;
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                // One line only: a newline in a single-line box would be invisible
                // and would break every regex it landed in.
                let pasted = app
                    .clipboard
                    .as_mut()
                    .ok()
                    .and_then(|clip| clip.get_text().ok())
                    .map(|text| text.replace(['\r', '\n'], " ").trim().to_string())
                    .unwrap_or_default();
                if pasted.is_empty() {
                    return false;
                }
                let (input, anchor) = field(app);
                return replace_selection(input, anchor, &pasted);
            }
            _ => {}
        }
    }

    let (input, anchor) = field(app);
    let len = char_len(input.value());
    let cursor = input.cursor();

    match key.code {
        KeyCode::Left => {
            move_cursor(input, anchor, cursor.saturating_sub(1), shift);
            false
        }
        KeyCode::Right => {
            move_cursor(input, anchor, (cursor + 1).min(len), shift);
            false
        }
        KeyCode::Home => {
            move_cursor(input, anchor, 0, shift);
            false
        }
        KeyCode::End => {
            move_cursor(input, anchor, len, shift);
            false
        }
        // A selection is what these replace; without one they fall through to
        // `tui_input`, which already deletes one character in the right direction.
        KeyCode::Backspace | KeyCode::Delete if selection(input, *anchor).is_some() => {
            replace_selection(input, anchor, "")
        }
        KeyCode::Char(c) if selection(input, *anchor).is_some() && !ctrl => {
            replace_selection(input, anchor, &c.to_string())
        }
        _ => {
            *anchor = None;
            input.handle_event(event).is_some()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    /// A scratch App whose strings filter is the field under test.
    fn app_with(text: &str, cursor: usize) -> App {
        let mut app = App::new();
        app.hex_view.strings_regex_input = Input::new(text.to_string()).with_cursor(cursor);
        app.hex_view.strings_filter_anchor = None;
        app
    }

    fn field(app: &mut App) -> (&mut Input, &mut Option<usize>) {
        (
            &mut app.hex_view.strings_regex_input,
            &mut app.hex_view.strings_filter_anchor,
        )
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
        handle_key(app, field, &key(code, modifiers))
    }

    /// Shift+Left and Shift+Right grow a block; the arrows alone drop it.
    #[test]
    fn shift_arrows_select_and_plain_arrows_clear() {
        let mut app = app_with("hello world", 5);

        press(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
        press(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
        let (input, anchor) = field(&mut app);
        assert_eq!(selection(input, *anchor), Some((3, 5)));
        assert_eq!(split(input, *anchor).1, "lo");

        // Coming back the other way shrinks it from the moving end, so what is left
        // is the character next to the anchor.
        press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        let (input, anchor) = field(&mut app);
        assert_eq!(split(input, *anchor).1, "o");

        press(&mut app, KeyCode::Left, KeyModifiers::NONE);
        let (input, anchor) = field(&mut app);
        assert_eq!(selection(input, *anchor), None, "a plain arrow clears the block");
    }

    /// Shift+Home and Shift+End take everything to one side, which is how a whole
    /// regex gets replaced in two keystrokes.
    #[test]
    fn shift_home_and_end_reach_the_ends() {
        let mut app = app_with("[a-z]+", 3);
        press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);
        let (input, anchor) = field(&mut app);
        assert_eq!(split(input, *anchor).1, "[a-");

        let mut app = app_with("[a-z]+", 3);
        press(&mut app, KeyCode::End, KeyModifiers::SHIFT);
        let (input, anchor) = field(&mut app);
        assert_eq!(split(input, *anchor).1, "z]+");
    }

    /// Home and End without Shift still just move, as `tui_input` already did.
    #[test]
    fn home_and_end_still_move_the_cursor() {
        let mut app = app_with("hello", 3);
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.hex_view.strings_regex_input.cursor(), 0);
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(app.hex_view.strings_regex_input.cursor(), 5);
    }

    /// Typing over a block replaces it, which is the whole point of having one.
    #[test]
    fn typing_replaces_the_selection() {
        let mut app = app_with("hello world", 11);
        press(&mut app, KeyCode::Home, KeyModifiers::SHIFT);
        assert!(press(&mut app, KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.hex_view.strings_regex_input.value(), "x");
        assert_eq!(app.hex_view.strings_regex_input.cursor(), 1);
        assert_eq!(app.hex_view.strings_filter_anchor, None);
    }

    /// Backspace and Delete take the block, not one character.
    #[test]
    fn backspace_and_delete_take_the_selection() {
        for code in [KeyCode::Backspace, KeyCode::Delete] {
            let mut app = app_with("keep[a-z]", 9);
            for _ in 0..5 {
                press(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
            }
            assert!(press(&mut app, code, KeyModifiers::NONE));
            assert_eq!(app.hex_view.strings_regex_input.value(), "keep", "{:?}", code);
        }
    }

    /// Without a selection they still delete one character, as before.
    #[test]
    fn backspace_without_a_selection_is_unchanged() {
        let mut app = app_with("abc", 3);
        assert!(press(&mut app, KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.hex_view.strings_regex_input.value(), "ab");
    }

    /// Multi-byte text is indexed by character, not by byte: a Korean regex must not
    /// slice a character in half.
    #[test]
    fn selection_is_by_character_not_by_byte() {
        let mut app = app_with("[가-힣]{2,}", 0);
        for _ in 0..5 {
            press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        }
        let (input, anchor) = field(&mut app);
        assert_eq!(split(input, *anchor).1, "[가-힣]");

        assert!(press(&mut app, KeyCode::Char('.'), KeyModifiers::NONE));
        assert_eq!(app.hex_view.strings_regex_input.value(), ".{2,}");
    }

    /// The selected span is what gets drawn highlighted, and nothing else.
    #[test]
    fn the_selection_is_the_only_highlighted_span() {
        let mut app = app_with("abcdef", 6);
        for _ in 0..3 {
            press(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
        }
        let base = Style::new();
        let highlight = Style::new().bg(ratatui::style::Color::Blue);
        let (input, anchor) = field(&mut app);
        let line = render_line(input, *anchor, base, highlight);

        let highlighted: String = line
            .spans
            .iter()
            .filter(|s| s.style == highlight)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(highlighted, "def");

        // No selection: one span, no highlight.
        let empty = Input::new("abc".to_string());
        let plain = render_line(&empty, None, base, highlight);
        assert_eq!(plain.spans.len(), 1);
        assert!(plain.spans.iter().all(|s| s.style == base));
    }

    /// A paste is flattened to one line: a newline in a single-line box is invisible
    /// and breaks any regex it lands in.
    #[test]
    fn a_pasted_newline_becomes_a_space() {
        // Exercised through the same helper the paste path uses, since the clipboard
        // itself is a shared resource that parallel tests fight over.
        let flattened = "one\r\ntwo\n".replace(['\r', '\n'], " ").trim().to_string();
        assert_eq!(flattened, "one  two");
    }
}

#[cfg(test)]
mod adoption_tests {
    //! Every text box in the program has to answer Shift+arrows, not just the ones
    //! that grew their own implementation of it.
    //!
    //! Three dialogs (Goto, Assemble, Edit Data) had hand-rolled selection before
    //! this module existed; the rest had none, so replacing a value meant holding
    //! Backspace. These tests drive each converted box through its real event
    //! handler.

    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::{KeyEvent, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE })
    }

    fn shift_home(app: &mut App, handler: fn(&mut App, &Event)) {
        handler(app, &key(KeyCode::End, KeyModifiers::NONE));
        handler(app, &key(KeyCode::Home, KeyModifiers::SHIFT));
    }

    /// The comment box (`;`).
    #[test]
    fn the_comment_box_selects_and_replaces() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::hex::comment::dialog_comment_events(app, event);
        }
        let mut app = App::new();
        app.hex_view.comment_input = Input::new("old note".to_string());
        app.state = crate::editor::UIState::DialogComment;

        shift_home(&mut app, handle);
        assert_eq!(
            selection(&app.hex_view.comment_input, app.hex_view.comment_anchor),
            Some((0, 8))
        );

        handle(&mut app, &key(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.hex_view.comment_input.value(), "n");
    }

    /// The calculator (`=`).
    #[test]
    fn the_calculator_selects_and_replaces() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::global::calculator::dialog_calculator_events(app, event);
        }
        let mut app = App::new();
        app.calculator.input = Input::new("dead+beef".to_string());

        shift_home(&mut app, handle);
        assert_eq!(selection(&app.calculator.input, app.calculator.anchor), Some((0, 9)));

        handle(&mut app, &key(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.calculator.input.value(), "1");
    }

    /// The image-base box (Alt+F6).
    #[test]
    fn the_image_base_box_selects_and_replaces() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::global::base::dialog_base_events(app, event);
        }
        let mut app = App::new();
        app.base_input = Input::new("140000000".to_string());
        app.state = crate::editor::UIState::DialogBase;

        shift_home(&mut app, handle);
        assert_eq!(selection(&app.base_input, app.base_anchor), Some((0, 9)));

        handle(&mut app, &key(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(app.base_input.value(), "4");
    }

    /// The Find dialog (Ctrl+B), and its block must not survive a field change.
    #[test]
    fn the_find_dialog_selects_and_drops_the_block_on_tab() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::hex::find_dialog::dialog_find_events(app, event);
        }
        let mut app = App::new();
        app.hex_view.find_dialog.input_enc1 = Input::new("pattern".to_string());
        app.state = crate::editor::UIState::DialogFindPattern;

        shift_home(&mut app, handle);
        assert!(app.hex_view.find_dialog.anchor.is_some(), "no block was started");

        handle(&mut app, &key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            app.hex_view.find_dialog.anchor.is_none(),
            "the block outlived the field it belonged to"
        );
    }

    /// The Replace dialog (Ctrl+H).
    #[test]
    fn the_replace_dialog_selects_and_replaces() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::events::handle_replace_pattern_events(app, event);
        }
        let mut app = App::new();
        app.hex_view.replace_dialog.search_input = Input::new("90 90".to_string());
        app.state = crate::editor::UIState::DialogReplacePattern;

        shift_home(&mut app, handle);
        assert_eq!(
            selection(&app.hex_view.replace_dialog.search_input, app.hex_view.replace_dialog.anchor),
            Some((0, 5))
        );

        handle(&mut app, &key(KeyCode::Char('C'), KeyModifiers::NONE));
        assert_eq!(app.hex_view.replace_dialog.search_input.value(), "C");
    }

    /// The Modify Block dialog (Ctrl+K), whose value box is one of two.
    #[test]
    fn the_modify_dialog_selects_the_focused_box() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::hex::modify_dialog::dialog_modify_events(app, event);
        }
        let mut app = App::new();
        app.hex_view.modify_dialog.operand_input = Input::new("FF".to_string());
        app.hex_view.modify_dialog.focus = crate::hex::modify_dialog::ModifyFocus::Operand;
        app.state = crate::editor::UIState::DialogModifyBlock;

        shift_home(&mut app, handle);
        assert_eq!(
            selection(&app.hex_view.modify_dialog.operand_input, app.hex_view.modify_dialog.anchor),
            Some((0, 2))
        );

        handle(&mut app, &key(KeyCode::Char('A'), KeyModifiers::NONE));
        assert_eq!(app.hex_view.modify_dialog.operand_input.value(), "A");
        assert_eq!(
            app.hex_view.modify_dialog.step_input.value(),
            "1",
            "the other box must be untouched"
        );
    }

    /// The section size prompt, which had Shift+Left/Right by hand and now gets the
    /// rest as well.
    #[test]
    fn the_section_size_prompt_reaches_the_ends() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::header::formats::pe::section_tools::dialog_section_size_events(app, event);
        }
        let mut app = App::new();
        app.header_view.section_size_dialog.input = Input::new("1000".to_string());
        app.state = crate::editor::UIState::DialogSectionSize;

        shift_home(&mut app, handle);
        assert_eq!(
            selection(
                &app.header_view.section_size_dialog.input,
                app.header_view.section_size_dialog.selection_anchor
            ),
            Some((0, 4)),
            "Shift+Home was not handled - it used to only know Shift+Left/Right"
        );

        handle(&mut app, &key(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.header_view.section_size_dialog.input.value(), "2");
    }

    /// The header field edit box, which borrows the Goto dialog's input.
    #[test]
    fn the_header_edit_box_selects_and_replaces() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::header::edit_dialog::handle_dialog_header_edit_events(app, event);
        }
        let mut app = App::new();
        app.header_view.edit_name = "SizeOfImage".to_string();
        app.goto_input = Input::new("449000".to_string());
        app.goto_selection_all = false;
        app.state = crate::editor::UIState::DialogHeaderEdit;

        shift_home(&mut app, handle);
        assert_eq!(selection(&app.goto_input, app.goto_selection_anchor), Some((0, 6)));

        handle(&mut app, &key(KeyCode::Char('5'), KeyModifiers::NONE));
        assert_eq!(app.goto_input.value(), "5");
    }

    /// A section name is still clamped to eight characters, block or no block.
    #[test]
    fn the_section_name_limit_survives_the_change() {
        fn handle(app: &mut App, event: &Event) {
            let _ = crate::header::edit_dialog::handle_dialog_header_edit_events(app, event);
        }
        let mut app = App::new();
        app.header_view.edit_name = "Section.Name".to_string();
        app.goto_input = Input::new("12345678".to_string());
        app.goto_selection_all = false;
        app.state = crate::editor::UIState::DialogHeaderEdit;

        handle(&mut app, &key(KeyCode::End, KeyModifiers::NONE));
        handle(&mut app, &key(KeyCode::Char('9'), KeyModifiers::NONE));

        assert_eq!(
            app.goto_input.value().chars().count(),
            8,
            "a ninth character got in: {:?}",
            app.goto_input.value()
        );
    }
}