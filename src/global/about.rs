//! About / program information dialog (F8, or `:about`).
//!
//! Two kinds of information live here, and they are why this is a dialog of its
//! own rather than another section appended to the F1 help text:
//!
//! * Identity and licensing - constant, and required to be reachable because
//!   this build is a modified derivative of a GPL-licensed program.
//! * Resolved config paths - runtime values that differ per machine and per
//!   launch directory, so they can't live in a `const` string. These exist
//!   because dz6 looks for `.dz6init`, `themes/` and `disasm.theme` in more
//!   than one directory; when a setting appears to be ignored, seeing which
//!   candidate actually won is the whole diagnosis.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame, symbols,
    layout::{Alignment, Rect},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};
use std::io::Result;
use std::path::{Path, PathBuf};

use crate::{app::App, editor::UIState, util::center_widget};

/// Maintainer of this build, credited in the notice below.
pub const BUILD_AUTHOR: &str = "stdust";

/// Authorship and licence notice, kept as one indivisible block.
///
/// GPL-3.0 requires a modified version to carry the original authorship and
/// licence notices and to state that it has been changed, so this is built in
/// one place rather than assembled at the call site - it can be repositioned in
/// the layout but not partially deleted, and `notice_is_complete` in the tests
/// below fails if any of it goes missing.
///
/// Both credits belong here for the same reason: this build's own work is
/// substantial (the tree is now ~26,000 lines across 79 files, most of it not in
/// upstream), while the base it grew from is still upstream's, so neither party
/// should be the only name on screen.
///
/// A function rather than a `concat!` constant because `concat!` only accepts
/// literals, which would mean spelling the author's name out four times.
fn notice() -> String {
    format!(
        // No `\` line continuation after the opening quote: it would swallow the
        // leading space of the next line and unalign the first section heading.
        " THIS BUILD
  Lead developer: {author}
  Disassembly view, assembler, PE section tools, wildcard
  pattern search/replace, xref and string-reference scans,
  header field editing, dialogs and theming.
  Context hint line with Ctrl/Alt pages, interface
  languages (English / 한국어 / 中文), reworked ':set' with a
  settings table and real errors, read-only feedback on
  every refused key, comment list editing, faster
  disassembly navigation, and the Windows icon and version
  resource.

 ORIGINAL WORK
  This is a MODIFIED build, not an official release.
  Modified in 2026 by {author}. Please report issues with
  this build to {author}, not to the original authors.
  It is based on dz6 by Mente Binaria and contributors:
    Fernando Merces (merces) - original author
    yeggor, sergiogarciadev, param-jasani
    https://github.com/mentebinaria/dz6
    https://menteb.in/dz6
  Thank you for releasing dz6 as free software.

 LICENSE
  {license}
  This program comes with ABSOLUTELY NO WARRANTY.
  You may redistribute it under the terms of the GNU
  General Public License version 3 or later. The full
  text is in the COPYING file shipped with the source.",
        author = BUILD_AUTHOR,
        license = env!("CARGO_PKG_LICENSE"),
    )
}

/// Left-aligned `key   value` row used by the paths and settings sections.
///
/// The pad has to exceed the longest key, not merely match it, or that one row
/// comes out with no gap at all ("Bytes per line16").
fn row(key: &str, value: impl AsRef<str>) -> String {
    format!("  {:<16}{}", key, value.as_ref())
}

/// Renders a path that may not have been resolved at all.
fn path_or(value: Option<&Path>, fallback: &str) -> String {
    match value {
        Some(p) => p.display().to_string(),
        None => fallback.to_string(),
    }
}

/// Marks a path with whether it currently exists, so a missing config file is
/// visible without leaving the dialog.
fn path_with_state(path: &Path) -> String {
    if path.is_file() {
        format!("{}  (found)", path.display())
    } else {
        format!("{}  (not found)", path.display())
    }
}

