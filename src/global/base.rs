//! Set Image Base dialog (Alt+F6).
//!
//! Every address dz6 shows is `base + rva`, and the base normally comes from the
//! header. That is wrong in three common situations: a memory dump taken from a
//! relocated module, a file whose header has been tampered with, and a raw
//! shellcode blob that has no header at all. Overriding the base makes the
//! addresses in the listing line up with a debugger's, which is what makes the
//! cross references, Follow and `:goto` usable on such a file.

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::Alignment,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::io::Result;

use crate::{app::App, editor::UIState, util::center_widget};

const BASE_DIALOG_WIDTH: u16 = 44;

/// Opens the dialog pre-filled with the base in effect, fully selected so typing
/// replaces it outright.
/// The image-base box and its selection anchor.
fn base_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (&mut app.base_input, &mut app.base_anchor)
}

pub fn open_base_dialog(app: &mut App) {
    let current = format!("{:X}", app.get_image_base());
    let cursor = current.chars().count();
    app.base_input = tui_input::Input::new(current).with_cursor(cursor);
    app.base_selection_all = true;
    app.state = UIState::DialogBase;
    app.dialog_renderer = Some(dialog_base_draw);
}

/// Applies `text` as the new base, or clears the override when it is empty.
///
/// Returns the message to log, or an error describing why nothing changed.
pub fn apply_base(app: &mut App, text: &str) -> std::result::Result<String, String> {
    let clean = text.trim();

    if clean.is_empty() {
        app.image_base_override = None;
        refresh_after_base_change(app);
        return Ok(format!(
            "Image base reset to the file's own value 0x{:X}",
            app.get_image_base()
        ));
    }

    // Hex by default with `t` for decimal, which is `util::parse_offset`'s rule and
    // therefore `:goto`'s. The `0x`/`h` decorations are stripped here because
    // `parse_offset` does not take them, and a base is normally pasted from a
    // debugger, which prints them.
    let normalized = {
        let no_prefix = clean
            .strip_prefix("0x")
            .or_else(|| clean.strip_prefix("0X"))
            .unwrap_or(clean);
        // Only for hex spellings: `10th` is not a number, and stripping the `h`
        // from a decimal `10t` would be wrong anyway.
        if no_prefix.ends_with('t') || no_prefix.ends_with('T') {
            no_prefix.to_string()
        } else {
            no_prefix
                .strip_suffix('h')
                .or_else(|| no_prefix.strip_suffix('H'))
                .unwrap_or(no_prefix)
                .to_string()
        }
    };

    let Ok(value) = crate::util::parse_offset(&normalized) else {
        return Err(format!("'{}' is not an address", clean));
    };

    app.image_base_override = Some(value as u64);
    refresh_after_base_change(app);
    Ok(format!("Image base set to 0x{:X}", value as u64))
}

/// Rebuilds everything derived from the base.
///
/// The import labels are keyed by absolute address and the disassembly caches
/// rendered rows with their addresses baked in, so without this the listing would
/// keep showing values computed from the previous base.
fn refresh_after_base_change(app: &mut App) {
    app.import_labels.clear();
    if let Some(pe) = app.header_view.pe.as_ref() {
        let base = app.get_image_base();
        app.import_labels = crate::disasm::imports::build_labels(pe, base);
    }

    app.view_generation = app.view_generation.wrapping_add(1);
}

pub fn dialog_base_draw(app: &mut App, frame: &mut Frame) {
    // Above centre, like Goto and Assemble: the line whose address is being
    // reinterpreted is usually mid-screen, and a box centred exactly there covers
    // it.
    let width = BASE_DIALOG_WIDTH
        .min(frame.area().width.saturating_sub(4))
        .max(24);
    let mut area = center_widget(width, 3, frame.area());
    area.y = area.y.saturating_sub(4);

    frame.render_widget(Clear, area);

    let value = app.base_input.value();
    // The file's own value is worth showing while an override is active: it is what
    // clearing the field goes back to, and there is otherwise no way to see it.
    let base_label = crate::i18n::M::ImageBaseTitle.tr(app.config.lang);
    let title = if app.image_base_override.is_some() {
        format!(" {} (file: {:X}) ", base_label, app.header_image_base())
    } else {
        format!(" {} ", base_label)
    };

    let block = Block::bordered()
        .title(title)
        .title_alignment(Alignment::Center);

    let paragraph = if app.base_selection_all && !value.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            value.to_string(),
            app.config.theme.highlight,
        )))
        .style(app.config.theme.dialog)
        .block(block)
    } else {
        Paragraph::new(crate::text_field::render_line(
            &app.base_input,
            app.base_anchor,
            app.config.theme.dialog,
            app.config.theme.highlight,
        ))
            .style(app.config.theme.dialog)
            .block(block)
    };

    frame.render_widget(paragraph, area);

    let cursor_x = area.x + 1 + app.base_input.visual_cursor() as u16;
    if cursor_x < area.x + area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, area.y + 1));
    }
}

