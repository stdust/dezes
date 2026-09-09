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
    pub history: crate::input_history::HistoryQueue<crate::input_history::DialogHistoryEntry>,
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
            history: crate::input_history::HistoryQueue::default(),
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
        self.history.reset_nav();
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
            self.input_enc1 = Input::new(bytes_to_escaped_string(bytes, self.enc1));
        }
        if source != EditDialogFocus::Utf8 {
            self.input_utf8 = Input::new(bytes_to_escaped_string(bytes, encoding_rs::UTF_8));
        }
        if source != EditDialogFocus::Utf16Le {
            self.input_utf16le = Input::new(bytes_to_escaped_string(bytes, encoding_rs::UTF_16LE));
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
                    let bytes = unescape_to_bytes(&text, self.enc1);
                    self.update_others_from_bytes(&bytes, EditDialogFocus::Enc1);
                }
            }
            EditDialogFocus::Utf8 => {
                let text = self.input_utf8.value().to_string();
                if text.is_empty() {
                    self.clear_except(EditDialogFocus::Utf8);
                } else {
                    let bytes = unescape_to_bytes(&text, encoding_rs::UTF_8);
                    self.update_others_from_bytes(&bytes, EditDialogFocus::Utf8);
                }
            }
            EditDialogFocus::Utf16Le => {
                let text = self.input_utf16le.value().to_string();
                if text.is_empty() {
                    self.clear_except(EditDialogFocus::Utf16Le);
                } else {
                    let bytes = unescape_to_bytes(&text, encoding_rs::UTF_16LE);
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
                    if valid_len > 0
                        && let Ok(bytes) = hex::decode(&raw_hex[..valid_len])
                    {
                        self.update_others_from_bytes(&bytes, EditDialogFocus::Hex);
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

        self.input_enc1 = Input::new(bytes_to_escaped_string(bytes, self.enc1)).with_cursor(0);
        self.input_utf8 = Input::new(bytes_to_escaped_string(bytes, encoding_rs::UTF_8)).with_cursor(0);
        self.input_utf16le = Input::new(bytes_to_escaped_string(bytes, encoding_rs::UTF_16LE)).with_cursor(0);
        self.input_hex = Input::new(hex::encode_upper(bytes)).with_cursor(0);
    }

    pub fn get_bytes(&self) -> &[u8] {
        &self.cached_bytes
    }
}

/// Parses C-style escape sequences (`\n`, `\r`, `\t`, `\0`, `\\`, `\xHH`) into raw bytes
/// according to the specified `Encoding`. Unrecognized escapes (like `\w` or trailing `\`)
/// are preserved as literal characters to ensure safe typing in progress.
pub fn unescape_to_bytes(text: &str, enc: &'static encoding_rs::Encoding) -> Vec<u8> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut regular_text = String::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' {
            if let Some(&next_c) = chars.get(i + 1) {
                match next_c {
                    'n' => {
                        if !regular_text.is_empty() {
                            out.extend(crate::util::encode_text(&regular_text, enc));
                            regular_text.clear();
                        }
                        out.extend(crate::util::encode_char('\n', enc));
                        i += 2;
                        continue;
                    }
                    'r' => {
                        if !regular_text.is_empty() {
                            out.extend(crate::util::encode_text(&regular_text, enc));
                            regular_text.clear();
                        }
                        out.extend(crate::util::encode_char('\r', enc));
                        i += 2;
                        continue;
                    }
                    't' => {
                        if !regular_text.is_empty() {
                            out.extend(crate::util::encode_text(&regular_text, enc));
                            regular_text.clear();
                        }
                        out.extend(crate::util::encode_char('\t', enc));
                        i += 2;
                        continue;
                    }
                    '0' => {
                        if !regular_text.is_empty() {
                            out.extend(crate::util::encode_text(&regular_text, enc));
                            regular_text.clear();
                        }
                        out.extend(crate::util::encode_char('\0', enc));
                        i += 2;
                        continue;
                    }
                    '\\' => {
                        if !regular_text.is_empty() {
                            out.extend(crate::util::encode_text(&regular_text, enc));
                            regular_text.clear();
                        }
                        out.extend(crate::util::encode_char('\\', enc));
                        i += 2;
                        continue;
                    }
                    'x' | 'X' => {
                        if let (Some(&h1), Some(&h2)) = (chars.get(i + 2), chars.get(i + 3))
                            && h1.is_ascii_hexdigit()
                            && h2.is_ascii_hexdigit()
                        {
                            let mut hex_buf = [0u8; 2];
                            hex_buf[0] = h1 as u8;
                            hex_buf[1] = h2 as u8;
                            if let Ok(s) = std::str::from_utf8(&hex_buf)
                                && let Ok(val) = u8::from_str_radix(s, 16)
                            {
                                if !regular_text.is_empty() {
                                    out.extend(crate::util::encode_text(&regular_text, enc));
                                    regular_text.clear();
                                }
                                match enc.name() {
                                    "UTF-16LE" => out.extend_from_slice(&[val, 0x00]),
                                    "UTF-16BE" => out.extend_from_slice(&[0x00, val]),
                                    _ => out.push(val),
                                }
                                i += 4;
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Trailing backslash or unrecognized escape: treat '\' as literal
            regular_text.push('\\');
            i += 1;
        } else {
            regular_text.push(chars[i]);
            i += 1;
        }
    }

    if !regular_text.is_empty() {
        out.extend(crate::util::encode_text(&regular_text, enc));
    }

    out
}

fn push_escaped_str(out: &mut String, s: &str) {
    use std::fmt::Write;
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\x7F' => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn bytes_to_escaped_string_utf8(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len());
    let mut pos = 0;
    while pos < bytes.len() {
        match std::str::from_utf8(&bytes[pos..]) {
            Ok(valid_str) => {
                push_escaped_str(&mut out, valid_str);
                break;
            }
            Err(e) => {
                let valid_len = e.valid_up_to();
                if valid_len > 0 {
                    if let Ok(valid_str) = std::str::from_utf8(&bytes[pos..pos + valid_len]) {
                        push_escaped_str(&mut out, valid_str);
                    }
                    pos += valid_len;
                }
                let bad_len = e.error_len().unwrap_or(1);
                for &b in &bytes[pos..pos + bad_len] {
                    let _ = write!(out, "\\x{:02X}", b);
                }
                pos += bad_len;
            }
        }
    }
    out
}

fn bytes_to_escaped_string_utf16le(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() / 2);
    let chunks = bytes.chunks_exact(2);
    let remainder = chunks.remainder();
    let u16s: Vec<u16> = chunks.map(|c| u16::from_le_bytes([c[0], c[1]])).collect();

    for res in char::decode_utf16(u16s) {
        match res {
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\0') => out.push_str("\\0"),
            Ok('\\') => out.push_str("\\\\"),
            Ok(c) if (c as u32) < 0x20 || c == '\x7F' => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            Ok(c) => out.push(c),
            Err(e) => {
                let val = e.unpaired_surrogate();
                let b = val.to_le_bytes();
                let _ = write!(out, "\\x{:02X}\\x{:02X}", b[0], b[1]);
            }
        }
    }
    for &b in remainder {
        let _ = write!(out, "\\x{:02X}", b);
    }
    out
}

fn bytes_to_escaped_string_other(bytes: &[u8], enc: &'static encoding_rs::Encoding) -> String {
    let (s, _) = enc.decode_without_bom_handling(bytes);
    let mut out = String::with_capacity(bytes.len());
    push_escaped_str(&mut out, &s);
    out
}

/// Converts binary bytes to an escaped string suitable for single-line TUI display
/// and round-trip editing (`\n`, `\r`, `\t`, `\0`, `\\`, `\xHH`).
pub fn bytes_to_escaped_string(bytes: &[u8], enc: &'static encoding_rs::Encoding) -> String {
    if enc.name() == "UTF-16LE" {
        bytes_to_escaped_string_utf16le(bytes)
    } else if enc == encoding_rs::UTF_8 {
        bytes_to_escaped_string_utf8(bytes)
    } else {
        bytes_to_escaped_string_other(bytes, enc)
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
        "  {}  |  Escapes: \\n, \\r, \\t, \\0, \\\\, \\xHH",
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
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.hex_view.edit_dialog.selection_anchor = None;
                let focus = app.hex_view.edit_dialog.focus;
                let val = app.hex_view.edit_dialog.active_input().value().to_string();
                let cur_entry = crate::input_history::DialogHistoryEntry { focus, value: val };
                if let Some(entry) = app.hex_view.edit_dialog.history.navigate_up(&cur_entry) {
                    app.hex_view.edit_dialog.focus = entry.focus;
                    let cur_len = entry.value.chars().count();
                    *app.hex_view.edit_dialog.active_input_mut() = Input::new(entry.value).with_cursor(cur_len);
                    app.hex_view.edit_dialog.sync_from_focus();
                }
                return Ok(false);
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.hex_view.edit_dialog.selection_anchor = None;
                let focus = app.hex_view.edit_dialog.focus;
                let val = app.hex_view.edit_dialog.active_input().value().to_string();
                let cur_entry = crate::input_history::DialogHistoryEntry { focus, value: val };
                if let Some(entry) = app.hex_view.edit_dialog.history.navigate_down(&cur_entry) {
                    app.hex_view.edit_dialog.focus = entry.focus;
                    let cur_len = entry.value.chars().count();
                    *app.hex_view.edit_dialog.active_input_mut() = Input::new(entry.value).with_cursor(cur_len);
                    app.hex_view.edit_dialog.sync_from_focus();
                }
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
                let focus = app.hex_view.edit_dialog.focus;
                let val = app.hex_view.edit_dialog.active_input().value().to_string();
                if !val.trim().is_empty() {
                    app.hex_view.edit_dialog.history.push(crate::input_history::DialogHistoryEntry {
                        focus,
                        value: val,
                    });
                }
                // Owned for the same reason as in `header/edit_dialog.rs`: the
                // bytes come out of `app` and staging them needs `app` mutably.
                let bytes = app.hex_view.edit_dialog.get_bytes().to_vec();
                if !bytes.is_empty() {
                    let mut ofs = app.hex_view.offset;
                    for &b in bytes.iter() {
                        if ofs < app.file_info.buffer_len() {
                            crate::hex::edit::record_edit(app, ofs, b);
                            ofs += 1;
                        } else {
                            break;
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
                    if let Ok(mut cb) = arboard::Clipboard::new()
                        && let Ok(pasted_text) = cb.get_text()
                    {
                        if app.hex_view.edit_dialog.selection_anchor.is_some() {
                            app.hex_view.edit_dialog.delete_selection();
                        }
                        let focus = app.hex_view.edit_dialog.focus;
                        if focus == EditDialogFocus::Hex {
                            let clean: String = pasted_text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                            for c in clean.chars() {
                                app.hex_view.edit_dialog.active_input_mut().handle(tui_input::InputRequest::InsertChar(c));
                            }
                        } else {
                            let clean = pasted_text
                                .replace("\r\n", "\\r\\n")
                                .replace('\r', "\\r")
                                .replace('\n', "\\n")
                                .replace('\t', "\\t");
                            for c in clean.chars() {
                                app.hex_view.edit_dialog.active_input_mut().handle(tui_input::InputRequest::InsertChar(c));
                            }
                        }
                        app.hex_view.edit_dialog.sync_from_focus();
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

    #[test]
    fn test_unescape_system_note_newlines() {
        let input = r"\n\n[SYSTEM NOTE";
        let bytes = unescape_to_bytes(input, encoding_rs::UTF_8);
        assert_eq!(
            bytes,
            vec![0x0A, 0x0A, 0x5B, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x20, 0x4E, 0x4F, 0x54, 0x45]
        );
    }

    #[test]
    fn test_unescape_c_escapes() {
        let input = r"A\nB\rC\tD\0E\\F";
        let bytes = unescape_to_bytes(input, encoding_rs::UTF_8);
        assert_eq!(bytes, b"A\nB\rC\tD\0E\\F");
    }

    #[test]
    fn test_unescape_hex_sequences() {
        let input = r"\x41\x42\x00\xFF\x90";
        let bytes = unescape_to_bytes(input, encoding_rs::UTF_8);
        assert_eq!(bytes, vec![0x41, 0x42, 0x00, 0xFF, 0x90]);

        // Incomplete / non-hex treated as literal
        let literal = r"\x\xG\w";
        let bytes_lit = unescape_to_bytes(literal, encoding_rs::UTF_8);
        assert_eq!(bytes_lit, b"\\x\\xG\\w");
    }

    #[test]
    fn test_unescape_utf16le() {
        let input = r"\n\n[A";
        let bytes = unescape_to_bytes(input, encoding_rs::UTF_16LE);
        assert_eq!(
            bytes,
            vec![0x0A, 0x00, 0x0A, 0x00, 0x5B, 0x00, 0x41, 0x00]
        );

        let input_hex = r"\x41";
        let bytes_hex = unescape_to_bytes(input_hex, encoding_rs::UTF_16LE);
        assert_eq!(bytes_hex, vec![0x41, 0x00]);
    }

    #[test]
    fn test_bytes_to_escaped_string_and_roundtrip() {
        let raw = vec![0x0A, 0x0A, 0x5B, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x20, 0x4E, 0x4F, 0x54, 0x45];
        let escaped = bytes_to_escaped_string(&raw, encoding_rs::UTF_8);
        assert_eq!(escaped, r"\n\n[SYSTEM NOTE");

        let back = unescape_to_bytes(&escaped, encoding_rs::UTF_8);
        assert_eq!(back, raw);
    }

    #[test]
    fn test_bytes_to_escaped_string_control_chars() {
        let raw = vec![0x00, 0x09, 0x0A, 0x0D, 0x1B, 0x5C];
        let escaped = bytes_to_escaped_string(&raw, encoding_rs::UTF_8);
        assert_eq!(escaped, r"\0\t\n\r\x1B\\");

        let back = unescape_to_bytes(&escaped, encoding_rs::UTF_8);
        assert_eq!(back, raw);
    }

    #[test]
    fn test_edit_dialog_sync_utf8_to_hex() {
        let mut dialog = EditDialog::default();
        dialog.focus = EditDialogFocus::Utf8;
        dialog.input_utf8 = Input::new(r"\n\n[SYSTEM NOTE".to_string());
        dialog.sync_from_focus();

        assert_eq!(dialog.input_hex.value(), "0A0A5B53595354454D204E4F5445");
        assert_eq!(
            dialog.get_bytes(),
            &[0x0A, 0x0A, 0x5B, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x20, 0x4E, 0x4F, 0x54, 0x45]
        );
        assert_eq!(dialog.input_enc1.value(), r"\n\n[SYSTEM NOTE");
    }

    #[test]
    fn test_edit_dialog_sync_hex_to_utf8() {
        let mut dialog = EditDialog::default();
        dialog.focus = EditDialogFocus::Hex;
        dialog.input_hex = Input::new("0A0A5B53595354454D204E4F5445".to_string());
        dialog.sync_from_focus();

        assert_eq!(dialog.input_utf8.value(), r"\n\n[SYSTEM NOTE");
        assert_eq!(
            dialog.get_bytes(),
            &[0x0A, 0x0A, 0x5B, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x20, 0x4E, 0x4F, 0x54, 0x45]
        );
    }

    #[test]
    fn test_edit_dialog_history_navigation_and_sync() {
        let mut dialog = EditDialog::default();
        dialog.focus = EditDialogFocus::Utf8;
        dialog.input_utf8 = Input::new("DraftInput".to_string());
        dialog.sync_from_focus();

        // Push two previous edit entries
        dialog.history.push(crate::input_history::DialogHistoryEntry {
            focus: EditDialogFocus::Hex,
            value: "909090".to_string(),
        });
        dialog.history.push(crate::input_history::DialogHistoryEntry {
            focus: EditDialogFocus::Utf8,
            value: "NewString".to_string(),
        });

        // 1st Up: returns most recent entry ("NewString" in Utf8)
        let cur_entry = crate::input_history::DialogHistoryEntry {
            focus: dialog.focus,
            value: dialog.active_input().value().to_string(),
        };
        let res = dialog.history.navigate_up(&cur_entry).unwrap();
        assert_eq!(res.focus, EditDialogFocus::Utf8);
        assert_eq!(res.value, "NewString");
        dialog.focus = res.focus;
        *dialog.active_input_mut() = Input::new(res.value);
        dialog.sync_from_focus();
        assert_eq!(dialog.input_hex.value(), "4E6577537472696E67");

        // 2nd Up: returns older entry ("909090" in Hex)
        let cur_entry2 = crate::input_history::DialogHistoryEntry {
            focus: dialog.focus,
            value: dialog.active_input().value().to_string(),
        };
        let res2 = dialog.history.navigate_up(&cur_entry2).unwrap();
        assert_eq!(res2.focus, EditDialogFocus::Hex);
        assert_eq!(res2.value, "909090");
        dialog.focus = res2.focus;
        *dialog.active_input_mut() = Input::new(res2.value);
        dialog.sync_from_focus();
        assert_eq!(dialog.get_bytes(), &[0x90, 0x90, 0x90]);

        // Down: returns "NewString"
        let cur_entry3 = crate::input_history::DialogHistoryEntry {
            focus: dialog.focus,
            value: dialog.active_input().value().to_string(),
        };
        let res3 = dialog.history.navigate_down(&cur_entry3).unwrap();
        assert_eq!(res3.value, "NewString");

        // Down again: restores draft ("DraftInput")
        let cur_entry4 = crate::input_history::DialogHistoryEntry {
            focus: dialog.focus,
            value: dialog.active_input().value().to_string(),
        };
        let res4 = dialog.history.navigate_down(&cur_entry4).unwrap();
        assert_eq!(res4.value, "DraftInput");
    }

    #[test]
    fn test_edit_dialog_events_ctrl_and_plain_arrows() {
        use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

        let mut app = App::new();
        app.hex_view.edit_dialog.reset();
        app.hex_view.edit_dialog.focus = EditDialogFocus::Enc1;

        // Push an entry to history
        app.hex_view.edit_dialog.history.push(crate::input_history::DialogHistoryEntry {
            focus: EditDialogFocus::Hex,
            value: "9090".to_string(),
        });

        let plain_down = Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let plain_up = Event::Key(KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let ctrl_up = Event::Key(KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let ctrl_down = Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });

        // Plain Down moves focus from Enc1 to Utf8
        let _ = dialog_edit_events(&mut app, &plain_down);
        assert_eq!(app.hex_view.edit_dialog.focus, EditDialogFocus::Utf8);

        // Plain Up moves focus back to Enc1
        let _ = dialog_edit_events(&mut app, &plain_up);
        assert_eq!(app.hex_view.edit_dialog.focus, EditDialogFocus::Enc1);

        // Ctrl+Up navigates history: loads "9090" in Hex
        let _ = dialog_edit_events(&mut app, &ctrl_up);
        assert_eq!(app.hex_view.edit_dialog.focus, EditDialogFocus::Hex);
        assert_eq!(app.hex_view.edit_dialog.input_hex.value(), "9090");

        // Ctrl+Down restores draft (which was empty)
        let _ = dialog_edit_events(&mut app, &ctrl_down);
        assert_eq!(app.hex_view.edit_dialog.focus, EditDialogFocus::Enc1);
        assert_eq!(app.hex_view.edit_dialog.input_enc1.value(), "");
    }
}
