use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Alignment;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{Frame, widgets::Clear};
use std::io::Result;

use crate::i18n::M;
use crate::util::center_widget;
use crate::{app::App, editor::UIState};

/// Maximum log lines kept in memory.
///
/// An unbounded `Vec<String>` grew for the whole session, and `dialog_log_draw`
/// joins the lot into one String per frame while the window is open.
const MAX_LOG_LINES: usize = 1000;

impl App {
    pub fn log(&mut self, text: String) {
        if self.logs.len() >= MAX_LOG_LINES {
            // Drop the oldest quarter at once rather than shifting the whole Vec
            // on every single push.
            self.logs.drain(0..MAX_LOG_LINES / 4);
        }
        self.logs.push(text)
    }

    /// Shows `text` in the command bar until the next key press, beeps, and keeps
    /// a copy in the log.
    ///
    /// The log alone was not enough for a refused key: it is only visible with the
    /// Log dialog (Alt+L) open, so a shortcut that silently did nothing looked like
    /// a bug rather than a restriction.
    pub fn error(&mut self, text: String) {
        crate::beep!();
        self.log(text.clone());
        self.status_error = Some(text);
    }

    /// Puts `text` on the clipboard and says what happened.
    ///
    /// `label` completes "Copied ... to clipboard", e.g. `"82 rows"`. Every result
    /// list needs the same four lines, and the clipboard can be missing outright
    /// (a bare TTY has no session to talk to) - a copy key that silently does
    /// nothing is indistinguishable from a key that is not bound, which is the
    /// state the three result dialogs were in.
    pub fn copy_to_clipboard(&mut self, text: String, label: String) {
        if text.is_empty() {
            self.error("Nothing to copy".to_string());
            return;
        }
        let copied = self
            .clipboard
            .as_mut()
            .ok()
            .and_then(|clip| clip.set_text(text).ok())
            .is_some();
        if copied {
            self.log(format!("Copied {} to clipboard", label));
        } else {
            self.error("Could not access the clipboard".to_string());
        }
    }

    /// Standard refusal for a shortcut a read-only file does not allow.
    ///
    /// `action` completes "cannot ...", e.g. [`M::RoEditData`]. Both halves are
    /// translated, so the sentence reads properly in every language rather than
    /// having an English tail.
    pub fn read_only_error(&mut self, action: crate::i18n::M) {
        let lang = self.config.lang;
        let message = crate::i18n::fill(
            crate::i18n::M::ReadOnlyRefused.tr(lang),
            &[action.tr(lang)],
        );
        self.error(message);
    }
}

pub fn open_log_dialog(app: &mut App) {
    app.state = UIState::DialogLog;
    app.dialog_renderer = Some(dialog_log_draw);
}

pub fn dialog_log_draw(app: &mut App, frame: &mut Frame) {
    let lang = app.config.lang;
    let text = format!("{:?}\n\n{}", app.reader, &app.logs.join("\n"));

    let para = Paragraph::new(text)
        .style(app.config.theme.dialog)
        .wrap(Wrap { trim: true })
        .block(
            Block::bordered()
                .title(M::LogTitle.tr(lang))
                .title_alignment(Alignment::Center)
                .title_bottom(M::LogFooter.tr(lang)),
        )
        .scroll(app.log_scroll_offset);

    // `- 5` underflows on a terminal narrower/shorter than 5 cells.
    let width = frame.area().width.saturating_sub(5);
    let height = frame.area().height.saturating_sub(5);
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(para, dialog_area);
}

