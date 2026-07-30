use ratatui::{
    Frame,
    crossterm::event::{KeyCode, KeyEvent},
};

use crate::{app::App, editor::UIState, widgets::ListChoice};

use std::io::Result;

/// The encodings the picker offers, label and table together.
///
/// One row per encoding, because the label and the encoding used to be two
/// independent lists indexed by the same number: `ENCODING_LIST` for what is drawn
/// and a `match sel` for what gets selected. Inserting a row in one and not the
/// other would have silently selected the neighbour, and the `_ =>` fallback at the
/// end of that match turned an off-by-one into "UTF-8" rather than a visible fault.
/// This is the same shape of bug that made every COFF header row edit the field
/// below it.
const ENCODINGS: [(&str, &encoding_rs::Encoding); 7] = [
    ("UTF-8", encoding_rs::UTF_8),
    ("CP949", encoding_rs::EUC_KR),
    ("CP936 (GBK)", encoding_rs::GBK),
    ("ISO-8859-1", encoding_rs::WINDOWS_1252),
    ("ISO-8859-2", encoding_rs::ISO_8859_2),
    ("UTF-16-LE", encoding_rs::UTF_16LE),
    ("UTF-16-BE", encoding_rs::UTF_16BE),
];

/// Label of the extra first row in the secondary picker, which can be switched off.
const NONE_LABEL: &str = "None (Disabled)";

/// Resolves a user-typed encoding name to an `encoding_rs` table.
///
/// Single source of truth for `:set enc1`, `:set enc2` and the `.dz6init`
/// file, which each used to carry their own copy of this `if/else` chain -
/// so adding a codepage meant remembering all of them.
///
/// Accepts the canonical `encoding_rs` name (what `save_initfile` writes) plus
/// the common aliases people actually type. Returns `None` for anything
/// unrecognised, including `"none"`; callers that allow disabling (only
/// `enc2`) check for that themselves.
pub fn encoding_from_name(name: &str) -> Option<&'static encoding_rs::Encoding> {
    let v = name.trim();
    let matches = |candidates: &[&str]| candidates.iter().any(|c| v.eq_ignore_ascii_case(c));

    if matches(&["utf-8", "utf8"]) {
        Some(encoding_rs::UTF_8)
    } else if matches(&["euc-kr", "cp949", "ks_c_5601-1987", "korean"]) {
        Some(encoding_rs::EUC_KR)
    } else if matches(&["gbk", "cp936", "chinese"]) {
        Some(encoding_rs::GBK)
    } else if matches(&["iso-8859-1", "windows-1252", "cp1252", "latin1"]) {
        Some(encoding_rs::WINDOWS_1252)
    } else if matches(&["iso-8859-2", "latin2"]) {
        Some(encoding_rs::ISO_8859_2)
    } else if matches(&["utf-16le", "utf-16-le", "utf16le", "unicode"]) {
        Some(encoding_rs::UTF_16LE)
    } else if matches(&["utf-16be", "utf-16-be", "utf16be"]) {
        Some(encoding_rs::UTF_16BE)
    } else {
        None
    }
}

/// True if `name` means "turn the secondary encoding off".
pub fn is_encoding_none(name: &str) -> bool {
    let v = name.trim();
    v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("disabled") || v.eq_ignore_ascii_case("off")
}

/// Row labels of the primary picker.
fn enc1_labels() -> Vec<String> {
    ENCODINGS.iter().map(|(label, _)| label.to_string()).collect()
}

/// Row labels of the secondary picker: the same list with "off" in front, so the
/// two cannot drift apart.
fn enc2_labels() -> Vec<String> {
    let mut labels = vec![NONE_LABEL.to_string()];
    labels.extend(enc1_labels());
    labels
}

/// The encoding a row of the primary picker selects.
fn enc1_at(row: usize) -> &'static encoding_rs::Encoding {
    ENCODINGS.get(row).map(|(_, enc)| *enc).unwrap_or(encoding_rs::UTF_8)
}

/// The encoding a row of the secondary picker selects. Row 0 is "off".
fn enc2_at(row: usize) -> Option<&'static encoding_rs::Encoding> {
    row.checked_sub(1).and_then(|i| ENCODINGS.get(i)).map(|(_, enc)| *enc)
}

pub fn dialog_encoding_draw(app: &mut App, frame: &mut Frame) {
    let mut dialog = ListChoice::new();
    dialog.set_title(" Select Encoding 1 (Primary) ".to_string());
    dialog.choices = enc1_labels();
    dialog.render(app, frame);
}

