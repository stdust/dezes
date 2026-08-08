//! `:set` support: the option table, the settings dialog and name suggestions.
//!
//! `:set` used to be a wall of match arms and nothing else, which meant three
//! things a user could not do: see what any option is currently set to, find out
//! that a name was misspelled (an unknown option was silently ignored), and turn a
//! boolean option off in the same words used to turn it on. The list below is the
//! one place option names live, so all three come from the same source.

use ratatui::{
    Frame,
    layout::Alignment,
    symbols,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::io::Result;

use unicode_width::UnicodeWidthStr;

use crate::{app::App, editor::UIState, i18n::M, util::center_widget};

/// One row of the `:set` table.
pub struct Setting {
    pub name: &'static str,
    pub value: String,
    /// Translated description; `&'static str` because every translation is a
    /// literal in `i18n`.
    pub note: &'static str,
}

/// A settable disassembly colour.
///
/// The eight `:set disasm_*` commands were eight near-identical match arms; this
/// turns them into one lookup plus one assignment.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DisasmColor {
    Memory,
    Register,
    Immediate,
    Keyword,
    Segment,
    ImportBg,
    ImportFg,
    Comment,
}

/// Canonical option name for a colour, and the aliases it answers to.
const DISASM_COLORS: &[(DisasmColor, &str, &[&str])] = &[
    (DisasmColor::Memory, "disasm_mem", &["disasm_memory"]),
    (DisasmColor::Register, "disasm_reg", &["disasm_register"]),
    (DisasmColor::Immediate, "disasm_imm", &["disasm_immediate"]),
    (DisasmColor::Keyword, "disasm_kw", &["disasm_keyword"]),
    (DisasmColor::Segment, "disasm_seg", &["disasm_segment"]),
    (DisasmColor::ImportBg, "disasm_import", &["disasm_import_bg"]),
    (DisasmColor::ImportFg, "disasm_import_fg", &[]),
    (DisasmColor::Comment, "disasm_comment", &[]),
];

/// The colour an option name refers to, if it is one of them.
pub fn disasm_color_target(name: &str) -> Option<DisasmColor> {
    DISASM_COLORS.iter().find_map(|(target, canonical, aliases)| {
        (*canonical == name || aliases.contains(&name)).then_some(*target)
    })
}

pub fn set_disasm_color(
    theme: &mut crate::disasm::theme::DisasmTheme,
    target: DisasmColor,
    color: ratatui::style::Color,
) {
    match target {
        DisasmColor::Memory => theme.memory_op_fg = color,
        DisasmColor::Register => theme.register_fg = color,
        DisasmColor::Immediate => theme.immediate_fg = color,
        DisasmColor::Keyword => theme.keyword_fg = color,
        DisasmColor::Segment => theme.segment_fg = color,
        DisasmColor::ImportBg => theme.import_bg = color,
        DisasmColor::ImportFg => theme.import_fg = color,
        DisasmColor::Comment => theme.comment_fg = color,
    }
}

fn disasm_color_value(theme: &crate::disasm::theme::DisasmTheme, target: DisasmColor) -> String {
    let color = match target {
        DisasmColor::Memory => theme.memory_op_fg,
        DisasmColor::Register => theme.register_fg,
        DisasmColor::Immediate => theme.immediate_fg,
        DisasmColor::Keyword => theme.keyword_fg,
        DisasmColor::Segment => theme.segment_fg,
        DisasmColor::ImportBg => theme.import_bg,
        DisasmColor::ImportFg => theme.import_fg,
        DisasmColor::Comment => theme.comment_fg,
    };
    crate::disasm::theme::color_to_hex_str(color)
}

/// Every canonical option name, plus the aliases worth suggesting.
///
/// Used for "did you mean" on a misspelling, so aliases belong here too: a user
/// who types `hilite` should be pointed at `hilight`.
pub const OPTION_NAMES: &[&str] = &[
    "byteline",
    "width",
    "ctrlchar",
    "enc1",
    "enc2",
    "lang",
    "addr",
    "va",
    "offset",
    "bitness",
    "theme",
    "disasmtheme",
    "db",
    "dimctrl",
    "dimzero",
    "nodim",
    "wrapscan",
    "highlight",
    "hilight",
    "hintbar",
    "view",
    "disasm_mem",
    "disasm_reg",
    "disasm_imm",
    "disasm_kw",
    "disasm_seg",
    "disasm_import",
    "disasm_import_fg",
    "disasm_comment",
];

