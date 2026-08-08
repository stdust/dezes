use std::io::Result;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{app::App, editor::UIState, text};

/// Bytes of file one screen row of text covers.
///
/// `text_contents_draw` decodes `height * width` bytes from `reader.page_start`,
/// so a row is a row's width of bytes in a single-byte encoding. Multi-byte
/// encodings decode to fewer characters than that, which makes the step
/// conservative rather than wrong: it moves at most one row.
fn row_bytes(app: &App) -> usize {
    app.text_view.area_width.max(1) as usize
}

/// Moves the window the text view decodes from by `rows` rows, forward when
/// `forward` is set.
///
/// Down did nothing at all on an executable. The paragraph offset was the only
/// thing the arrows moved, and it is bounded by `lines_to_show` - the number of
/// lines in the decoded chunk. A text document wraps into many more lines than
/// the viewport is tall, so there was always somewhere to scroll; a binary has
/// almost no newlines in it, so `lines_to_show` never exceeded the height and the
/// key was a no-op. Moving the window through the file is what "down" means in
/// either case.
fn scroll_window(app: &mut App, rows: usize, forward: bool) {
    let step = row_bytes(app).saturating_mul(rows);
    let len = app.file_info.buffer_len();

    let new_start = if forward {
        // Stops with the last screenful in view rather than scrolling into an
        // empty page past the end.
        let screen = row_bytes(app).saturating_mul(app.text_view.area_height.max(1) as usize);
        let last_start = len.saturating_sub(screen);
        app.reader.page_start.saturating_add(step).min(last_start)
    } else {
        app.reader.page_start.saturating_sub(step)
    };

    if new_start == app.reader.page_start {
        crate::beep!();
        return;
    }

    app.reader.page_start = new_start;
    app.reader.page_end = new_start
        .saturating_add(app.reader.page_current_size)
        .saturating_sub(1);
    // The cursor follows what is on screen, so the status bar reports where the
    // window is and switching back to Hex lands on the bytes that were visible.
    app.hex_view.offset = new_start.min(len.saturating_sub(1));
}