pub fn dialog_encoding2_draw(app: &mut App, frame: &mut Frame) {
    let mut dialog = ListChoice::new();
    dialog.set_title(" Select Encoding 2 (Secondary) ".to_string());
    dialog.choices = enc2_labels();
    dialog.render(app, frame);
}

pub fn dialog_encoding_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // quit
        KeyCode::Esc => {
            app.state = UIState::Normal;
            app.dialog_renderer = None;
        }
        // switch
        KeyCode::Enter => {
            let sel = app.list_state.selected().unwrap_or(0);
            app.text_view.table = enc1_at(sel);

            app.state = UIState::Normal;
            app.dialog_renderer = None;
            app.save_initfile();
        }
        KeyCode::Down => {
            if app.list_state.selected() == Some(ENCODINGS.len() - 1) {
                app.list_state.select_first();
            } else {
                app.list_state.select_next();
            }
        }
        KeyCode::Up => {
            if app.list_state.selected() == Some(0) {
                app.list_state.select_last();
            } else {
                app.list_state.select_previous();
            }
        }
        KeyCode::PageUp | KeyCode::Home => {
            app.list_state.select_first();
        }
        KeyCode::PageDown | KeyCode::End => {
            app.list_state.select_last();
        }
        _ => {}
    }
    Ok(false)
}

pub fn dialog_encoding2_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.state = UIState::Normal;
            app.dialog_renderer = None;
        }
        KeyCode::Enter => {
            let sel = app.list_state.selected().unwrap_or(0);
            app.hex_view.enc2_table = enc2_at(sel);

            app.state = UIState::Normal;
            app.dialog_renderer = None;
            app.save_initfile();
        }
        KeyCode::Down => {
            if app.list_state.selected() == Some(ENCODINGS.len()) {
                app.list_state.select_first();
            } else {
                app.list_state.select_next();
            }
        }
        KeyCode::Up => {
            if app.list_state.selected() == Some(0) {
                app.list_state.select_last();
            } else {
                app.list_state.select_previous();
            }
        }
        KeyCode::PageUp | KeyCode::Home => {
            app.list_state.select_first();
        }
        KeyCode::PageDown | KeyCode::End => {
            app.list_state.select_last();
        }
        _ => {}
    }
    Ok(false)
}



#[cfg(test)]
mod list_alignment_tests {
    use super::*;

    /// Every row of the primary picker selects the encoding its label names.
    ///
    /// The label and the encoding were two lists indexed by the same number; this is
    /// what would have caught them drifting apart.
    #[test]
    fn every_row_selects_what_it_says() {
        for (row, (label, expected)) in ENCODINGS.iter().enumerate() {
            let picked = enc1_at(row);
            assert_eq!(
                picked.name(),
                expected.name(),
                "row {} is labelled '{}' but selects {}",
                row,
                label,
                picked.name()
            );
        }
        assert_eq!(enc1_labels().len(), ENCODINGS.len());
    }

    /// The secondary picker is the same list with "off" at row 0, and every row is
    /// shifted by exactly one.
    #[test]
    fn the_secondary_picker_is_offset_by_one_row() {
        let labels = enc2_labels();
        assert_eq!(labels.len(), ENCODINGS.len() + 1);
        assert_eq!(labels[0], NONE_LABEL);
        assert!(enc2_at(0).is_none(), "row 0 has to mean off");

        for (row, (label, expected)) in ENCODINGS.iter().enumerate() {
            assert_eq!(labels[row + 1], *label);
            let picked = enc2_at(row + 1).expect("a real encoding");
            assert_eq!(
                picked.name(),
                expected.name(),
                "row {} is labelled '{}' but selects {}",
                row + 1,
                label,
                picked.name()
            );
        }

        // Past the end selects nothing rather than the first encoding.
        assert!(enc2_at(labels.len()).is_none());
    }

    /// Every label the picker shows is also a name `:set enc1` accepts, so the two
    /// ways in agree.
    ///
    /// `CP936 (GBK)` carries a parenthetical for the reader; the name before the
    /// space is what has to resolve.
    #[test]
    fn the_labels_are_names_the_set_command_accepts() {
        for (label, expected) in ENCODINGS.iter() {
            let typed = label.split_whitespace().next().unwrap_or(label);
            let resolved = encoding_from_name(typed)
                .unwrap_or_else(|| panic!("':set enc1 {}' is not accepted", typed));
            assert_eq!(
                resolved.name(),
                expected.name(),
                "'{}' resolves to {} but the picker row selects {}",
                typed,
                resolved.name(),
                expected.name()
            );
        }
    }
}