pub fn dialog_base_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match key.code {
            KeyCode::Esc => {
                app.base_selection_all = false;
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            KeyCode::Enter => {
                app.base_selection_all = false;
                let text = app.base_input.value().to_string();
                match apply_base(app, &text) {
                    Ok(message) => App::log(app, message),
                    Err(reason) => {
                        crate::beep!();
                        App::log(app, format!("Image base unchanged: {}", reason));
                    }
                }
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            // Typing over a fully selected value replaces it, which is the point of
            // opening pre-selected.
            KeyCode::Char(c)
                if app.base_selection_all && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.base_selection_all = false;
                app.base_input = tui_input::Input::new(c.to_string()).with_cursor(1);
            }
            KeyCode::Backspace | KeyCode::Delete if app.base_selection_all => {
                app.base_selection_all = false;
                app.base_input = tui_input::Input::default();
            }
            _ => {
                app.base_selection_all = false;
                crate::text_field::handle_key(app, base_field, event);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A raw blob: no header, so the base is entirely up to the override. This is
    /// the shellcode case the feature exists for.
    fn app_with_blob() -> App {
        let dir = std::env::temp_dir().join("dz6_base");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("blob_{n}.bin"));
        std::fs::write(&path, vec![0x90u8; 0x200]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app
    }

    /// Alt+F6 opens the dialog from any view, pre-filled with the base in effect.
    #[test]
    fn alt_f6_opens_the_dialog_in_every_view() {
        use crate::editor::{AppView, UIState};
        use ratatui::crossterm::event::{
            KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };

        for view in [AppView::Hex, AppView::Disasm, AppView::Text, AppView::Header] {
            let mut app = app_with_blob();
            app.editor_view = view;
            app.state = UIState::Normal;

            let key = KeyEvent {
                code: KeyCode::F(6),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = crate::global::events::handle_global_events(&mut app, key);

            assert!(
                app.state == UIState::DialogBase,
                "Alt+F6 did not open the base dialog in {:?}",
                view
            );
            assert_eq!(
                app.base_input.value(),
                format!("{:X}", app.get_image_base()),
                "the box must open showing the base currently in effect"
            );
            assert!(app.base_selection_all, "the value must open selected so typing replaces it");
        }
    }

    /// Bare F6 is the strings list; it must not be swallowed by the base dialog.
    #[test]
    fn bare_f6_is_left_alone() {
        use crate::editor::UIState;
        use ratatui::crossterm::event::{
            KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };

        let mut app = app_with_blob();
        app.state = UIState::Normal;

        let key = KeyEvent {
            code: KeyCode::F(6),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::global::events::handle_global_events(&mut app, key);

        assert!(
            app.state != UIState::DialogBase,
            "the base dialog must require Alt"
        );
    }

    /// The box renders, sits above centre, and shows the base being replaced.
    #[test]
    fn the_dialog_renders_above_centre() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = app_with_blob();
        super::apply_base(&mut app, "140000000").expect("set");
        super::open_base_dialog(&mut app);

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                super::dialog_base_draw(&mut app, f);
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..24u16)
            .map(|y| (0..100u16).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();

        let border_row = rows
            .iter()
            .position(|r| r.contains("Image Base"))
            .expect("the titled border must be drawn");
        // A vertically centred 3-row box on a 24-row screen starts at row 10; this
        // one is lifted 4 so it does not cover the line being reinterpreted.
        assert!(
            border_row < 10,
            "the box must sit above centre, found its title on row {border_row}"
        );

        assert!(
            rows.iter().any(|r| r.contains("140000000")),
            "the base in effect must be shown for editing"
        );
        // The file's own value is what clearing the field goes back to, so it has to
        // be visible while an override is active.
        assert!(
            rows[border_row].contains("file:"),
            "expected the file's own base in the title, got: {}",
            rows[border_row].trim_end()
        );
    }

    /// Setting the base shifts every address the view computes.
    #[test]
    fn the_base_shifts_addresses() {
        let mut app = app_with_blob();
        assert_eq!(app.get_va(0x10), 0x10, "no header, no override: a plain offset");

        super::apply_base(&mut app, "140000000").expect("accepted");

        assert_eq!(app.get_image_base(), 0x1_4000_0000);
        assert_eq!(app.get_va(0x10), 0x1_4000_0010);
    }

    /// `va_to_offset` has to stay the inverse of `get_va`, or Follow and Xref land
    /// on the wrong byte.
    ///
    /// The headerless path used to return `va as usize`, treating the address as an
    /// offset outright - with a base set that is off by the whole base.
    #[test]
    fn address_translation_round_trips() {
        let mut app = app_with_blob();
        super::apply_base(&mut app, "400000").expect("accepted");

        for offset in [0usize, 1, 0x40, 0x1FF] {
            let va = app.get_va(offset);
            assert_eq!(
                app.va_to_offset(va),
                Some(offset),
                "0x{:X} -> VA 0x{:X} -> back",
                offset,
                va
            );
        }
    }

    /// Hex is the default spelling, `t` means decimal - as everywhere else in dz6.
    #[test]
    fn the_value_is_hex_by_default() {
        let mut app = app_with_blob();

        super::apply_base(&mut app, "10").expect("hex");
        assert_eq!(app.get_image_base(), 0x10);

        super::apply_base(&mut app, "10t").expect("decimal");
        assert_eq!(app.get_image_base(), 10);

        // Decorations a debugger prints are accepted, since the value is usually
        // pasted from one.
        super::apply_base(&mut app, "0x20").expect("0x prefix");
        assert_eq!(app.get_image_base(), 0x20);

        super::apply_base(&mut app, "30h").expect("h suffix");
        assert_eq!(app.get_image_base(), 0x30);

        super::apply_base(&mut app, "0x140000000").expect("full address");
        assert_eq!(app.get_image_base(), 0x1_4000_0000);
    }

    /// An empty value means "go back to the file's own base".
    #[test]
    fn an_empty_value_clears_the_override() {
        let mut app = app_with_blob();
        super::apply_base(&mut app, "140000000").expect("set");
        assert!(app.image_base_override.is_some());

        super::apply_base(&mut app, "   ").expect("cleared");
        assert!(app.image_base_override.is_none());
        assert_eq!(app.get_va(0x10), 0x10, "back to plain offsets");
    }

    /// Junk is refused and leaves the current base alone.
    #[test]
    fn junk_is_refused() {
        let mut app = app_with_blob();
        super::apply_base(&mut app, "140000000").expect("set");

        assert!(super::apply_base(&mut app, "zzz").is_err());
        assert_eq!(
            app.get_image_base(),
            0x1_4000_0000,
            "a rejected value must not disturb the current base"
        );
    }

    /// A base change must invalidate the disassembly row cache.
    #[test]
    fn changing_the_base_invalidates_the_disasm_cache() {
        let mut app = app_with_blob();
        let before = app.view_generation;
        super::apply_base(&mut app, "140000000").expect("set");
        assert_ne!(
            app.view_generation, before,
            "the cached rows still carry addresses from the old base"
        );
    }

    /// The override belongs to the image it was entered for.
    #[test]
    fn opening_another_file_clears_the_override() {
        let mut app = app_with_blob();
        super::apply_base(&mut app, "140000000").expect("set");

        let second = app_with_blob();
        let path = second.file_info.path.clone();
        drop(second);
        app.load_file(&path, 0, true).expect("open another");

        assert!(
            app.image_base_override.is_none(),
            "carrying the base over would translate the new file through the wrong layout"
        );
    }

    /// For a real PE the import labels are keyed by absolute address, so they have
    /// to be rebuilt against the new base.
    #[test]
    fn import_labels_follow_the_new_base() {
        let mut app = App::new();
        app.config.database = false;
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(exe) = exe.to_str() else { return };
        if app.load_file(exe, 0, true).is_err() {
            return;
        }
        if app.import_labels.is_empty() {
            return;
        }

        let original_base = app.get_image_base();
        let before: Vec<u64> = app.import_labels.keys().copied().collect();

        let new_base = original_base + 0x1000_0000;
        super::apply_base(&mut app, &format!("{:X}", new_base)).expect("set");

        assert_eq!(
            app.import_labels.len(),
            before.len(),
            "the same imports must still be labelled"
        );
        for key in before {
            assert!(
                !app.import_labels.contains_key(&key),
                "slot 0x{:X} is still keyed at the old base",
                key
            );
        }
    }
}
