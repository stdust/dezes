use ratatui::{
    Frame,
    layout::Alignment,
    widgets::{Block, Clear, Paragraph},
};

use ratatui::crossterm::event::{Event, KeyCode};
use serde::{Deserialize, Serialize};
use std::io::Result;


use crate::{app::App, commands::Commands, editor::UIState, util::center_widget};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub offset: usize,
    pub comment: String,
}

/// Width of the comment dialog, clamped to the terminal by `center_widget`.
const COMMENT_DIALOG_WIDTH: u16 = 60;

/// Opens the comment dialog for the byte under the cursor.
///
/// Pre-filled with whatever comment is already there, so `;` on a commented
/// offset edits it instead of forcing it to be retyped from scratch. Submitting
/// an empty box deletes the comment, which is how `Commands::comment` already
/// behaves.
pub fn open_comment_dialog(app: &mut App) {
    let existing = app
        .hex_view
        .comments
        .get(&app.hex_view.offset)
        .cloned()
        .unwrap_or_default();
    let cursor = existing.chars().count();
    app.hex_view.comment_input = tui_input::Input::new(existing).with_cursor(cursor);
    app.hex_view.comment_anchor = None;
    app.state = UIState::DialogComment;
    app.dialog_renderer = Some(dialog_comment_draw);
}

pub fn dialog_comment_draw(app: &mut App, frame: &mut Frame) {
    // A bordered box rather than the one-line overlay this used to draw on the
    // command bar: that shared its row with the status/command line, gave no
    // indication of which offset was being annotated, and looked identical to
    // typing a `:` command.
    // Lifted a few rows above centre, like the assemble dialog: dead centre sits
    // right on the line being annotated, so the box hides the very byte you are
    // commenting on.
    let mut area = center_widget(COMMENT_DIALOG_WIDTH, 3, frame.area());
    area.y = area.y.saturating_sub(4);

    let addr = if app.hex_view.show_va {
        app.get_va(app.hex_view.offset)
    } else {
        app.hex_view.offset as u64
    };
    let title = format!(
        " {} {:X} ",
        crate::i18n::M::CommentAtTitle.tr(app.config.lang),
        addr
    );

    let inner_width = area.width.saturating_sub(2) as usize;

    // Scroll the text so the cursor stays inside the box on a long comment.
    let cursor = app.hex_view.comment_input.visual_cursor();
    let scroll = cursor.saturating_sub(inner_width.saturating_sub(1));

    // The box scrolls, so only the part of the block that is on screen is painted.
    let paragraph = Paragraph::new(crate::text_field::render_window(
        &app.hex_view.comment_input,
        app.hex_view.comment_anchor,
        scroll,
        inner_width,
        app.config.theme.dialog,
        app.config.theme.highlight,
    ))
        .style(app.config.theme.dialog)
        .block(
            Block::bordered()
                .title(title)
                .title_alignment(Alignment::Center),
        );

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);

    let cursor_x = area.x + 1 + (cursor - scroll) as u16;
    if cursor_x < area.x + area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, area.y + 1));
    }
}

/// The comment box and its selection anchor.
fn comment_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (&mut app.hex_view.comment_input, &mut app.hex_view.comment_anchor)
}

/// Drops any existing Names-list entry for `offset`.
///
/// Keyed on the offset being commented, not on `hex_view.offset`. Using the
/// cursor position worked only while the two happened to agree - i.e. for the
/// `;` key - and broke `:cmt <offset> <text>`, which comments an arbitrary
/// address: the entry for the old comment at that address survived, so the Names
/// list showed the same offset twice with the stale text still first, and
/// deleting a comment left its entry behind entirely.
fn forget_name_entry(app: &mut App, offset: usize) {
    app.hex_view
        .comment_name_list
        .retain(|entry| entry.offset != offset);
}

impl Commands {
    pub fn comment(app: &mut App, offset: usize, comment: String) {
        if comment.is_empty() {
            // remove the comment; no effect if it doesn't exist
            app.hex_view.comments.remove(&offset);
            forget_name_entry(app, offset);
        } else {
            app.hex_view.comments.insert(offset, comment.clone());
            forget_name_entry(app, offset);
            app.hex_view
                .comment_name_list
                .push(Comment { offset, comment });
        }
        app.dialog_renderer = None;
        app.state = UIState::Normal;
    }
}

pub fn dialog_comment_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
            KeyCode::Enter => {
                let ofs = app.hex_view.offset;
                let cmt = app.hex_view.comment_input.value_and_reset();
                Commands::comment(app, ofs, cmt);
            }
            // Shift+arrows, Shift+Home/End and Ctrl+C/X/V over the block.
            _ => {
                crate::text_field::handle_key(app, comment_field, event);
            }
        }
    }
    Ok(false)
}

pub fn comment_show_draw(app: &mut App, frame: &mut Frame) {
    // check if the current offset has a comment to be shown
    if let Some(cmt) = app.hex_view.comments.get(&app.hex_view.offset)
        && app.state == UIState::Normal
    {
        // format comment
        let para = Paragraph::new(format!(";{}", cmt)).style(app.config.theme.main);

        frame.render_widget(Clear, app.command_area);
        frame.render_widget(para, app.command_area);
    }
}

