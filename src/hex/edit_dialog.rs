use ratatui::{Frame, crossterm::event::{Event, KeyCode, KeyModifiers}};
use std::io::Result;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::hex::field_box::{self, FieldRow};
use crate::{app::App, editor::UIState};

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum EditDialogFocus {
    /// The user's configured primary encoding (`e` key / `:set` ), *not* a fixed
    /// codepage - so a CP936 user edits CP936 here and a CP949 user edits CP949.
    #[default]
    Enc1,
    Utf8,
    Utf16Le,
    Hex,
}

impl EditDialogFocus {
    pub fn next(&self) -> Self {
        match self {
            Self::Enc1 => Self::Utf8,
            Self::Utf8 => Self::Utf16Le,
            Self::Utf16Le => Self::Hex,
            Self::Hex => Self::Enc1,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Enc1 => Self::Hex,
            Self::Utf8 => Self::Enc1,
            Self::Utf16Le => Self::Utf8,
            Self::Hex => Self::Utf16Le,
        }
    }
}

/// Encoding used for the first field when nothing else is known.
///
/// Only reached if the dialog is somehow used before [`EditDialog::set_enc1`]
/// runs; in practice the value always comes from `app.text_view.table`.
fn default_enc1() -> &'static encoding_rs::Encoding {
    encoding_rs::UTF_8
}

#[derive(Debug)]
pub struct EditDialog {
    pub input_enc1: Input,
    pub input_utf8: Input,
    pub input_utf16le: Input,
    pub input_hex: Input,
    pub focus: EditDialogFocus,
    /// Encoding backing the first field, mirrored from the app's primary
    /// encoding each time the dialog opens. Stored here because
    /// `sync_from_focus` runs with only the dialog borrowed, not all of `App`.
    enc1: &'static encoding_rs::Encoding,
    cached_bytes: Vec<u8>,
    pub selection_anchor: Option<usize>,
}

impl Default for EditDialog {
    fn default() -> Self {
        Self {
            input_enc1: Input::default(),
            input_utf8: Input::default(),
            input_utf16le: Input::default(),
            input_hex: Input::default(),
            focus: EditDialogFocus::default(),
            enc1: default_enc1(),
            cached_bytes: Vec::new(),
            selection_anchor: None,
        }
    }
}

impl EditDialog {
    /// Points the first field at `enc`, called when the dialog opens so it
    /// tracks the primary encoding the rest of the UI displays.
    pub fn set_enc1(&mut self, enc: &'static encoding_rs::Encoding) {
        self.enc1 = enc;
    }

