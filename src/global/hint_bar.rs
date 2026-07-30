//! Bottom hint line: the keys that are useful *right now*.
//!
//! dz6 has more shortcuts than anyone keeps in their head, and the only place
//! that listed them was the F1 dialog - which you have to know to open, and which
//! covers the whole screen once you do. This is the Hiew / Midnight Commander
//! answer: a single line of the six to ten keys that apply to the current view and
//! mode, always on screen.
//!
//! Two design decisions worth knowing about:
//!
//! * It shares the command-bar row instead of taking one of its own. That row is
//!   empty except while `:` is being typed or a message is showing, and on an
//!   80x25 console the hex view cannot spare another line (ruler + status +
//!   command bar already cost three). `draw` gives the row to the command line
//!   first, then to a message, then to the hints.
//! * Holding Ctrl or Alt swaps the line for that modifier's bindings. A terminal
//!   does not report a modifier being held on its own, so this is read from the
//!   keyboard directly on Windows (`GetAsyncKeyState`) and simply never triggers
//!   elsewhere, where the plain page stays put.

use ratatui::{
    Frame,
    layout::Rect,
    prelude::*,
    widgets::{Clear, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    editor::{AppView, UIState},
    i18n::M,
};

/// Which set of bindings the line is showing.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum HintPage {
    /// Keys that need no modifier, chosen by view and mode.
    Plain,
    /// Everything on Ctrl.
    Ctrl,
    /// Everything on Alt.
    Alt,
}

/// One slot on the line.
struct Hint {
    /// Key as the user has to press it. Never translated - it is what is printed on
    /// the keyboard.
    key: &'static str,
    /// What it does, in as few columns as stay readable.
    label: Label,
    /// Shorter wording, used when the full label would push the slot off the line.
    ///
    /// Without this a longer label does not shorten, it disappears: slots that do
    /// not fit are dropped whole. `F12 Save and quit` reads better and fits a wide
    /// terminal, but on an 100-column one it cost the slot altogether.
    short: Option<Label>,
    /// True for actions a read-only file refuses, so they can be dimmed before
    /// the user finds out by pressing them.
    needs_write: bool,
}

/// Shorthand for a hint that works on any file.
const fn ro(key: &'static str, label: M) -> Hint {
    Hint { key, label: Label::Msg(label), short: None, needs_write: false }
}

/// As [`ro`], with a shorter wording for when the line is tight.
const fn ro_short(key: &'static str, label: M, short: M) -> Hint {
    Hint {
        key,
        label: Label::Msg(label),
        short: Some(Label::Msg(short)),
        needs_write: false,
    }
}

/// Shorthand for a hint that needs a writable file.
const fn rw(key: &'static str, label: M) -> Hint {
    Hint { key, label: Label::Msg(label), short: None, needs_write: true }
}

/// A slot whose label is a literal value rather than a word - `00`, `90` and the
/// like read the same in every language.
const fn raw(key: &'static str, literal: &'static str, needs_write: bool) -> Hint {
    Hint { key, label: Label::Raw(literal), short: None, needs_write }
}

/// What a slot says.
#[derive(Copy, Clone)]
enum Label {
    /// A translated word.
    Msg(M),
    /// A value that is not language-dependent.
    Raw(&'static str),
}

impl Label {
    fn text(self, lang: crate::i18n::Lang) -> &'static str {
        match self {
            Label::Msg(m) => m.tr(lang),
            Label::Raw(text) => text,
        }
    }
}

/// Modifier currently held down, or `Plain` when none is.
///
/// `GetAsyncKeyState`'s high bit means "down right now", which is exactly the
/// question being asked. It is a cheap register read - no window message round
/// trip - so calling it once per frame costs nothing worth measuring.
#[cfg(target_os = "windows")]
pub fn held_page() -> HintPage {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(vKey: i32) -> i16;
    }

    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12; // Alt
    const DOWN: u16 = 0x8000;

    unsafe {
        // Alt is tested first: Alt+Ctrl combinations are not bound, and the Alt
        // page is the smaller of the two, so it is the more useful answer.
        if (GetAsyncKeyState(VK_MENU) as u16) & DOWN != 0 {
            return HintPage::Alt;
        }
        if (GetAsyncKeyState(VK_CONTROL) as u16) & DOWN != 0 {
            return HintPage::Ctrl;
        }
    }
    HintPage::Plain
}

