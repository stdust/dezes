use std::io::Result;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{app::App, editor::UIState, text};

/// Bytes of file one screen row of text covers.
fn row_bytes(app: &App) -> usize {
    app.text_view.area_width.max(1) as usize
}

/// Moves the window the text view decodes from by `rows` rows, forward when
/// `forward` is set.
fn scroll_window(app: &mut App, rows: usize, forward: bool) {
    let step = row_bytes(app).saturating_mul(rows);
    let len = app.file_info.buffer_len();

    let new_start = if forward {
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
    app.hex_view.offset = new_start.min(len.saturating_sub(1));
}

fn line_char_count(lines: &[String], line_idx: usize) -> usize {
    lines.get(line_idx).map(|s| s.chars().count()).unwrap_or(0)
}

fn ensure_cursor_visible(app: &mut App) {
    let height = app.text_view.area_height.max(1) as usize;
    let width = app.text_view.area_width.max(1) as usize;
    let cursor = app.text_view.cursor;

    let scroll_y = app.text_view.scroll_offset.0 as usize;
    if cursor.0 < scroll_y {
        app.text_view.scroll_offset.0 = cursor.0 as u16;
    } else if cursor.0 >= scroll_y + height {
        app.text_view.scroll_offset.0 = (cursor.0 + 1).saturating_sub(height) as u16;
    }

    let scroll_x = app.text_view.scroll_offset.1 as usize;
    if cursor.1 < scroll_x {
        app.text_view.scroll_offset.1 = cursor.1 as u16;
    } else if cursor.1 >= scroll_x + width {
        app.text_view.scroll_offset.1 = (cursor.1 + 1).saturating_sub(width) as u16;
    }
}

pub fn get_selected_text(app: &App) -> Option<String> {
    let sel = app.text_view.normalized_selection()?;
    let lines = crate::text::draw::get_visual_lines(app);
    if lines.is_empty() {
        return None;
    }
    let (start_pos, end_pos) = sel;
    let mut result = String::new();
    let max_idx = end_pos.0.min(lines.len().saturating_sub(1));
    for (line_idx, line_str) in lines.iter().enumerate().take(max_idx + 1).skip(start_pos.0) {
        let char_count = line_str.chars().count();
        if line_idx == start_pos.0 && line_idx == end_pos.0 {
            let s_col = start_pos.1.min(char_count);
            let e_col = end_pos.1.min(char_count);
            let part: String = line_str.chars().skip(s_col).take(e_col.saturating_sub(s_col)).collect();
            result.push_str(&part);
        } else if line_idx == start_pos.0 {
            let s_col = start_pos.1.min(char_count);
            let part: String = line_str.chars().skip(s_col).collect();
            result.push_str(&part);
            result.push('\n');
        } else if line_idx == end_pos.0 {
            let e_col = end_pos.1.min(char_count);
            let part: String = line_str.chars().take(e_col).collect();
            result.push_str(&part);
        } else {
            result.push_str(line_str);
            result.push('\n');
        }
    }
    Some(result)
}

pub fn copy_text_selection(app: &mut App) {
    if let Some(text) = get_selected_text(app)
        && !text.is_empty()
    {
        let len = text.chars().count();
        app.copy_to_clipboard(text, format!("{} chars", len));
        return;
    }

    let lines = crate::text::draw::get_visual_lines(app);
    if let Some(line) = lines.get(app.text_view.cursor.0) {
        if !line.is_empty() {
            app.copy_to_clipboard(line.clone(), "1 line".to_string());
        } else {
            let msg = crate::i18n::M::ErrNothingToCopy.tr(app.config.lang).to_string();
            app.error(msg);
        }
    } else {
        let msg = crate::i18n::M::ErrNothingToCopy.tr(app.config.lang).to_string();
        app.error(msg);
    }
}

pub fn text_mode_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let lines = crate::text::draw::get_visual_lines(app);
    let num_lines = lines.len();

    let update_selection_before_move = |app: &mut App| {
        if shift {
            if app.text_view.selection_anchor.is_none() {
                app.text_view.selection_anchor = Some(app.text_view.cursor);
            }
        } else {
            app.text_view.selection_anchor = None;
        }
    };

    match key.code {
        KeyCode::Up => {
            update_selection_before_move(app);
            if app.text_view.cursor.0 > 0 {
                app.text_view.cursor.0 -= 1;
                let len = line_char_count(&lines, app.text_view.cursor.0);
                app.text_view.cursor.1 = app.text_view.cursor.1.min(len);
                ensure_cursor_visible(app);
            } else if app.reader.page_start > 0 {
                scroll_window(app, 1, false);
                app.text_view.cursor.0 = 0;
                app.text_view.scroll_offset.0 = 0;
            }
        }
        KeyCode::Down => {
            update_selection_before_move(app);
            let height = app.text_view.area_height.max(1) as usize;
            if app.text_view.cursor.0 + 1 < num_lines {
                app.text_view.cursor.0 += 1;
                let len = line_char_count(&lines, app.text_view.cursor.0);
                app.text_view.cursor.1 = app.text_view.cursor.1.min(len);
                ensure_cursor_visible(app);
            } else {
                let start_before = app.reader.page_start;
                scroll_window(app, 1, true);
                if app.reader.page_start > start_before {
                    let updated_lines = crate::text::draw::get_visual_lines(app);
                    let new_len = updated_lines.len();
                    app.text_view.cursor.0 = new_len.saturating_sub(1);
                    let len = line_char_count(&updated_lines, app.text_view.cursor.0);
                    app.text_view.cursor.1 = app.text_view.cursor.1.min(len);
                    app.text_view.scroll_offset.0 = new_len.saturating_sub(height) as u16;
                }
            }
        }
        KeyCode::Left => {
            update_selection_before_move(app);
            if app.text_view.cursor.1 > 0 {
                app.text_view.cursor.1 -= 1;
            } else if app.text_view.cursor.0 > 0 {
                app.text_view.cursor.0 -= 1;
                app.text_view.cursor.1 = line_char_count(&lines, app.text_view.cursor.0);
            }
            ensure_cursor_visible(app);
        }
        KeyCode::Right => {
            update_selection_before_move(app);
            let cur_len = line_char_count(&lines, app.text_view.cursor.0);
            if app.text_view.cursor.1 < cur_len {
                app.text_view.cursor.1 += 1;
            } else if app.text_view.cursor.0 + 1 < num_lines {
                app.text_view.cursor.0 += 1;
                app.text_view.cursor.1 = 0;
            }
            ensure_cursor_visible(app);
        }
        KeyCode::Home => {
            update_selection_before_move(app);
            if ctrl {
                app.reader.page_start = 0;
                app.hex_view.offset = 0;
                app.text_view.scroll_offset = (0, 0);
                app.text_view.cursor = (0, 0);
            } else {
                app.text_view.cursor.1 = 0;
                app.text_view.scroll_offset.1 = 0;
            }
        }
        KeyCode::End => {
            update_selection_before_move(app);
            if ctrl {
                let len = app.file_info.buffer_len();
                if len > 0 {
                    let screen = row_bytes(app).saturating_mul(app.text_view.area_height.max(1) as usize);
                    let last_start = len.saturating_sub(screen);
                    app.reader.page_start = last_start;
                    app.hex_view.offset = len.saturating_sub(1);
                    let delta = app.text_view.lines_to_show.saturating_sub(app.text_view.area_height as usize);
                    app.text_view.scroll_offset = (delta as u16, 0);
                    let last_idx = num_lines.saturating_sub(1);
                    app.text_view.cursor = (last_idx, line_char_count(&lines, last_idx));
                }
            } else {
                app.text_view.cursor.1 = line_char_count(&lines, app.text_view.cursor.0);
                ensure_cursor_visible(app);
            }
        }
        KeyCode::PageUp => {
            update_selection_before_move(app);
            let height = app.text_view.area_height.max(1) as usize;
            if app.text_view.cursor.0 >= height {
                app.text_view.cursor.0 -= height;
            } else {
                scroll_window(app, height, false);
                app.text_view.cursor.0 = 0;
                app.text_view.scroll_offset.0 = 0;
            }
            let len = line_char_count(&lines, app.text_view.cursor.0);
            app.text_view.cursor.1 = app.text_view.cursor.1.min(len);
            ensure_cursor_visible(app);
        }
        KeyCode::PageDown => {
            update_selection_before_move(app);
            let height = app.text_view.area_height.max(1) as usize;
            if app.text_view.cursor.0 + height < num_lines {
                app.text_view.cursor.0 += height;
            } else {
                let start_before = app.reader.page_start;
                scroll_window(app, height, true);
                if app.reader.page_start > start_before {
                    let updated_lines = crate::text::draw::get_visual_lines(app);
                    let new_len = updated_lines.len();
                    app.text_view.cursor.0 = new_len.saturating_sub(1);
                    app.text_view.scroll_offset.0 = new_len.saturating_sub(height) as u16;
                }
            }
            let len = line_char_count(&lines, app.text_view.cursor.0);
            app.text_view.cursor.1 = app.text_view.cursor.1.min(len);
            ensure_cursor_visible(app);
        }
        // Ctrl+A: Select All
        KeyCode::Char('a') | KeyCode::Char('A') if ctrl => {
            app.text_view.selection_anchor = Some((0, 0));
            let last_idx = num_lines.saturating_sub(1);
            let last_len = line_char_count(&lines, last_idx);
            app.text_view.cursor = (last_idx, last_len);
        }
        // Copy: 'y' or Ctrl+C
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('c') | KeyCode::Char('C')
            if ctrl || key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') =>
        {
            copy_text_selection(app);
        }
        // Esc: Clear selection or return to primary view
        KeyCode::Esc => {
            if app.text_view.selection_anchor.is_some() {
                app.text_view.selection_anchor = None;
            } else {
                app.return_to_primary_view();
            }
        }
        // Alt+E: Encoding dialog
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.state = UIState::DialogEncoding;
            app.dialog_renderer = Some(text::dialog_encoding::dialog_encoding_draw);
        }
        _ => {}
    }

    let len = app.file_info.buffer_len();
    if len > 0 {
        let width = app.text_view.area_width.max(1) as usize;
        let cur_row = app.text_view.cursor.0 + (app.text_view.scroll_offset.0 as usize);
        let cur_col = app.text_view.cursor.1;
        let byte_ofs = app.reader.page_start.saturating_add(cur_row.saturating_mul(width)).saturating_add(cur_col);
        app.hex_view.offset = byte_ofs.min(len.saturating_sub(1));
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

    /// Down moves through the file on a binary.
    #[test]
    fn down_moves_through_a_file_without_newlines() {
        let mut app = app_with_binary();
        render(&mut app);
        assert!(app.text_view.area_width > 0, "the draw has to report its width");

        let start = app.reader.page_start;
        for _ in 0..HEIGHT + 2 {
            press(&mut app, KeyCode::Down);
        }
        assert!(
            app.reader.page_start > start,
            "Down left the window at {} - the view cannot be scrolled at all",
            start
        );

        // And back.
        for _ in 0..HEIGHT + 2 {
            press(&mut app, KeyCode::Up);
        }
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

        assert_eq!(app.text_view.cursor.0, 1, "cursor moves down one line");
        assert_eq!(app.reader.page_start, start, "the window stays put");
    }

    #[test]
    fn cursor_navigation_and_shift_selection() {
        let dir = std::env::temp_dir().join("dezes_text_scroll");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("t_sel_{}_{}.txt", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let text = "Hello World\nSecond Line\nThird Line\n";
        std::fs::write(&path, text.as_bytes()).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.editor_view = AppView::Text;
        render(&mut app);

        assert_eq!(app.text_view.cursor, (0, 0));
        assert!(app.text_view.selection_anchor.is_none());

        // Press Right 5 times -> cursor at (0, 5)
        for _ in 0..5 {
            let key = KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = text_mode_events(&mut app, key);
        }
        assert_eq!(app.text_view.cursor, (0, 5));

        // Press Shift+Right 6 times -> selection from (0, 5) to (0, 11)
        for _ in 0..6 {
            let key = KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = text_mode_events(&mut app, key);
        }
        assert_eq!(app.text_view.selection_anchor, Some((0, 5)));
        assert_eq!(app.text_view.cursor, (0, 11));

        let selected = get_selected_text(&app);
        assert_eq!(selected.as_deref(), Some(" World"));

        // Press Shift+Down -> selection extends to line 1
        let shift_down = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, shift_down);
        assert_eq!(app.text_view.selection_anchor, Some((0, 5)));
        assert_eq!(app.text_view.cursor, (1, 11));

        let selected_multi = get_selected_text(&app);
        assert_eq!(selected_multi.as_deref(), Some(" World\nSecond Line"));

        // Pressing plain Left clears selection
        let left = KeyEvent {
            code: KeyCode::Left,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, left);
        assert!(app.text_view.selection_anchor.is_none());

        // Ctrl+A selects all
        let ctrl_a = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, ctrl_a);
        assert_eq!(app.text_view.selection_anchor, Some((0, 0)));

        // Esc clears selection
        let esc = KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, esc);
        assert!(app.text_view.selection_anchor.is_none());
    }

    #[test]
    fn copy_selection_and_line() {
        let dir = std::env::temp_dir().join("dezes_text_scroll");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("t_copy_{}_{}.txt", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let text = "First Line Text\nSecond Line Text\n";
        std::fs::write(&path, text.as_bytes()).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.editor_view = AppView::Text;
        render(&mut app);

        // Select "First Line"
        for _ in 0..10 {
            let key = KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = text_mode_events(&mut app, key);
        }

        // Press 'y' to copy
        let y_key = KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, y_key);
        let log = app.logs.last().cloned().unwrap_or_default();
        assert!(log.contains("chars"), "log should report copied chars: {}", log);

        // Clear selection and move to line 1
        press(&mut app, KeyCode::Down);
        let ctrl_c = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = text_mode_events(&mut app, ctrl_c);
        let log2 = app.logs.last().cloned().unwrap_or_default();
        assert!(log2.contains("1 line"), "log should report 1 line: {}", log2);
    }
}