    pub fn enc1(&self) -> &'static encoding_rs::Encoding {
        self.enc1
    }

    pub fn reset(&mut self) {
        self.input_enc1 = Input::default();
        self.input_utf8 = Input::default();
        self.input_utf16le = Input::default();
        self.input_hex = Input::default();
        self.focus = EditDialogFocus::Enc1;
        self.cached_bytes.clear();
        self.selection_anchor = None;
    }

    pub fn active_input(&self) -> &Input {
        match self.focus {
            EditDialogFocus::Enc1 => &self.input_enc1,
            EditDialogFocus::Utf8 => &self.input_utf8,
            EditDialogFocus::Utf16Le => &self.input_utf16le,
            EditDialogFocus::Hex => &self.input_hex,
        }
    }

    pub fn active_input_mut(&mut self) -> &mut Input {
        match self.focus {
            EditDialogFocus::Enc1 => &mut self.input_enc1,
            EditDialogFocus::Utf8 => &mut self.input_utf8,
            EditDialogFocus::Utf16Le => &mut self.input_utf16le,
            EditDialogFocus::Hex => &mut self.input_hex,
        }
    }

    pub fn get_selection_range(&self) -> Option<(usize, usize)> {
        if let Some(anchor) = self.selection_anchor {
            let cursor = self.active_input().cursor();
            if anchor != cursor {
                return Some((std::cmp::min(anchor, cursor), std::cmp::max(anchor, cursor)));
            }
        }
        None
    }

    pub fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.get_selection_range() {
            let val = self.active_input().value().to_string();
            let chars: Vec<char> = val.chars().collect();
            let safe_start = std::cmp::min(start, chars.len());
            let safe_end = std::cmp::min(end, chars.len());

            let mut new_chars = Vec::new();
            new_chars.extend_from_slice(&chars[..safe_start]);
            new_chars.extend_from_slice(&chars[safe_end..]);
            let new_val: String = new_chars.into_iter().collect();

            *self.active_input_mut() = Input::new(new_val).with_cursor(safe_start);
            self.selection_anchor = None;
            true
        } else {
            false
        }
    }

    fn update_others_from_bytes(&mut self, bytes: &[u8], source: EditDialogFocus) {
        self.cached_bytes = bytes.to_vec();

        if source != EditDialogFocus::Enc1 {
            let (s, _) = self.enc1.decode_without_bom_handling(bytes);
            self.input_enc1 = Input::new(s.into_owned());
        }
        if source != EditDialogFocus::Utf8 {
            let s = String::from_utf8_lossy(bytes);
            let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
            self.input_utf8 = Input::new(s.to_string());
        }
        if source != EditDialogFocus::Utf16Le {
            let (s, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(bytes);
            self.input_utf16le = Input::new(s.into_owned());
        }
        if source != EditDialogFocus::Hex {
            self.input_hex = Input::new(hex::encode_upper(bytes));
        }
    }

    pub fn sync_from_focus(&mut self) {
        match self.focus {
            EditDialogFocus::Enc1 => {
                let text = self.input_enc1.value().to_string();
                if text.is_empty() {
                    self.clear_except(EditDialogFocus::Enc1);
                } else {
                    // Through `encode_text`, since enc1 can itself be UTF-16LE or
                    // UTF-16BE, which `Encoding::encode` would turn into UTF-8.
                    let bytes = crate::util::encode_text(&text, self.enc1);
                    self.update_others_from_bytes(&bytes, EditDialogFocus::Enc1);
                }
            }
            EditDialogFocus::Utf8 => {
                let text = self.input_utf8.value().to_string();
                if text.is_empty() {
                    self.clear_except(EditDialogFocus::Utf8);
                } else {
                    let (bytes, _, _) = encoding_rs::UTF_8.encode(&text);
                    self.update_others_from_bytes(&bytes, EditDialogFocus::Utf8);
                }
            }
            EditDialogFocus::Utf16Le => {
                let text = self.input_utf16le.value().to_string();
                if text.is_empty() {
                    self.clear_except(EditDialogFocus::Utf16Le);
                } else {
                    // encoding_rs cannot encode *to* UTF-16 - it maps the label to
                    // UTF-8 - so this field used to fill the Hex row with the UTF-8
                    // bytes of the text and write those into the file. `分析` gave
                    // `E5 88 86 E6 9E 90` instead of `06 52 90 67`.
                    let bytes = crate::util::encode_text(&text, encoding_rs::UTF_16LE);
                    self.update_others_from_bytes(&bytes, EditDialogFocus::Utf16Le);
                }
            }
            EditDialogFocus::Hex => {
                let raw_hex: String = self.input_hex.value()
                    .chars()
                    .filter(|c| c.is_ascii_hexdigit())
                    .collect();

                if raw_hex.is_empty() {
                    self.clear_except(EditDialogFocus::Hex);
                } else {
                    let valid_len = raw_hex.len() - (raw_hex.len() % 2);
                    if valid_len > 0 {
                        if let Ok(bytes) = hex::decode(&raw_hex[..valid_len]) {
                            self.update_others_from_bytes(&bytes, EditDialogFocus::Hex);
                        }
                    }
                }
            }
        }
    }

    fn clear_except(&mut self, source: EditDialogFocus) {
        self.cached_bytes.clear();
        if source != EditDialogFocus::Enc1 {
            self.input_enc1 = Input::default();
        }
        if source != EditDialogFocus::Utf8 {
            self.input_utf8 = Input::default();
        }
        if source != EditDialogFocus::Utf16Le {
            self.input_utf16le = Input::default();
        }
        if source != EditDialogFocus::Hex {
            self.input_hex = Input::default();
        }
    }

    pub fn load_bytes(&mut self, bytes: &[u8]) {
        self.cached_bytes = bytes.to_vec();

        let (s, _) = self.enc1.decode_without_bom_handling(bytes);
        self.input_enc1 = Input::new(s.into_owned()).with_cursor(0);
        let s = String::from_utf8_lossy(bytes);
        let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
        self.input_utf8 = Input::new(s.to_string()).with_cursor(0);
        let (s, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(bytes);
        self.input_utf16le = Input::new(s.into_owned()).with_cursor(0);
        self.input_hex = Input::new(hex::encode_upper(bytes)).with_cursor(0);
    }

    pub fn get_bytes(&self) -> &[u8] {
        &self.cached_bytes
    }
}

pub fn open_edit_dialog(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoEditData);
        return;
    }
    app.state = UIState::DialogEditData;
    app.hex_view.edit_dialog.reset();
    let enc1 = app.text_view.table;
    app.hex_view.edit_dialog.set_enc1(enc1);
    app.hex_view.edit_dialog.focus = EditDialogFocus::Enc1;
    app.dialog_renderer = Some(dialog_edit_draw);

    let range = app
        .hex_view
        .blocks
        .iter()
        .find(|block| app.hex_view.offset >= block.start && app.hex_view.offset <= block.end)
        .map(|block| (block.start, block.end))
        .or_else(|| {
            let sel = app.hex_view.selection;
            if app.state == UIState::HexSelection || sel.start != sel.end {
                Some((sel.start.min(sel.end), sel.start.max(sel.end)))
            } else {
                None
            }
        });

    if let Some((start, end)) = range {
        let buffer = app.file_info.get_buffer_ref();
        if start < buffer.len() {
            let end = end.min(buffer.len() - 1);
            let mut bytes = buffer[start..=end].to_vec();
            if !app.hex_view.changed_bytes.is_empty() {
                for (i, b) in bytes.iter_mut().enumerate() {
                    if let Some(edited) = app.hex_view.changed_bytes.get(&(start + i)) {
                        *b = *edited;
                    }
                }
            }
            app.hex_view.edit_dialog.load_bytes(&bytes);
            app.goto(start);
        }
    }
}