/// Non-Windows terminals cannot report a modifier held on its own, so the plain
/// page is all there is. F1 still lists the Ctrl and Alt bindings in full.
#[cfg(not(target_os = "windows"))]
pub fn held_page() -> HintPage {
    HintPage::Plain
}

/// True when the hint line should be given the command-bar row.
///
/// Anything that draws into that row itself - the command line, an error message,
/// a dialog - takes precedence, so the hints are only offered the row when the
/// user is actually in a view.
pub fn should_show(app: &App) -> bool {
    if !app.config.hint_bar || app.status_error.is_some() {
        return false;
    }
    // `dialog_renderer` is how the command line and most dialogs draw; two dialogs
    // key off the state instead, hence the explicit list below.
    if app.dialog_renderer.is_some() {
        return false;
    }
    matches!(
        app.state,
        UIState::Normal | UIState::Error | UIState::HexSelection | UIState::HexEditing
    )
}

/// Hints for the current view, mode and held modifier.
fn hints_for(app: &App, page: HintPage) -> Vec<Hint> {
    match page {
        HintPage::Ctrl => vec![
            ro("C", M::Copy),
            rw("E", M::Data),
            ro("G", M::Goto),
            ro("B", M::Find),
            rw("K", M::Modify),
            rw("H", M::Replace),
            ro("R", M::Xref),
            ro("X", M::Addr),
            rw("Z", M::Undo),
            rw("Y", M::Redo),
            ro("O", M::Open),
        ],
        HintPage::Alt => vec![
            ro("E", M::Encoding),
            ro("Shift+E", M::Encoding2),
            ro("H", M::Highlight),
            ro("L", M::Log),
            ro("M", M::Color),
            ro("N", M::Names),
            raw("F2", "VA", false),
            rw("F3", M::RevertByte),
            ro("F6", M::ImageBase),
            ro("F7", M::DecodeWidth),
        ],
        HintPage::Plain => plain_hints(app),
    }
}

/// The no-modifier page, which is the one that has to follow the mode.
fn plain_hints(app: &App) -> Vec<Hint> {
    // Edit mode first: it is a mode, not a view, and what it accepts has nothing
    // in common with the others.
    if app.state == UIState::HexEditing {
        return vec![
            ro("0-9A-F", M::Type),
            ro("Tab", M::Column),
            rw("~", M::Case),
            ro("Shift+Arrows", M::Select),
            ro("Esc", M::Done),
        ];
    }

    // A live block changes what most keys mean, whether it was made with Shift,
    // the mouse, or left over from either.
    let has_selection = app.hex_view.selection.start != app.hex_view.selection.end;
    if app.editor_view != AppView::Header
        && (app.state == UIState::HexSelection || has_selection)
    {
        return vec![
            ro("Ctrl+C", M::Copy),
            raw("Ins", "00", true),
            raw("Del", "NOP", true),
            rw("Ctrl+K", M::Modify),
            rw("~", M::Case),
            ro("Alt+M", M::Color),
            ro("Esc", M::Clear),
        ];
    }

    match app.editor_view {
        // Function keys only, in number order. Everything on Ctrl or Alt is one
        // modifier press away and has a page of its own; a row of F-keys in numeric
        // order is scannable in a way a mixed list is not, and the number *is* the
        // position, so the eye can go straight to a slot.
        //
        // Tab (switch view) and Shift+V (paste) are absent on purpose: Tab is
        // learned in two presses, and there is no Shift page for paste to live on -
        // Shift is held throughout a Shift+arrow selection, so a Shift page would
        // replace the selection hints exactly when they are needed. Both are in F1.
        AppView::Hex => vec![
            ro("F1", M::Help),
            rw("F2", M::Edit),
            ro("F4", M::HeaderView),
            ro("F5", M::Refs),
            ro("F6", M::Strings),
            ro("F7", M::TextView),
            ro("F8", M::About),
            ro("F9", M::Open),
            ro_short("F12", M::SaveQuit, M::Save),
        ],
        AppView::Disasm => vec![
            ro("F1", M::Help),
            ro("F4", M::HeaderView),
            ro("F5", M::Refs),
            ro("F6", M::Strings),
            ro("F7", M::TextView),
            ro("F8", M::About),
            ro("F9", M::Open),
            ro_short("F12", M::SaveQuit, M::Save),
        ],
        AppView::Text => vec![
            ro("F1", M::Help),
            ro("F4", M::HeaderView),
            ro("F6", M::Strings),
            ro("F7", M::TextView),
            ro("F8", M::About),
            ro("F9", M::Open),
            ro_short("F12", M::SaveQuit, M::Save),
        ],
        AppView::Header => vec![
            ro("F1", M::Help),
            ro("F4", M::HexView),
            ro("F5", M::Refs),
            ro("F6", M::Strings),
            ro("F7", M::TextView),
            ro("F8", M::About),
            ro("F9", M::Open),
            ro_short("F12", M::SaveQuit, M::Save),
        ],
    }
}