/// Closest known option name to `unknown`, if one is close enough to be worth
/// offering.
///
/// The threshold scales with the length of what was typed: one edit for a short
/// name, two for a longer one. Without that, `db` would "suggest" half the list.
pub fn suggest(unknown: &str) -> Option<&'static str> {
    let unknown = unknown.trim().to_ascii_lowercase();
    if unknown.is_empty() {
        return None;
    }
    let limit = if unknown.len() <= 4 { 1 } else { 2 };

    OPTION_NAMES
        .iter()
        .map(|name| (edit_distance(&unknown, name), *name))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, name)| (*distance, name.len()))
        .map(|(_, name)| name)
}

/// Levenshtein distance, two rows at a time.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn on_off(value: bool) -> String {
    if value { "on".to_string() } else { "off".to_string() }
}

/// Current value of every option, in the order the dialog shows them.
pub fn current_settings(app: &App) -> Vec<Setting> {
    let lang = app.config.lang;
    let mut rows = vec![
        Setting {
            name: "byteline",
            value: if app.config.hex_mode_bytes_per_line_auto {
                format!("auto ({})", app.config.hex_mode_bytes_per_line)
            } else {
                app.config.hex_mode_bytes_per_line.to_string()
            },
            note: M::NoteByteline.tr(lang),
        },
        Setting {
            name: "ctrlchar",
            value: app.config.hex_mode_non_graphic_char.to_string(),
            note: M::NoteCtrlchar.tr(lang),
        },
        Setting {
            name: "enc1",
            value: app.text_view.table.name().to_string(),
            note: M::NoteEnc1.tr(lang),
        },
        Setting {
            name: "enc2",
            value: match app.hex_view.enc2_table {
                Some(table) => table.name().to_string(),
                None => "none".to_string(),
            },
            note: M::NoteEnc2.tr(lang),
        },
        Setting {
            name: "lang",
            value: lang.label().to_string(),
            note: M::NoteLang.tr(lang),
        },
        Setting {
            name: "addr",
            value: if app.hex_view.show_va { "va".to_string() } else { "offset".to_string() },
            note: M::NoteAddr.tr(lang),
        },
        Setting {
            name: "bitness",
            value: app.describe_bitness(),
            note: M::NoteBitness.tr(lang),
        },
        Setting {
            name: "view",
            value: format!("{:?}", app.editor_view).to_lowercase(),
            note: M::NoteView.tr(lang),
        },
        Setting {
            name: "theme",
            value: app.config.theme.name.clone(),
            note: M::NoteTheme.tr(lang),
        },
        Setting {
            name: "db",
            value: on_off(app.config.database),
            note: M::NoteDb.tr(lang),
        },
        Setting {
            name: "dimctrl",
            value: on_off(app.config.dim_control_chars),
            note: M::NoteDimctrl.tr(lang),
        },
        Setting {
            name: "dimzero",
            value: on_off(app.config.dim_zeroes),
            note: M::NoteDimzero.tr(lang),
        },
        Setting {
            name: "wrapscan",
            value: on_off(app.config.search_wrap),
            note: M::NoteWrapscan.tr(lang),
        },
        Setting {
            name: "highlight",
            value: on_off(app.config.syntax_highlight),
            note: M::NoteHighlight.tr(lang),
        },
        Setting {
            name: "hintbar",
            value: on_off(app.config.hint_bar),
            note: M::NoteHintbar.tr(lang),
        },
    ];

    for (target, canonical, _) in DISASM_COLORS {
        rows.push(Setting {
            name: canonical,
            value: disasm_color_value(&app.config.disasm_theme, *target),
            note: M::NoteDisasmColor.tr(lang),
        });
    }

    rows
}