/// One row per field: `"  <label>  : [ ... ]"`, all sharing a single outer
/// border instead of each field being its own nested 3-row box.
/// The first row's label is built at draw time from the active encoding, so
/// only the three fixed rows live here.
const FIELD_ROWS: [(&str, EditDialogFocus); 3] = [
    ("UTF-8", EditDialogFocus::Utf8),
    ("UNICODE (LE)", EditDialogFocus::Utf16Le),
    ("Hex", EditDialogFocus::Hex),
];

/// Row label for the primary-encoding field, e.g. `"ANSI (GBK)"`.
pub fn enc1_label(enc: &'static encoding_rs::Encoding) -> String {
    format!("ANSI ({})", enc.name())
}

pub fn dialog_edit_draw(app: &mut App, frame: &mut Frame) {
    let ofs = app.hex_view.offset;
    let at = crate::i18n::M::EditDataTitle.tr(app.config.lang);
    let title = if app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va {
        let va = app.get_va(ofs);
        let is_64 = app.is_64();
        if is_64 {
            format!(" {} 0x{:X} ", at, va)
        } else {
            format!(" {} 0x{:08X} ", at, va)
        }
    } else {
        format!(" {} 0x{:08X} ", at, ofs)
    };

    // First row's label depends on the active encoding, so the row list is
    // assembled here rather than being a fully static table.
    let enc1_row_label = enc1_label(app.hex_view.edit_dialog.enc1());
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
    let inner_area = field_box::draw_box(app, frame, title, label_width, rows.len(), true);

    let focus = app.hex_view.edit_dialog.focus;
    let inputs = [
        &app.hex_view.edit_dialog.input_enc1,
        &app.hex_view.edit_dialog.input_utf8,
        &app.hex_view.edit_dialog.input_utf16le,
        &app.hex_view.edit_dialog.input_hex,
    ];

    let mut cursor_pos: Option<(u16, u16)> = None;

    for (row, (label, field_focus)) in rows.iter().enumerate() {
        let is_focused = *field_focus == focus;
        let selection = if is_focused { app.hex_view.edit_dialog.get_selection_range() } else { None };

        let field = FieldRow {
            label,
            input: inputs[row],
            focused: is_focused,
            selection,
        };

        if let Some(pos) = field_box::draw_field_row(app, frame, inner_area, row as u16, label_width, &field) {
            cursor_pos = Some(pos);
        }
    }

    let byte_cnt = app.hex_view.edit_dialog.get_bytes().len();
    let status_text = format!(
        "  {}",
        crate::i18n::fill(
            crate::i18n::M::BytesSelected.tr(app.config.lang),
            &[&byte_cnt.to_string()]
        )
    );
    field_box::draw_status_row(app, frame, inner_area, rows.len() as u16, &status_text);

    if let Some((x, y)) = cursor_pos {
        frame.set_cursor_position((x, y));
    }
}