#[cfg(test)]
mod comment_key_tests {
    use crate::app::App;
    use crate::editor::{AppView, UIState};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A distinct fixture per call: opening a file maps it, and tests run in
    /// parallel, so a shared path cannot be rewritten while still mapped.
    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dz6_comment_key");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("f_{n}.bin"));
        std::fs::write(&path, vec![0x90u8; 0x100]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app
    }

    fn press_semicolon(app: &mut App) {
        let key = KeyEvent {
            code: KeyCode::Char(';'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::global::events::handle_global_events(app, key);
    }

    /// ';' must open the dialog in every view.
    ///
    /// It was bound in `hex/events.rs` alone, so in the Disasm view - the one with a
    /// comment column, where annotating an address matters most - the key did
    /// nothing whatsoever, with no feedback.
    #[test]
    fn semicolon_opens_the_dialog_in_every_view() {
        for view in [AppView::Hex, AppView::Disasm, AppView::Text, AppView::Header] {
            let mut app = app_with_file();
            app.editor_view = view;
            app.state = UIState::Normal;

            press_semicolon(&mut app);

            assert!(
                app.state == UIState::DialogComment,
                "';' did not open the comment dialog in {:?}",
                view
            );
            assert!(app.dialog_renderer.is_some(), "no renderer set for {:?}", view);
        }
    }

    /// The box opens pre-filled, so ';' on a commented offset edits it.
    #[test]
    fn the_dialog_is_prefilled_with_the_existing_comment() {
        let mut app = app_with_file();
        app.hex_view.offset = 0x20;
        app.hex_view.comments.insert(0x20, "existing".to_string());
        app.editor_view = AppView::Disasm;

        press_semicolon(&mut app);

        assert_eq!(app.hex_view.comment_input.value(), "existing");
        assert_eq!(
            app.hex_view.comment_input.cursor(),
            "existing".chars().count(),
            "the cursor belongs at the end so typing appends"
        );
    }

    /// The box renders above centre, so it does not cover the line being annotated.
    #[test]
    fn the_dialog_renders_above_centre() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = app_with_file();
        app.hex_view.offset = 0x20;
        press_semicolon(&mut app);

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                super::dialog_comment_draw(&mut app, f);
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..24u16)
            .map(|y| (0..100u16).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();

        let border_row = rows
            .iter()
            .position(|r| r.contains("Comment at"))
            .expect("the titled border must be drawn");
        // A vertically centred 3-row box on a 24-row screen starts at row 10.
        assert!(
            border_row < 10,
            "the box must sit above centre, found its title on row {border_row}"
        );
        assert!(
            rows[border_row].contains("20"),
            "the title must name the offset being annotated, got: {}",
            rows[border_row].trim_end()
        );
    }

    /// An offset with no comment opens empty, not with the previous one's text.
    #[test]
    fn the_dialog_is_empty_for_an_uncommented_offset() {
        let mut app = app_with_file();
        app.hex_view.comments.insert(0x20, "elsewhere".to_string());
        app.hex_view.offset = 0x30;

        press_semicolon(&mut app);

        assert_eq!(app.hex_view.comment_input.value(), "");
    }
}

#[cfg(test)]
mod comment_tests {
    use crate::app::App;
    use crate::commands::Commands;

    fn app() -> App {
        let mut app = App::new();
        app.config.database = false;
        app
    }

    /// A second comment at the same offset replaces the Names-list entry.
    ///
    /// The de-duplication used to key on `hex_view.offset` - the cursor - rather
    /// than the offset being commented, so it only worked when the two agreed.
    /// With `:cmt <offset> <text>` the old entry survived and the Names list held
    /// the same offset twice, stale text first.
    #[test]
    fn recommenting_an_offset_replaces_the_entry() {
        let mut app = app();
        app.hex_view.offset = 0; // cursor deliberately elsewhere

        Commands::comment(&mut app, 0x100, "first".to_string());
        Commands::comment(&mut app, 0x100, "second".to_string());

        assert_eq!(
            app.hex_view.comment_name_list.len(),
            1,
            "one entry per offset"
        );
        assert_eq!(app.hex_view.comment_name_list[0].comment, "second");
        assert_eq!(
            app.hex_view.comments.get(&0x100).map(String::as_str),
            Some("second")
        );
    }

    /// Deleting a comment must remove its Names-list entry too.
    #[test]
    fn deleting_a_comment_removes_the_entry() {
        let mut app = app();
        app.hex_view.offset = 0;

        Commands::comment(&mut app, 0x100, "note".to_string());
        Commands::comment(&mut app, 0x100, String::new());

        assert!(
            app.hex_view.comment_name_list.is_empty(),
            "the Names list still lists a comment that no longer exists"
        );
        assert!(!app.hex_view.comments.contains_key(&0x100));
    }

    /// Comments at other offsets are untouched.
    #[test]
    fn other_offsets_are_left_alone() {
        let mut app = app();

        Commands::comment(&mut app, 0x100, "a".to_string());
        Commands::comment(&mut app, 0x200, "b".to_string());
        Commands::comment(&mut app, 0x100, "a2".to_string());

        let mut entries: Vec<(usize, String)> = app
            .hex_view
            .comment_name_list
            .iter()
            .map(|c| (c.offset, c.comment.clone()))
            .collect();
        entries.sort();
        assert_eq!(
            entries,
            vec![(0x100, "a2".to_string()), (0x200, "b".to_string())]
        );
    }
}
