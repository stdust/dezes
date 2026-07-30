//! Find Pattern dialog (Ctrl+F).
//!
//! Same box layout as the Edit Data dialog (ASCII/UTF-8/UNICODE/Hex fields),
//! so the same muscle memory (Tab/Up/Down to switch fields, Enter to commit)
//! applies. Unlike Edit Data, the four fields are independent - each is just
//! a different way to spell out what to search for, not four views of the
//! same bytes - so typing in one does not overwrite the others. Whichever
//! field is focused when Enter is pressed is the one that's searched:
//!
//!   - ANSI (the configured primary encoding) / UTF-8 / UNICODE (LE): the text
//!     is encoded and searched for as an exact byte sequence.
//!   - Hex: parsed as a wildcard hex pattern (`??` matches any byte), exactly
//!     like the old hex-only search.

use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers},
};
use std::io::Result;
use tui_input::Input;

use crate::app::App;
use crate::editor::UIState;
use crate::hex::edit_dialog::EditDialogFocus;
use crate::hex::field_box::{self, FieldRow};
use crate::hex::search::{HexPatternByte, found_at_message, hex_string_to_pattern, search_pattern};

#[derive(Default)]
pub struct FindDialog {
    /// Character a Shift-selection started from, or `None`. One per dialog rather
    /// than per field: only the focused field can have a block, and switching
    /// fields drops it.
    pub anchor: Option<usize>,
    pub input_enc1: Input,
    pub input_utf8: Input,
    pub input_utf16le: Input,
    pub input_hex: Input,
    pub focus: EditDialogFocus,
    pub error_message: Option<String>,
    pub status_message: Option<String>,
}

impl FindDialog {
    pub fn reset(&mut self) {
        self.input_enc1 = Input::default();
        self.input_utf8 = Input::default();
        self.input_utf16le = Input::default();
        self.input_hex = Input::default();
        self.focus = EditDialogFocus::Enc1;
        self.anchor = None;
        self.error_message = None;
        self.status_message = None;
    }

    #[allow(dead_code)]
    fn active_input_mut(&mut self) -> &mut Input {
        match self.focus {
            EditDialogFocus::Enc1 => &mut self.input_enc1,
            EditDialogFocus::Utf8 => &mut self.input_utf8,
            EditDialogFocus::Utf16Le => &mut self.input_utf16le,
            EditDialogFocus::Hex => &mut self.input_hex,
        }
    }