pub fn dialog_edit_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.hex_view.edit_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            KeyCode::Tab => {
                app.hex_view.edit_dialog.selection_anchor = None;
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    app.hex_view.edit_dialog.focus = app.hex_view.edit_dialog.focus.prev();
                } else {
                    app.hex_view.edit_dialog.focus = app.hex_view.edit_dialog.focus.next();
                }
                return Ok(false);
            }
            KeyCode::BackTab => {
                app.hex_view.edit_dialog.selection_anchor = None;
                app.hex_view.edit_dialog.focus = app.hex_view.edit_dialog.focus.prev();
                return Ok(false);
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::NONE) => {
                app.hex_view.edit_dialog.selection_anchor = None;
                app.hex_view.edit_dialog.focus = app.hex_view.edit_dialog.focus.prev();
                return Ok(false);
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::NONE) => {
                app.hex_view.edit_dialog.selection_anchor = None;
                app.hex_view.edit_dialog.focus = app.hex_view.edit_dialog.focus.next();
                return Ok(false);
            }
            KeyCode::Enter => {
                // Owned for the same reason as in `header/edit_dialog.rs`: the
                // bytes come out of `app` and staging them needs `app` mutably.
                let bytes = app.hex_view.edit_dialog.get_bytes().to_vec();
                if !bytes.is_empty() {
                    let mut ofs = app.hex_view.offset;
                    for &b in bytes.iter() {
                        if ofs < app.file_info.size {
                            crate::hex::edit::record_edit(app, ofs, b);
                            ofs += 1;
                        }
                    }
                    app.goto(ofs);
                }

                app.hex_view.edit_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            _ => {}
        }

        // Shift selection navigation
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Left => {
                    let cur = app.hex_view.edit_dialog.active_input().cursor();
                    if app.hex_view.edit_dialog.selection_anchor.is_none() {
                        app.hex_view.edit_dialog.selection_anchor = Some(cur);
                    }
                    if cur > 0 {
                        let new_pos = cur - 1;
                        let input = app.hex_view.edit_dialog.active_input().clone();
                        *app.hex_view.edit_dialog.active_input_mut() = input.with_cursor(new_pos);
                    }
                    return Ok(false);
                }
                KeyCode::Right => {
                    let cur = app.hex_view.edit_dialog.active_input().cursor();
                    let len = app.hex_view.edit_dialog.active_input().value().chars().count();
                    if app.hex_view.edit_dialog.selection_anchor.is_none() {
                        app.hex_view.edit_dialog.selection_anchor = Some(cur);
                    }
                    if cur < len {
                        let new_pos = cur + 1;
                        let input = app.hex_view.edit_dialog.active_input().clone();
                        *app.hex_view.edit_dialog.active_input_mut() = input.with_cursor(new_pos);
                    }
                    return Ok(false);
                }
                KeyCode::Home => {
                    let cur = app.hex_view.edit_dialog.active_input().cursor();
                    if app.hex_view.edit_dialog.selection_anchor.is_none() {
                        app.hex_view.edit_dialog.selection_anchor = Some(cur);
                    }
                    let input = app.hex_view.edit_dialog.active_input().clone();
                    *app.hex_view.edit_dialog.active_input_mut() = input.with_cursor(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    let cur = app.hex_view.edit_dialog.active_input().cursor();
                    let len = app.hex_view.edit_dialog.active_input().value().chars().count();
                    if app.hex_view.edit_dialog.selection_anchor.is_none() {
                        app.hex_view.edit_dialog.selection_anchor = Some(cur);
                    }
                    let input = app.hex_view.edit_dialog.active_input().clone();
                    *app.hex_view.edit_dialog.active_input_mut() = input.with_cursor(len);
                    return Ok(false);
                }
                _ => {}
            }
        }

        // Ctrl shortcuts: Ctrl+A (Select All), Ctrl+C (Copy)
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    let len = app.hex_view.edit_dialog.active_input().value().chars().count();
                    app.hex_view.edit_dialog.selection_anchor = Some(0);
                    let input = app.hex_view.edit_dialog.active_input().clone();
                    *app.hex_view.edit_dialog.active_input_mut() = input.with_cursor(len);
                    return Ok(false);
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    let text_to_copy = if let Some((start, end)) = app.hex_view.edit_dialog.get_selection_range() {
                        let chars: Vec<char> = app.hex_view.edit_dialog.active_input().value().chars().collect();
                        let safe_start = std::cmp::min(start, chars.len());
                        let safe_end = std::cmp::min(end, chars.len());
                        chars[safe_start..safe_end].iter().collect::<String>()
                    } else {
                        app.hex_view.edit_dialog.active_input().value().to_string()
                    };
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        cb.set_text(text_to_copy).ok();
                    }
                    return Ok(false);
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        if let Ok(pasted_text) = cb.get_text() {
                            let clean = pasted_text.replace("\r\n", "").replace('\n', "");
                            for c in clean.chars() {
                                app.hex_view.edit_dialog.active_input_mut().handle(tui_input::InputRequest::InsertChar(c));
                            }
                        }
                    }
                    return Ok(false);
                }
                _ => {}
            }
        }

        // Handle navigation keys without Shift: clear selection anchor
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {
                app.hex_view.edit_dialog.selection_anchor = None;
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if app.hex_view.edit_dialog.delete_selection() {
                    app.hex_view.edit_dialog.sync_from_focus();
                    return Ok(false);
                }
            }
            _ => {
                // If typing a character while text is selected, delete selection first
                if app.hex_view.edit_dialog.selection_anchor.is_some() {
                    app.hex_view.edit_dialog.delete_selection();
                }
            }
        }

        // Pass event to current focused field and sync other fields
        let focus = app.hex_view.edit_dialog.focus;
        match focus {
            EditDialogFocus::Enc1 => {
                app.hex_view.edit_dialog.input_enc1.handle_event(event);
            }
            EditDialogFocus::Utf8 => {
                app.hex_view.edit_dialog.input_utf8.handle_event(event);
            }
            EditDialogFocus::Utf16Le => {
                app.hex_view.edit_dialog.input_utf16le.handle_event(event);
            }
            EditDialogFocus::Hex => {
                app.hex_view.edit_dialog.input_hex.handle_event(event);
            }
        }
        app.hex_view.edit_dialog.sync_from_focus();
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_cursor_manipulation() {
        let input = Input::new("hello".to_string()).with_cursor(2);
        assert_eq!(input.cursor(), 2);
        assert_eq!(input.value(), "hello");
    }
}