/// Renders the table as one block of text, so both the dialog and the log can use
/// it.
pub fn settings_text(app: &App) -> String {
    let rows = current_settings(app);
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(8);
    // Values can hold a translated language label (`ko (한국어)`), and CJK is
    // double-width, so the column is padded by display width rather than by
    // character count - otherwise the notes column comes out ragged.
    let value_width = rows
        .iter()
        .map(|r| UnicodeWidthStr::width(r.value.as_str()))
        .max()
        .unwrap_or(8);

    let mut out = String::with_capacity(rows.len() * 64);
    for row in &rows {
        let pad = value_width.saturating_sub(UnicodeWidthStr::width(row.value.as_str()));
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "  {:<name_width$}  {}{}  {}\n",
                row.name,
                row.value,
                " ".repeat(pad),
                row.note
            ),
        );
    }
    out
}

pub fn open_settings_dialog(app: &mut App) {
    app.settings_scroll_offset = 0;
    app.state = UIState::DialogSettings;
    app.dialog_renderer = Some(dialog_settings_draw);
}

pub fn dialog_settings_draw(app: &mut App, frame: &mut Frame) {
    let text = settings_text(app);
    let line_count = text.lines().count() as u16;

    let width = frame.area().width.saturating_sub(4).min(76).max(20);
    let height = (line_count + 2).min(frame.area().height.saturating_sub(2).max(6));
    let area = center_widget(width, height, frame.area());

    let max_scroll = line_count.saturating_sub(height.saturating_sub(2));
    app.settings_scroll_offset = app.settings_scroll_offset.min(max_scroll);

    let para = Paragraph::new(text)
        .style(app.config.theme.dialog)
        .wrap(Wrap { trim: false })
        .scroll((app.settings_scroll_offset, 0))
        .block(
            Block::new()
                .title(M::SettingsTitle.tr(app.config.lang))
                .title_bottom(M::SettingsFooter.tr(app.config.lang))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_set(symbols::border::PLAIN)
                .style(app.config.theme.dialog)
                .padding(Padding::horizontal(1)),
        );

    frame.render_widget(Clear, area);
    frame.render_widget(para, area);
}

pub fn dialog_settings_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            app.settings_scroll_offset = 0;
        }
        KeyCode::Down => {
            app.settings_scroll_offset = app.settings_scroll_offset.saturating_add(1);
        }
        KeyCode::Up => {
            app.settings_scroll_offset = app.settings_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.settings_scroll_offset = app.settings_scroll_offset.saturating_add(10);
        }
        KeyCode::PageUp => {
            app.settings_scroll_offset = app.settings_scroll_offset.saturating_sub(10);
        }
        KeyCode::Home => {
            app.settings_scroll_offset = 0;
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    /// Every option the table lists must be a name `:set` accepts, and vice versa:
    /// a name in one and not the other is how the two drift apart.
    #[test]
    fn the_table_and_the_name_list_agree() {
        let app = App::new();
        for row in current_settings(&app) {
            assert!(
                OPTION_NAMES.contains(&row.name),
                "'{}' is shown by ':set' but is not a known option name",
                row.name
            );
        }
    }

    /// A misspelling gets pointed at the right name; something unrelated does not
    /// get a bogus suggestion.
    #[test]
    fn suggestions_are_useful_and_not_noise() {
        assert_eq!(suggest("bytelin"), Some("byteline"));
        assert_eq!(suggest("hintbr"), Some("hintbar"));
        assert_eq!(suggest("wrapscn"), Some("wrapscan"));
        assert_eq!(suggest("dimzeros"), Some("dimzero"));
        assert_eq!(suggest("enc"), Some("enc1"));
        assert_eq!(suggest("qwertyuiop"), None);
        assert_eq!(suggest(""), None);
    }

    /// The colour lookup covers both spellings of each name.
    #[test]
    fn disasm_colour_names_resolve() {
        assert_eq!(disasm_color_target("disasm_mem"), Some(DisasmColor::Memory));
        assert_eq!(disasm_color_target("disasm_memory"), Some(DisasmColor::Memory));
        assert_eq!(disasm_color_target("disasm_import_fg"), Some(DisasmColor::ImportFg));
        assert_eq!(disasm_color_target("theme"), None);
    }

    /// The rendered table has one line per option, name first.
    #[test]
    fn the_text_has_a_line_per_option() {
        let app = App::new();
        let text = settings_text(&app);
        assert_eq!(text.lines().count(), current_settings(&app).len());
        assert!(text.contains("byteline"));
        assert!(text.contains("hintbar"));
    }
}