/// Prefix shown on the modifier pages, so a line of bare letters can't be
/// mistaken for keys that work on their own.
fn page_prefix(page: HintPage) -> Option<&'static str> {
    match page {
        HintPage::Ctrl => Some("Ctrl"),
        HintPage::Alt => Some("Alt"),
        HintPage::Plain => None,
    }
}

const SEPARATOR: &str = " │ ";

pub fn hint_bar_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (spans, _used) = build_line(app, held_page(), area.width as usize);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(app.config.theme.main),
        area,
    );
}

/// Builds the line for `page`, dropping slots that do not fit in `budget`
/// columns, and returns it with the number of columns it occupies.
///
/// Separate from the drawing so the width arithmetic can be tested: rendering into
/// a fixed-size buffer cannot overflow, it silently clips, which is exactly the
/// failure this needs to catch.
fn build_line(app: &App, page: HintPage, budget: usize) -> (Vec<Span<'static>>, usize) {
    let hints = hints_for(app, page);

    let base = app.config.theme.main;
    // `offsets` is the theme's accent on the main background - the address column
    // uses it - so keys stand out without the line turning into a colour test.
    // `byte_highlight` would have been white-on-red, far too loud for a row that is
    // always on screen.
    let key_style = app.config.theme.offsets;
    let dim = app.config.theme.dimmed;
    let read_only = app.file_info.is_read_only;

    let lang = app.config.lang;

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(hints.len() * 4 + 2);
    // Budget in terminal columns, measured with `unicode_width`: a Korean or Chinese
    // label is one character but two columns, so a character count would let the
    // line overrun the row and clip mid-label.
    let mut used = 0usize;

    if let Some(prefix) = page_prefix(page) {
        let text = format!("{}: ", prefix);
        used += UnicodeWidthStr::width(text.as_str());
        spans.push(Span::styled(text, key_style));
    }

    for (i, hint) in hints.iter().enumerate() {
        let sep_len = UnicodeWidthStr::width(SEPARATOR) * usize::from(i > 0);
        let width_of = |text: &str| UnicodeWidthStr::width(hint.key) + 1 + UnicodeWidthStr::width(text);

        // The full wording if it fits, the shorter one if not. Whole slots are
        // dropped rather than the line being cut mid-word: a truncated hint is
        // worse than a missing one.
        let mut label = hint.label.text(lang);
        if used + sep_len + width_of(label) > budget
            && let Some(short) = hint.short
        {
            label = short.text(lang);
        }
        let item_len = width_of(label);
        if used + sep_len + item_len > budget {
            break;
        }
        used += sep_len + item_len;

        if i > 0 {
            spans.push(Span::styled(SEPARATOR, dim));
        }
        // A dimmed slot means "this file is read-only, that action is refused" -
        // the same information the status bar's `Read Only` marker carries, but at
        // the point of use.
        let disabled = read_only && hint.needs_write;
        spans.push(Span::styled(
            hint.key,
            if disabled { dim } else { key_style },
        ));
        spans.push(Span::styled(" ", base));
        spans.push(Span::styled(label, if disabled { dim } else { base }));
    }

    (spans, used)
}