pub fn dialog_log_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // close log dialog
        KeyCode::Esc => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
        }
        // Copies the whole log, like `y` on the About box, for pasting into a bug
        // report: the interesting lines are usually the ones scrolled off.
        //
        // `c` as well as `y`: every other panel that copies takes both, and which
        // one a person reaches for depends on whether they think "yank" or "copy".
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('c') | KeyCode::Char('C') => {
            let text = app.logs.join("\n");
            let copied = app
                .clipboard
                .as_mut()
                .ok()
                .and_then(|clip| clip.set_text(text).ok())
                .is_some();
            if copied {
                App::log(app, "Copied the log to clipboard".to_string());
            } else {
                app.error("Could not access the clipboard".to_string());
            }
        }
        // Empties the log.
        //
        // Diagnostics accumulate: by the time something interesting happens there
        // can be a thousand lines of slow-frame reports above it. Clearing first and
        // then reproducing the problem gives a log that is all signal.
        KeyCode::Delete => {
            let had = app.logs.len();
            app.logs.clear();
            app.log_scroll_offset = (0, 0);
            App::log(app, format!("Log cleared ({} line(s) dropped)", had));
        }
        KeyCode::Down => {
            app.log_scroll_offset.0 += 1;
        }
        KeyCode::Up => {
            app.log_scroll_offset.0 = app.log_scroll_offset.0.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.log_scroll_offset.0 = app.log_scroll_offset.0.saturating_add(10);
        }
        KeyCode::PageUp => {
            app.log_scroll_offset.0 = app.log_scroll_offset.0.saturating_sub(10);
        }
        KeyCode::Home => {
            app.log_scroll_offset.0 = 0;
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(bytes: &[u8]) -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dezes_log");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("log_{}_{}.bin", std::process::id(), id));
        std::fs::write(&path, bytes).unwrap();
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, false).unwrap();
        app
    }

    #[test]
    fn the_log_is_capped() {
        let mut app = app_with(&[0x41]);
        for i in 0..(MAX_LOG_LINES + 50) {
            app.log(format!("line {}", i));
        }
        assert!(
            app.logs.len() <= MAX_LOG_LINES,
            "grew to {} lines",
            app.logs.len()
        );
        assert!(
            app.logs.last().unwrap().ends_with(&format!("{}", MAX_LOG_LINES + 49)),
            "the newest line has to survive the trim"
        );
    }

    /// Delete empties the log, so a problem can be reproduced against a clean one.
    #[test]
    fn delete_clears_the_log() {
        let mut app = app_with(&[0x41]);
        for i in 0..50 {
            app.log(format!("line {}", i));
        }
        open_log_dialog(&mut app);
        app.log_scroll_offset = (12, 0);

        dialog_log_events(
            &mut app,
            KeyEvent::new(KeyCode::Delete, ratatui::crossterm::event::KeyModifiers::NONE),
        )
        .unwrap();

        // One line remains: the report of the clearing itself, which says how much
        // went. An empty window would leave no sign that the key did anything.
        assert_eq!(app.logs.len(), 1, "logs: {:?}", app.logs);
        assert!(app.logs[0].contains("cleared"), "got: {:?}", app.logs[0]);
        assert_eq!(app.log_scroll_offset, (0, 0), "the scroll has to go back to the top");
        assert!(app.state == UIState::DialogLog, "clearing closed the window");
    }

    /// `c` copies as well as `y`, matching every other panel that copies.
    #[test]
    fn c_copies_like_y() {
        let mut app = app_with(&[0x41]);
        app.log("something worth pasting".to_string());
        open_log_dialog(&mut app);

        for key in [KeyCode::Char('c'), KeyCode::Char('C'), KeyCode::Char('y')] {
            let before = app.logs.len();
            dialog_log_events(
                &mut app,
                KeyEvent::new(key, ratatui::crossterm::event::KeyModifiers::NONE),
            )
            .unwrap();
            // Either outcome is reported; a machine without a clipboard still logs.
            assert!(
                app.logs.len() > before,
                "{:?} did not report anything",
                key
            );
            let last = app.logs.last().cloned().unwrap_or_default();
            assert!(
                last.contains("clipboard"),
                "{:?} logged {:?}",
                key,
                last
            );
        }
    }

    /// The footer names both of the keys that are not guessable.
    #[test]
    fn the_footer_lists_copy_and_clear() {
        for lang in crate::i18n::Lang::ALL {
            let footer = M::LogFooter.tr(lang);
            assert!(footer.contains('y'), "{:?}: {:?}", lang, footer);
            assert!(footer.contains("Delete"), "{:?}: {:?}", lang, footer);
        }
    }

    #[test]
    fn esc_closes_the_window() {
        let mut app = app_with(&[0x41]);
        open_log_dialog(&mut app);
        assert!(app.dialog_renderer.is_some());

        dialog_log_events(
            &mut app,
            KeyEvent::new(KeyCode::Esc, ratatui::crossterm::event::KeyModifiers::NONE),
        )
        .unwrap();

        assert!(app.state == UIState::Normal);
        assert!(app.dialog_renderer.is_none());
    }
}