/// The `<file>.dz6` sidecar dz6 would use for the file that's open, mirroring
/// the lookup in `database.rs`.
fn database_sidecar(app: &App) -> String {
    if !app.config.database {
        return "off  (':set db' to enable)".to_string();
    }
    if app.file_info.name.is_empty() {
        return "on   (no file open)".to_string();
    }

    let db_name = format!("{}.{}", app.file_info.name, crate::app::DB_EXT);
    let beside_file = Path::new(&app.file_info.path)
        .parent()
        .unwrap_or(Path::new("."))
        .join(&db_name);
    let beside_startup = crate::util::startup_dir().join(&db_name);

    for candidate in [&beside_startup, &beside_file] {
        if candidate.is_file() {
            return format!("on   {}", candidate.display());
        }
    }
    format!("on   {}  (none yet)", beside_file.display())
}

/// Full dialog body. Also what the copy action puts on the clipboard, so a bug
/// report can't disagree with what the user was looking at.
pub fn about_text(app: &App) -> String {
    let exe = std::env::current_exe().ok();
    let exe_dir = crate::util::exe_dir();
    let startup_dir = crate::util::startup_dir();

    let enc1 = app.text_view.table.name();
    let encodings = match app.hex_view.enc2_table {
        Some(enc2) => format!("{} | {}", enc1, enc2.name()),
        None => format!("{} | none", enc1),
    };

    let mut lines: Vec<String> = Vec::new();

    // The build author is in the title line as well as the notice, so a
    // screenshot makes it obvious this isn't the official 0.8.0 release.
    lines.push(format!(
        " Dezes {} - {} build",
        env!("CARGO_PKG_VERSION"),
        BUILD_AUTHOR
    ));
    lines.push(format!(" {}", env!("CARGO_PKG_DESCRIPTION")));
    lines.push(format!(
        " {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    lines.push(String::new());

    lines.push(notice());
    lines.push(String::new());

    lines.push(" PATHS".to_string());
    lines.push(row("Executable", path_or(exe.as_deref(), "unknown")));
    lines.push(row("Exe dir", exe_dir.display().to_string()));
    lines.push(row("Startup dir", startup_dir.display().to_string()));
    lines.push(row(
        "Init file",
        match app.initfile_loaded.as_deref() {
            Some(p) => format!("{}  (loaded)", p.display()),
            None => format!(
                "{}  (none of 3 candidates found)",
                crate::app::INIT_FILE
            ),
        },
    ));
    lines.push(row(
        "Themes dir",
        path_or(Some(&exe_dir.join("themes")), "unknown"),
    ));
    lines.push(row("Disasm theme", {
        let p: PathBuf = crate::disasm::theme::get_theme_config_path();
        path_with_state(&p)
    }));
    lines.push(row("Database", database_sidecar(app)));
    lines.push(String::new());

    lines.push(" SETTINGS".to_string());
    lines.push(row("Theme", &app.config.theme.name));
    lines.push(row("Encoding 1|2", encodings));
    lines.push(row(
        "Bytes per line",
        if app.config.hex_mode_bytes_per_line_auto {
            format!("{} (auto)", app.config.hex_mode_bytes_per_line)
        } else {
            app.config.hex_mode_bytes_per_line.to_string()
        },
    ));

    lines.join("\n")
}

/// Outer width of the About box. Paths are the widest content, so this is a bit
/// roomier than the help box and still clamps on narrow terminals.
fn about_box_width(area: Rect) -> u16 {
    (area.width.saturating_sub(4)).min(84).max(20)
}

/// Rows the text occupies once wrapped, so the scroll bound matches what is
/// actually drawn rather than the raw line count.
fn about_row_count(text: &str, box_width: u16) -> u16 {
    let text_width = box_width.saturating_sub(4).max(1) as usize;
    text.lines()
        .map(|line| (line.chars().count().div_ceil(text_width)).max(1) as u16)
        .sum()
}

fn about_box_height(rows: u16, area: Rect) -> u16 {
    let avail = area.height.saturating_sub(4).max(8);
    (rows + 2).min(avail)
}

fn max_about_scroll(rows: u16, box_height: u16) -> u16 {
    rows.saturating_sub(box_height.saturating_sub(2))
}

pub fn dialog_about_draw(app: &mut App, frame: &mut Frame) {
    let text = about_text(app);
    let width = about_box_width(frame.area());
    let rows = about_row_count(&text, width);
    let height = about_box_height(rows, frame.area());
    let dialog_area = center_widget(width, height, frame.area());

    // Re-clamped every frame: the terminal can be resized while the dialog is
    // open, shrinking a scroll offset that was legal a moment ago.
    app.about_scroll_offset = app
        .about_scroll_offset
        .min(max_about_scroll(rows, dialog_area.height));

    let block = Block::new()
        .title(crate::i18n::M::AboutTitle.tr(app.config.lang))
        .title_bottom(crate::i18n::M::AboutFooter.tr(app.config.lang))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_set(symbols::border::DOUBLE)
        .style(app.config.theme.dialog)
        .padding(Padding::horizontal(1));

    let para = Paragraph::new(text)
        .style(app.config.theme.dialog)
        .wrap(Wrap { trim: false })
        .scroll((app.about_scroll_offset, 0))
        .block(block);

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(para, dialog_area);
}

pub fn dialog_about_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Recomputed from the same geometry the renderer uses so the scroll keys
    // stop exactly where the last line reaches the bottom of the box.
    let text = about_text(app);
    let box_width = about_box_width(app.screen);
    let rows = about_row_count(&text, box_width);
    let box_height = about_box_height(rows, app.screen);
    let max_scroll = max_about_scroll(rows, box_height);

    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(8) => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            app.about_scroll_offset = 0;
        }
        // Copy the whole panel, so version, paths and settings can be pasted
        // into a bug report in one go.
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let copied = match app.clipboard.as_mut() {
                Ok(clip) => clip.set_text(text).is_ok(),
                Err(_) => false,
            };
            if copied {
                App::log(app, "Copied program info to clipboard".to_string());
            } else {
                App::log(app, "Could not access the clipboard".to_string());
                crate::beep!();
            }
        }
        KeyCode::Down => {
            app.about_scroll_offset = (app.about_scroll_offset + 1).min(max_scroll);
        }
        KeyCode::Up => {
            app.about_scroll_offset = app.about_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.about_scroll_offset = (app.about_scroll_offset + 10).min(max_scroll);
        }
        KeyCode::PageUp => {
            app.about_scroll_offset = app.about_scroll_offset.saturating_sub(10);
        }
        KeyCode::Home => {
            app.about_scroll_offset = 0;
        }
        KeyCode::End => {
            app.about_scroll_offset = max_scroll;
        }
        _ => {}
    }
    Ok(false)
}