    /// Re-renders the other three fields from whatever the focused one now holds.
    ///
    /// The fields used to be independent, which meant typing `分析` into UNICODE
    /// (LE) gave no way to see the bytes it would look for - and getting that wrong
    /// silently is exactly what the UTF-16 bug was. Now every field is a view of the
    /// same byte string, as in the Edit Data dialog.
    ///
    /// The one asymmetry is wildcards: `??` in the Hex field has no text spelling,
    /// so a pattern containing one leaves the text fields empty rather than
    /// inventing characters for bytes that are not fixed.
    fn sync_from_focus(&mut self, enc1: &'static encoding_rs::Encoding) {
        let bytes = match self.focus {
            EditDialogFocus::Enc1 => {
                crate::util::encode_text(self.input_enc1.value(), enc1)
            }
            EditDialogFocus::Utf8 => {
                crate::util::encode_text(self.input_utf8.value(), encoding_rs::UTF_8)
            }
            EditDialogFocus::Utf16Le => {
                crate::util::encode_text(self.input_utf16le.value(), encoding_rs::UTF_16LE)
            }
            EditDialogFocus::Hex => {
                let raw = self.input_hex.value().to_string();
                if raw.contains('?') {
                    // A wildcard pattern: the text fields cannot represent it.
                    self.input_enc1 = Input::default();
                    self.input_utf8 = Input::default();
                    self.input_utf16le = Input::default();
                    return;
                }
                let digits: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                let even = digits.len() - (digits.len() % 2);
                hex::decode(&digits[..even]).unwrap_or_default()
            }
        };

        if self.focus != EditDialogFocus::Enc1 {
            let (s, _) = enc1.decode_without_bom_handling(&bytes);
            self.input_enc1 = Input::new(s.into_owned());
        }
        if self.focus != EditDialogFocus::Utf8 {
            let s = String::from_utf8_lossy(&bytes);
            let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
            self.input_utf8 = Input::new(s.to_string());
        }
        if self.focus != EditDialogFocus::Utf16Le {
            let (s, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(&bytes);
            self.input_utf16le = Input::new(s.into_owned());
        }
        if self.focus != EditDialogFocus::Hex {
            self.input_hex = Input::new(hex::encode_upper(&bytes));
        }
    }
}

/// First row's label comes from the active encoding, so only the fixed rows
/// are listed here.
const FIELD_ROWS: [(&str, EditDialogFocus); 3] = [
    ("UTF-8", EditDialogFocus::Utf8),
    ("UNICODE (LE)", EditDialogFocus::Utf16Le),
    ("Hex", EditDialogFocus::Hex),
];

pub fn draw_find_dialog(app: &mut App, frame: &mut Frame) {
    let enc1_row_label = crate::hex::edit_dialog::enc1_label(app.text_view.table);
    let mut labels: Vec<&str> = vec![enc1_row_label.as_str()];
    labels.extend(FIELD_ROWS.iter().map(|(l, _)| *l));

    let rows: Vec<(&str, EditDialogFocus)> = labels
        .iter()
        .copied()
        .zip(
            std::iter::once(EditDialogFocus::Enc1)
                .chain(FIELD_ROWS.iter().map(|(_, f)| *f)),
        )
        .collect();

    let label_width = field_box::label_width(&labels);
    // Two status rows: the result of the last search, and the keys - which stay put
    // instead of being overwritten by the result.
    let inner_area = field_box::draw_box_rows(
        app,
        frame,
        format!(
            " {} (Ctrl+B) ",
            crate::i18n::M::FindPatternTitle.tr(app.config.lang)
        ),
        label_width,
        rows.len(),
        2,
    );

    let focus = app.hex_view.find_dialog.focus;
    let inputs = [
        &app.hex_view.find_dialog.input_enc1,
        &app.hex_view.find_dialog.input_utf8,
        &app.hex_view.find_dialog.input_utf16le,
        &app.hex_view.find_dialog.input_hex,
    ];

    let anchor = app.hex_view.find_dialog.anchor;
    let mut cursor_pos: Option<(u16, u16)> = None;
    for (row, (label, field_focus)) in rows.iter().enumerate() {
        let is_focused = *field_focus == focus;
        let field = FieldRow {
            label,
            input: inputs[row],
            focused: is_focused,
            // `draw_field_row` already knows how to paint a block; the field just
            // never had one to give it.
            selection: if is_focused {
                crate::text_field::selection(inputs[row], anchor)
            } else {
                None
            },
        };
        if let Some(pos) = field_box::draw_field_row(app, frame, inner_area, row as u16, label_width, &field) {
            cursor_pos = Some(pos);
        }
    }

    let lang = app.config.lang;
    let dialog = &app.hex_view.find_dialog;
    let result_text = if let Some(ref err) = dialog.error_message {
        format!("  {}: {}", crate::i18n::M::LblError.tr(lang), err)
    } else if let Some(ref msg) = dialog.status_message {
        format!("  {}", msg)
    } else {
        String::new()
    };
    let n_rows = rows.len() as u16;
    field_box::draw_status_row_at(app, frame, inner_area, n_rows, 0, &result_text);
    field_box::draw_status_row_at(
        app,
        frame,
        inner_area,
        n_rows,
        1,
        &format!("  {}", crate::i18n::M::FindHint.tr(lang)),
    );

    if let Some((x, y)) = cursor_pos {
        frame.set_cursor_position((x, y));
    }
}

pub fn dialog_find_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.hex_view.find_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            // Changing field drops the block: it belonged to the field being left.
            KeyCode::Tab | KeyCode::Down => {
                app.hex_view.find_dialog.focus = app.hex_view.find_dialog.focus.next();
                app.hex_view.find_dialog.anchor = None;
                return Ok(false);
            }
            KeyCode::BackTab | KeyCode::Up => {
                app.hex_view.find_dialog.focus = app.hex_view.find_dialog.focus.prev();
                app.hex_view.find_dialog.anchor = None;
                return Ok(false);
            }
            // Enter / F3 step forward, Shift+Enter / Shift+F3 step back, so the hit
            // list can be walked without closing the dialog. Same pair the view uses
            // outside it.
            KeyCode::Enter | KeyCode::F(3) => {
                let forward = !key.modifiers.contains(KeyModifiers::SHIFT);
                execute_find(app, forward);
                return Ok(false);
            }
            // The Hex field only ever holds hex digits, spaces, an "0x"
            // prefix, and "?" wildcards - anything else is silently dropped,
            // matching the old hex-only search field's behavior.
            KeyCode::Char(c)
                if app.hex_view.find_dialog.focus == EditDialogFocus::Hex
                    && !(c.is_ascii_hexdigit() || c == 'x' || c == 'X' || c == ' ' || c == '?') =>
            {
                return Ok(false);
            }
            _ => {}
        }
    }

    // Shift+arrows, Shift+Home/End and Ctrl+C/X/V over the block, via the shared
    // text-field handling.
    crate::text_field::handle_key(app, find_field, event);
    // Every keystroke re-renders the other fields, so the Hex row always shows the
    // bytes that will actually be searched for.
    let enc1 = app.text_view.table;
    app.hex_view.find_dialog.sync_from_focus(enc1);
    Ok(false)
}