#[cfg(test)]
mod hint_bar_tests {
    use super::*;

    fn app_in(view: AppView, state: UIState) -> App {
        let mut app = App::new();
        app.config.database = false;
        app.editor_view = view;
        app.state = state;
        app
    }

    /// A long label shortens rather than disappearing.
    ///
    /// Slots that do not fit are dropped whole, so giving F12 the fuller wording
    /// cost the slot entirely at 100 columns - the key vanished from the line rather
    /// than being described in fewer words.
    #[test]
    fn a_long_label_falls_back_instead_of_dropping_the_slot() {
        let app = app_in(AppView::Hex, UIState::Normal);

        let text_at = |budget: usize| {
            let (spans, _) = build_line(&app, HintPage::Plain, budget);
            spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        };

        let wide = text_at(120);
        assert!(
            wide.contains("F12 Save and quit"),
            "the full wording is missing where it fits: {:?}",
            wide
        );

        let tight = text_at(100);
        assert!(
            tight.contains("F12 Save") && !tight.contains("Save and quit"),
            "the slot should have shortened, got: {:?}",
            tight
        );

        // Narrower still: dropping it is fine, but nothing may be cut mid-word.
        for budget in 60..=130 {
            let line = text_at(budget);
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= budget,
                "the line is {} columns in a budget of {}: {:?}",
                UnicodeWidthStr::width(line.as_str()),
                budget,
                line
            );
        }
    }

    /// Every view has something to show, and the line has to fit the narrowest
    /// terminal dz6 runs in (68 columns).
    #[test]
    fn every_page_has_hints_and_a_sane_width() {
        let mut app = app_in(AppView::Hex, UIState::Normal);

        for view in [AppView::Hex, AppView::Disasm, AppView::Text, AppView::Header] {
            app.editor_view = view;
            let hints = hints_for(&app, HintPage::Plain);
            assert!(!hints.is_empty(), "{:?} has no hints", view);
            assert!(hints.len() <= 10, "{:?} shows too many slots", view);
        }

        for page in [HintPage::Ctrl, HintPage::Alt] {
            let hints = hints_for(&app, page);
            assert!(!hints.is_empty(), "{:?} has no hints", page);
        }
    }

    /// Function keys lead the line, in number order, and nothing that is learned in
    /// two presses takes a slot.
    #[test]
    fn function_keys_come_first_and_in_order() {
        let mut app = app_in(AppView::Hex, UIState::Normal);

        for view in [AppView::Hex, AppView::Disasm, AppView::Text] {
            app.editor_view = view;
            let hints = hints_for(&app, HintPage::Plain);

            let numbers: Vec<u32> = hints
                .iter()
                .filter_map(|h| h.key.strip_prefix('F'))
                .filter_map(|n| n.parse().ok())
                .collect();
            assert!(numbers.len() >= 4, "{:?} shows too few F-keys", view);
            assert!(
                numbers.windows(2).all(|w| w[0] < w[1]),
                "{:?} lists F-keys out of order: {:?}",
                view,
                numbers
            );

            // The F-keys must be the leading run, since narrow terminals drop
            // slots from the end.
            let first_non_f = hints
                .iter()
                .position(|h| !h.key.starts_with('F') || h.key.len() > 3)
                .unwrap_or(hints.len());
            assert!(first_non_f >= numbers.len(), "{:?} interleaves other keys", view);

            assert!(
                !hints.iter().any(|h| h.key == "Tab" || h.key == "Shift+V"),
                "{:?} still spends a slot on Tab or paste",
                view
            );
        }
    }

    /// Edit mode and selection mode must not show the Normal-mode line.
    #[test]
    fn the_mode_decides_the_page() {
        let app = app_in(AppView::Hex, UIState::HexEditing);
        let editing = hints_for(&app, HintPage::Plain);
        assert!(
            editing.iter().any(|h| h.key == "0-9A-F"),
            "edit mode should lead with typing"
        );

        let mut app = app_in(AppView::Hex, UIState::HexSelection);
        app.hex_view.selection.start = 0x10;
        app.hex_view.selection.end = 0x20;
        let selecting = hints_for(&app, HintPage::Plain);
        assert!(
            selecting
                .iter()
                .any(|h| h.label.text(crate::i18n::Lang::En) == "Copy"),
            "a block should offer Copy"
        );
    }

    /// A leftover Shift selection in Normal state still switches the line: the
    /// block is on screen, so the keys that act on it are the relevant ones.
    #[test]
    fn a_live_selection_switches_the_page_in_normal_state() {
        let mut app = app_in(AppView::Hex, UIState::Normal);
        app.hex_view.selection.start = 0;
        app.hex_view.selection.end = 4;

        let hints = hints_for(&app, HintPage::Plain);
        assert!(
            hints
                .iter()
                .any(|h| h.label.text(crate::i18n::Lang::En) == "Clear")
        );
    }

    /// The row belongs to the command line, messages and dialogs first.
    #[test]
    fn dialogs_and_messages_take_the_row() {
        let mut app = app_in(AppView::Hex, UIState::Normal);
        assert!(should_show(&app), "a plain view should show hints");

        app.status_error = Some("something went wrong".to_string());
        assert!(!should_show(&app), "a message wins");
        app.status_error = None;

        app.dialog_renderer = Some(crate::hex::help::dialog_help_draw);
        assert!(!should_show(&app), "an open dialog wins");
        app.dialog_renderer = None;

        app.state = UIState::Command;
        assert!(!should_show(&app), "the command line wins");
        app.state = UIState::Normal;

        app.config.hint_bar = false;
        assert!(!should_show(&app), "':set hintbar off' must silence it");
    }
}