impl App {
    pub fn open_about_dialog(&mut self) {
        self.about_scroll_offset = 0;
        self.state = UIState::DialogAbout;
        self.dialog_renderer = Some(dialog_about_draw);
    }
}

#[cfg(test)]
mod about_tests {
    use super::*;

    /// The attribution and licence text is a GPL obligation for a modified
    /// build, so this guards against it being trimmed away while the layout
    /// around it is edited.
    #[test]
    fn notice_is_complete() {
        let notice = notice();
        for required in [
            BUILD_AUTHOR,
            "Lead developer",
            "MODIFIED build",
            // GPL-3.0 section 5(a) wants the modification date stated, not just
            // the fact of modification.
            "Modified in 2026",
            "Mente Binaria",
            "merces",
            "github.com/mentebinaria/dz6",
            "menteb.in/dz6",
            "GPL-3.0",
            "NO WARRANTY",
            "COPYING",
        ] {
            assert!(
                notice.contains(required),
                "attribution/licence notice lost '{}'",
                required
            );
        }
    }

    #[test]
    fn notice_reports_the_crate_license() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-or-later");
    }

    /// Rows must never be zero, otherwise `max_about_scroll` underflows into a
    /// huge bound and the panel can be scrolled entirely off screen.
    #[test]
    fn empty_lines_still_occupy_a_row() {
        assert_eq!(about_row_count("\n\n", 40), 2);
        assert_eq!(about_row_count("abc", 40), 1);
    }

    /// A line wider than the box counts as several rows, so the scroll bound
    /// reaches the real bottom of the text.
    #[test]
    fn long_lines_count_as_multiple_rows() {
        // Box width 24 leaves 20 columns for text after borders and padding.
        let line = "x".repeat(41);
        assert_eq!(about_row_count(&line, 24), 3);
    }

    #[test]
    fn scroll_bound_is_zero_when_text_fits() {
        assert_eq!(max_about_scroll(5, 20), 0);
    }
}