/// The focused field of the Find dialog, and the dialog's selection anchor.
fn find_field(app: &mut App) -> (&mut Input, &mut Option<usize>) {
    let dialog = &mut app.hex_view.find_dialog;
    let input = match dialog.focus {
        EditDialogFocus::Enc1 => &mut dialog.input_enc1,
        EditDialogFocus::Utf8 => &mut dialog.input_utf8,
        EditDialogFocus::Utf16Le => &mut dialog.input_utf16le,
        EditDialogFocus::Hex => &mut dialog.input_hex,
    };
    (input, &mut dialog.anchor)
}

/// Encodes `text` with `encoding` and turns the resulting bytes into an
/// exact-match pattern (no wildcards) for [`search_pattern`].
fn text_to_exact_pattern(text: &str, encoding: &'static encoding_rs::Encoding) -> Option<Vec<HexPatternByte>> {
    if text.is_empty() {
        return None;
    }
    // `crate::util::encode_text`, not `encoding.encode`: encoding_rs turns a
    // UTF-16 target into UTF-8, so searching the UNICODE (LE) field used to look
    // for the UTF-8 bytes of the text and never match.
    let bytes = crate::util::encode_text(text, encoding);
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.iter().map(|&b| HexPatternByte::Exact(b)).collect())
}