#[cfg(test)]
mod hint_bar_render_tests {
    use super::*;
    use crate::i18n::Lang;

    fn app_for(lang: Lang) -> App {
        let mut app = App::new();
        app.config.database = false;
        app.config.lang = lang;
        app.editor_view = AppView::Hex;
        app.state = UIState::Normal;
        app
    }

    /// Assembles the line the way `hint_bar_draw` does and returns its text.
    fn line_text(app: &App, width: usize) -> (String, usize) {
        let (spans, used) = build_line(app, HintPage::Plain, width);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        (text, used)
    }

    /// The line must fit the row in every language.
    ///
    /// Korean and Chinese labels are one character but two columns wide. Measured by
    /// character count, the Korean row came out nine columns over the width of a
    /// 68-column terminal, which clips a label in half - the widths are counted in
    /// display columns instead.
    #[test]
    fn every_language_fits_every_width() {
        for lang in Lang::ALL {
            let app = app_for(lang);
            for width in [68usize, 80, 100, 120] {
                let (text, used) = line_text(&app, width);
                assert_eq!(
                    used,
                    UnicodeWidthStr::width(text.as_str()),
                    "{} at {}: the budget and the text disagree",
                    lang.name(),
                    width
                );
                assert!(
                    used <= width,
                    "{} at {} columns needs {}: {:?}",
                    lang.name(),
                    width,
                    used,
                    text
                );
                assert!(!text.is_empty(), "{} at {} drew nothing", lang.name(), width);
            }
        }
    }

    /// The words follow the language; the key names do not.
    #[test]
    fn labels_are_translated_and_keys_are_not() {
        let (en, _) = line_text(&app_for(Lang::En), 120);
        assert!(en.contains("F1 Help"), "got: {:?}", en);

        let (ko, _) = line_text(&app_for(Lang::Ko), 120);
        assert!(ko.contains("F1 도움말"), "got: {:?}", ko);
        assert!(ko.contains("F12"), "key names must not be translated: {:?}", ko);

        let (zh, _) = line_text(&app_for(Lang::Zh), 120);
        assert!(zh.contains("F1 帮助"), "got: {:?}", zh);
    }
}
