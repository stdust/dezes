use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Alignment;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{Frame, widgets::Clear};
use std::io::Result;

use crate::i18n::M;
use crate::util::center_widget;
use crate::{app::App, editor::UIState};

/// Maximum log lines kept in memory.
const MAX_LOG_LINES: usize = 1000;

impl App {
    pub fn log(&mut self, text: String) {
        if self.logs.len() >= MAX_LOG_LINES {
            self.logs.drain(0..MAX_LOG_LINES / 4);
        }
        self.logs.push(text)
    }

    pub fn error(&mut self, text: String) {
        crate::beep!();
        self.log(text.clone());
        self.status_error = Some(text);
    }

    pub fn copy_to_clipboard(&mut self, text: String, label: String) {
        if text.is_empty() {
            let msg = M::ErrNothingToCopy.tr(self.config.lang).to_string();
            self.error(msg);
            return;
        }
        let copied = self
            .clipboard
            .as_mut()
            .ok()
            .and_then(|clip| clip.set_text(text).ok())
            .is_some();
        if copied {
            let msg = match self.config.lang {
                crate::i18n::Lang::Ko => format!("{}을(를) 클립보드에 복사했습니다", label),
                crate::i18n::Lang::Zh => format!("已将 {} 复制到剪贴板", label),
                crate::i18n::Lang::En => format!("Copied {} to clipboard", label),
            };
            self.log(msg);
        } else {
            let msg = M::ErrClipboardAccess.tr(self.config.lang).to_string();
            self.error(msg);
        }
    }

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

    let width = frame.area().width.saturating_sub(5);
    let height = frame.area().height.saturating_sub(5);
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(para, dialog_area);
}

pub fn dialog_log_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let text = app.logs.join("\n");
            let copied = app
                .clipboard
                .as_mut()
                .ok()
                .and_then(|clip| clip.set_text(text).ok())
                .is_some();
            if copied {
                App::log(app, M::DoneLogCopied.tr(app.config.lang).to_string());
            } else {
                let msg = M::ErrClipboardAccess.tr(app.config.lang).to_string();
                app.error(msg);
            }
        }
        KeyCode::Delete => {
            let had = app.logs.len();
            app.logs.clear();
            app.log_scroll_offset = (0, 0);
            let msg = crate::i18n::fill(M::DoneLogCleared.tr(app.config.lang), &[&had.to_string()]);
            App::log(app, msg);
        }
        KeyCode::Down => {
            app.log_scroll_offset.0 = app.log_scroll_offset.0.saturating_add(1);
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

        assert_eq!(app.logs.len(), 1, "logs: {:?}", app.logs);
        assert!(app.logs[0].contains("cleared"), "got: {:?}", app.logs[0]);
        assert_eq!(app.log_scroll_offset, (0, 0), "the scroll has to go back to the top");
        assert!(app.state == UIState::DialogLog, "clearing closed the window");
    }

    #[test]
    fn c_copies_like_y() {
        let mut app = app_with(&[0x41]);
        app.log("something worth pasting".to_string());
        open_log_dialog(&mut app);

        for key in [KeyCode::Char('c'), KeyCode::Char('C')] {
            let before = app.logs.len();
            dialog_log_events(
                &mut app,
                KeyEvent::new(key, ratatui::crossterm::event::KeyModifiers::CONTROL),
            )
            .unwrap();
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

    #[test]
    fn the_footer_lists_copy_and_clear() {
        for lang in crate::i18n::Lang::ALL {
            let footer = M::LogFooter.tr(lang);
            assert!(footer.contains("Ctrl+C"), "{:?}: {:?}", lang, footer);
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

    #[test]
    fn log_command_without_args_opens_dialog() {
        let mut app = app_with(&[0x41]);
        crate::commands::parse_command(&mut app, "log");
        assert!(app.state == UIState::DialogLog);
        assert!(app.dialog_renderer.is_some());
    }

    #[test]
    fn log_command_records_user_message() {
        let mut app = app_with(&[0x41]);
        crate::commands::parse_command(&mut app, "log OEP found at 0x401000");
        assert!(app.state == UIState::Normal);
        assert_eq!(app.logs.last().unwrap(), "[User] OEP found at 0x401000");

        // With colon and quotes: :log "hihi"
        crate::commands::parse_command(&mut app, ":log \"hihi\"");
        assert!(app.state == UIState::Normal);
        assert_eq!(app.logs.last().unwrap(), "[User] hihi");
    }

    #[test]
    fn log_command_clears_logs() {
        let mut app = app_with(&[0x41]);
        app.log("old log 1".to_string());
        app.log("old log 2".to_string());
        crate::commands::parse_command(&mut app, "log clear");
        assert!(app.state == UIState::Normal);
        assert_eq!(app.logs.len(), 1);
        assert!(app.logs[0].contains("Log cleared"));
    }

    #[test]
    fn log_command_saves_to_file() {
        let mut app = app_with(&[0x41]);
        app.log("test log entry 123".to_string());
        let temp_file = std::env::temp_dir().join(format!("test_save_log_{}.txt", std::process::id()));
        let cmd = format!("log save {}", temp_file.display());
        crate::commands::parse_command(&mut app, &cmd);
        assert!(app.state == UIState::Normal);
        assert!(temp_file.exists());
        let saved_content = std::fs::read_to_string(&temp_file).unwrap();
        assert!(saved_content.contains("test log entry 123"));
        let _ = std::fs::remove_file(&temp_file);
    }
}