pub fn text_mode_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // Inside the decoded chunk while there is more of it above or below the
        // viewport; through the file itself once there is not.
        KeyCode::Up => {
            if app.text_view.scroll_offset.0 > 0 {
                app.text_view.scroll_offset.0 -= 1;
            } else {
                scroll_window(app, 1, false);
            }
        }
        KeyCode::Down => {
            // The last line number being shown, against the number of lines the
            // chunk decoded to - kept up to date by `text_contents_draw`.
            let last_line_shown: usize =
                (app.text_view.scroll_offset.0 + app.text_view.area_height).into();
            if last_line_shown < app.text_view.lines_to_show {
                app.text_view.scroll_offset.0 += 1;
            } else {
                scroll_window(app, 1, true);
            }
        }
        KeyCode::PageUp => {
            let height = app.text_view.area_height;
            if app.text_view.scroll_offset.0 >= height {
                app.text_view.scroll_offset.0 -= height;
            } else {
                app.text_view.scroll_offset.0 = 0;
                scroll_window(app, height.max(1) as usize, false);
            }
        }
        KeyCode::PageDown => {
            let height = app.text_view.area_height;
            let last_line_shown: usize = (app.text_view.scroll_offset.0 + height).into();
            if last_line_shown + (height as usize) < app.text_view.lines_to_show {
                app.text_view.scroll_offset.0 += height;
            } else {
                scroll_window(app, height.max(1) as usize, true);
                app.text_view.scroll_offset.0 = 0;
            }
        }
        KeyCode::Left if app.text_view.scroll_offset.1 > 0 => {
            app.text_view.scroll_offset.1 -= 1;
        }
        KeyCode::Right => {
            app.text_view.scroll_offset.1 += 1;
        }
        KeyCode::Home => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                app.reader.page_start = 0;
                app.hex_view.offset = 0;
                app.text_view.scroll_offset = (0, 0);
            } else {
                app.text_view.scroll_offset.1 = 0;
            }
        }
        KeyCode::End => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let len = app.file_info.buffer_len();
                if len > 0 {
                    let screen = row_bytes(app).saturating_mul(app.text_view.area_height.max(1) as usize);
                    let last_start = len.saturating_sub(screen);
                    app.reader.page_start = last_start;
                    app.hex_view.offset = len.saturating_sub(1);
                    let delta = app.text_view.lines_to_show.saturating_sub(app.text_view.area_height as usize);
                    app.text_view.scroll_offset = (delta as u16, 0);
                }
            } else {
                app.text_view.scroll_offset.1 = app.text_view.scroll_offset.1.saturating_add(20);
            }
        }
        // End should go to the end of the current line,
        // but I probably need the length of the biggest line
        // to set text_mode.scroll_offset.1 there
        // encoding dialog (Alt+E), matching the Hex view. Was a bare 'e'.
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.state = UIState::DialogEncoding;
            app.dialog_renderer = Some(text::dialog_encoding::dialog_encoding_draw);
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    use crate::editor::AppView;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};
    use ratatui::layout::Rect;
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A file with no newlines in it at all, which is what an executable looks
    /// like to the text view.
    fn app_with_binary() -> App {
        let dir = std::env::temp_dir().join("dezes_text_scroll");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("b_{}_{}.bin", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        // 64 KB of printable bytes, no 0x0A anywhere.
        let bytes: Vec<u8> = (0..0x10000u32).map(|i| b'A' + (i % 26) as u8).collect();
        std::fs::write(&path, &bytes).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.editor_view = AppView::Text;
        app
    }

    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 24;

    /// One frame, so the view records the size of its viewport.
    fn render(app: &mut App) {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
        app.screen = Rect::new(0, 0, WIDTH, HEIGHT);
        terminal.draw(|f| crate::draw::draw(f, app)).expect("draw");
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(app, key);
    }

    /// Down has to move through the file on a binary.
    ///
    /// It only ever moved the paragraph's scroll offset, which is bounded by the
    /// number of lines the decoded chunk has. An executable decodes to almost no
    /// lines, so the key did nothing at all - the view was stuck on the first
    /// screenful.
    #[test]
    fn down_moves_through_a_file_without_newlines() {
        let mut app = app_with_binary();
        render(&mut app);
        assert!(app.text_view.area_width > 0, "the draw has to report its width");

        let start = app.reader.page_start;
        press(&mut app, KeyCode::Down);
        assert!(
            app.reader.page_start > start,
            "Down left the window at {} - the view cannot be scrolled at all",
            start
        );

        // And back.
        press(&mut app, KeyCode::Up);
        assert_eq!(app.reader.page_start, start, "Up has to undo it");
    }

    /// The cursor follows the window, so the status bar and a switch back to Hex
    /// agree with what is on screen.
    #[test]
    fn the_cursor_follows_the_window() {
        let mut app = app_with_binary();
        render(&mut app);

        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }

        assert_eq!(app.hex_view.offset, app.reader.page_start);
        assert!(app.hex_view.offset > 0);
    }

    /// Scrolling stops with the last screenful visible instead of running off the
    /// end of the file.
    #[test]
    fn the_window_stops_at_the_end() {
        let mut app = app_with_binary();
        render(&mut app);

        for _ in 0..10_000 {
            press(&mut app, KeyCode::Down);
        }

        let screen = app.text_view.area_width as usize * app.text_view.area_height as usize;
        assert_eq!(
            app.reader.page_start,
            app.file_info.buffer_len().saturating_sub(screen),
            "the window ran past the last screenful"
        );
    }

    /// A text document still scrolls inside the decoded chunk first, so wrapped
    /// lines are not skipped over.
    #[test]
    fn a_text_file_still_scrolls_line_by_line() {
        let dir = std::env::temp_dir().join("dezes_text_scroll");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("t_{}_{}.txt", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let text = (0..400).map(|i| format!("line {}\n", i)).collect::<String>();
        std::fs::write(&path, text.as_bytes()).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.editor_view = AppView::Text;
        render(&mut app);

        let start = app.reader.page_start;
        press(&mut app, KeyCode::Down);

        assert_eq!(app.text_view.scroll_offset.0, 1, "one line, not one window");
        assert_eq!(app.reader.page_start, start, "the window stays put");
    }
}