fn execute_find(app: &mut App, forward: bool) {
    // `search_pattern` picks the next hit relative to the cursor in this direction,
    // so stepping backwards is just a direction change plus the same search.
    app.hex_view.search.direction = if forward {
        crate::hex::search::SearchDirection::Forward
    } else {
        crate::hex::search::SearchDirection::Backward
    };

    let focus = app.hex_view.find_dialog.focus;

    let pattern = match focus {
        EditDialogFocus::Hex => {
            let search_str = app.hex_view.find_dialog.input_hex.value().to_string();
            if search_str.trim().is_empty() {
                app.hex_view.find_dialog.error_message = None;
                app.hex_view.find_dialog.status_message = Some(crate::i18n::M::FindEnterHex.tr(app.config.lang).to_string());
                return;
            }
            match hex_string_to_pattern(&search_str) {
                Some(p) => p,
                None => {
                    app.hex_view.find_dialog.error_message = Some(crate::i18n::M::FindInvalidHex.tr(app.config.lang).to_string());
                    app.hex_view.find_dialog.status_message = None;
                    return;
                }
            }
        }
        EditDialogFocus::Enc1 => {
            let text = app.hex_view.find_dialog.input_enc1.value().to_string();
            match text_to_exact_pattern(&text, app.text_view.table) {
                Some(p) => p,
                None => {
                    app.hex_view.find_dialog.error_message = None;
                    app.hex_view.find_dialog.status_message = Some(crate::i18n::M::FindEnterText.tr(app.config.lang).to_string());
                    return;
                }
            }
        }
        EditDialogFocus::Utf8 => {
            let text = app.hex_view.find_dialog.input_utf8.value().to_string();
            match text_to_exact_pattern(&text, encoding_rs::UTF_8) {
                Some(p) => p,
                None => {
                    app.hex_view.find_dialog.error_message = None;
                    app.hex_view.find_dialog.status_message = Some(crate::i18n::M::FindEnterText.tr(app.config.lang).to_string());
                    return;
                }
            }
        }
        EditDialogFocus::Utf16Le => {
            let text = app.hex_view.find_dialog.input_utf16le.value().to_string();
            match text_to_exact_pattern(&text, encoding_rs::UTF_16LE) {
                Some(p) => p,
                None => {
                    app.hex_view.find_dialog.error_message = None;
                    app.hex_view.find_dialog.status_message = Some(crate::i18n::M::FindEnterText.tr(app.config.lang).to_string());
                    return;
                }
            }
        }
    };

    if let Some(ofs) = search_pattern(app, &pattern) {
        let msg = found_at_message(app, ofs);
        // Scrolled clear of this dialog, and highlighted, so the hit is actually
        // visible behind the box instead of only being reported in the status line.
        let enc1_label = crate::hex::edit_dialog::enc1_label(app.text_view.table);
        let labels = [
            enc1_label.as_str(),
            "UTF-8",
            "UNICODE (LE)",
            "Hex",
        ];
        let label_width = field_box::label_width(&labels);
        let dialog = field_box::dialog_rect(app, label_width, labels.len(), 2);
        field_box::reveal_behind_dialog(app, ofs, dialog);

        let end = (ofs + pattern.len().saturating_sub(1))
            .min(app.file_info.size.saturating_sub(1))
            .max(ofs);
        app.hex_view.selection.start = ofs;
        app.hex_view.selection.end = end;
        app.hex_view.selection.direction = None;
        app.hex_view.selection.is_mouse = false;
        app.hex_view.selection_target = crate::editor::EditingTarget::Hex;

        app.hex_view.find_dialog.error_message = None;
        app.hex_view.find_dialog.status_message = Some(msg);
    } else {
        app.hex_view.find_dialog.error_message = None;
        app.hex_view.find_dialog.status_message = Some(crate::i18n::M::FindNoMatch.tr(app.config.lang).to_string());
    }
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    /// `分析` in UTF-16LE is `06 52 90 67`, not its UTF-8 bytes.
    ///
    /// encoding_rs maps the UTF-16 labels to UTF-8 when encoding, so
    /// `UTF_16LE.encode()` returned `E5 88 86 E6 9E 90` and the UNICODE (LE) field
    /// searched for a byte sequence that is not in the file - x64dbg found the
    /// string, dz6 did not.
    #[test]
    fn utf16le_text_encodes_to_utf16_bytes() {
        let pattern = text_to_exact_pattern("分析", encoding_rs::UTF_16LE).expect("a pattern");
        let bytes: Vec<u8> = pattern
            .iter()
            .map(|b| match b {
                HexPatternByte::Exact(v) => *v,
                HexPatternByte::Wildcard => panic!("no wildcards in a text pattern"),
            })
            .collect();

        assert_eq!(bytes, vec![0x06, 0x52, 0x90, 0x67]);
        assert_ne!(
            bytes,
            "分析".as_bytes().to_vec(),
            "the UTF-8 spelling must not be what UNICODE (LE) searches for"
        );
    }

    /// ASCII in UTF-16LE still gets its trailing zero bytes, which is what makes a
    /// wide string findable at all.
    #[test]
    fn ascii_in_utf16le_is_interleaved_with_zeros() {
        let pattern = text_to_exact_pattern("Hi", encoding_rs::UTF_16LE).expect("a pattern");
        let bytes: Vec<u8> = pattern
            .iter()
            .map(|b| match b {
                HexPatternByte::Exact(v) => *v,
                HexPatternByte::Wildcard => panic!("no wildcards"),
            })
            .collect();
        assert_eq!(bytes, vec![b'H', 0x00, b'i', 0x00]);
    }

    /// The other fields are unchanged: UTF-8 stays UTF-8, and enc1 follows the
    /// configured codepage.
    #[test]
    fn the_other_fields_keep_their_encodings() {
        let utf8 = text_to_exact_pattern("分析", encoding_rs::UTF_8).expect("a pattern");
        assert_eq!(utf8.len(), 6, "分析 is six bytes in UTF-8");

        let cp949 = text_to_exact_pattern("가", encoding_rs::EUC_KR).expect("a pattern");
        assert_eq!(cp949.len(), 2, "가 is two bytes in CP949");
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;

    fn dialog_with(focus: EditDialogFocus, value: &str) -> FindDialog {
        let mut dialog = FindDialog::default();
        dialog.focus = focus;
        match focus {
            EditDialogFocus::Enc1 => dialog.input_enc1 = Input::new(value.to_string()),
            EditDialogFocus::Utf8 => dialog.input_utf8 = Input::new(value.to_string()),
            EditDialogFocus::Utf16Le => dialog.input_utf16le = Input::new(value.to_string()),
            EditDialogFocus::Hex => dialog.input_hex = Input::new(value.to_string()),
        }
        dialog
    }

    /// Typing text shows the bytes that will be searched for.
    #[test]
    fn text_fills_the_hex_field() {
        let mut dialog = dialog_with(EditDialogFocus::Utf16Le, "分析");
        dialog.sync_from_focus(encoding_rs::UTF_8);
        assert_eq!(dialog.input_hex.value(), "06529067");

        let mut dialog = dialog_with(EditDialogFocus::Utf8, "Hi");
        dialog.sync_from_focus(encoding_rs::UTF_8);
        assert_eq!(dialog.input_hex.value(), "4869");
    }

    /// Typing hex shows what those bytes spell in each encoding.
    #[test]
    fn hex_fills_the_text_fields() {
        let mut dialog = dialog_with(EditDialogFocus::Hex, "06529067");
        dialog.sync_from_focus(encoding_rs::UTF_8);

        assert_eq!(dialog.input_utf16le.value(), "分析");
        assert_eq!(
            dialog.input_hex.value(),
            "06529067",
            "the focused field must not be rewritten under the cursor"
        );
    }

    /// An odd trailing digit is ignored rather than guessed at.
    #[test]
    fn a_half_typed_byte_is_ignored() {
        let mut dialog = dialog_with(EditDialogFocus::Hex, "486");
        dialog.sync_from_focus(encoding_rs::UTF_8);
        assert_eq!(dialog.input_utf8.value(), "H");
    }

    /// A wildcard pattern has no text spelling, so the text fields stay empty
    /// instead of showing characters for bytes that are not fixed.
    #[test]
    fn wildcards_clear_the_text_fields() {
        let mut dialog = dialog_with(EditDialogFocus::Hex, "48??4A");
        dialog.input_utf8 = Input::new("stale".to_string());
        dialog.sync_from_focus(encoding_rs::UTF_8);

        assert_eq!(dialog.input_utf8.value(), "");
        assert_eq!(dialog.input_hex.value(), "48??4A", "the pattern itself is kept");
    }
}
