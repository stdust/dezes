use crossterm::event::KeyModifiers;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::fmt::Write as _;
use std::io::Result;

use crate::app::App;
use crate::editor::UIState;
use crate::hex::blocks::ColoredBlock;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Direction {
    LeftOrUp,
    RightOrDown,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
    pub direction: Option<Direction>,
    pub is_mouse: bool,
}

impl IntoIterator for Selection {
    type Item = usize;
    type IntoIter = std::ops::RangeInclusive<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.start..=self.end
    }
}

impl Selection {
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset <= self.end
    }
    pub fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
        self.direction = None;
        self.is_mouse = false;
    }
    pub fn select_left_or_up(&mut self, step: usize) {
        self.is_mouse = false;
        match self.direction {
            None => {
                self.direction = Some(Direction::LeftOrUp);
                self.start = self.start.saturating_sub(step);
            }
            Some(Direction::LeftOrUp) => self.start = self.start.saturating_sub(step),
            Some(Direction::RightOrDown) => self.end = self.end.saturating_sub(step),
        }

        if self.start == self.end {
            self.direction = None;
        }
    }
    /// Extends the selection forward, clamped to `last_offset`.
    ///
    /// `last_offset` is the last *valid* offset, i.e. `size - 1`, because the
    /// range is inclusive on both ends (see `contains` and `IntoIterator`).
    /// Callers used to pass `file_info.size`, which let `end` reach one past EOF;
    /// `Insert`/`Delete` then recorded a `changed_bytes` entry at that offset and
    /// `:w` seeked there and grew the file by a byte.
    pub fn select_right_or_down(&mut self, last_offset: usize, step: usize) {
        self.is_mouse = false;
        match self.direction {
            None => {
                self.direction = Some(Direction::RightOrDown);
                self.end = (self.start + step).min(last_offset);
            }
            Some(Direction::LeftOrUp) => self.start = (self.start + step).min(last_offset),
            Some(Direction::RightOrDown) => self.end = (self.end + step).min(last_offset),
        }
        if self.start == self.end {
            self.direction = None;
        }
    }
}

/// `"41 42 43 ..."` dump of the active selection, wrapped every bytes-per-line.
///
/// Shared by the `y` handler in both normal and selection mode; both used to
/// build this with a `format!` allocation per byte.
pub fn format_selection_hex(app: &mut App) -> String {
    let bpl = app.config.hex_mode_bytes_per_line.max(1);
    let start = app.hex_view.selection.start.min(app.hex_view.selection.end);
    let end = app.hex_view.selection.start.max(app.hex_view.selection.end);

    let mut s = String::with_capacity((end - start + 1) * 3 + 8);
    let mut count = 0usize;
    for offset in start..=end {
        if let Some(byte) = app.read_u8(offset) {
            let _ = write!(s, "{:02X} ", byte);
            count += 1;
            if count % bpl == 0 {
                s.push_str("\r\n");
            }
        }
    }
    s.truncate(s.trim_end().len());
    s
}

/// The selection as the encoding column draws it: decoded text, with every
/// non-printable character replaced by the view's placeholder.
///
/// The placeholder is the point. Decoding raw put the actual control bytes on the
/// clipboard, so what was copied did not match what was on screen, and a `0x00`
/// anywhere in the block truncated the whole string - Windows clipboard text ends at
/// the first NUL. Substituting makes the copy exactly the row of characters the user
/// selected.
///
/// This deliberately does not round-trip: pasting it back writes literal dots. Byte-
/// accurate copying is what the hex column is for.
///
/// Pending edits are included, since `read_u8` reports them - copying out of a column
/// that shows edited bytes has to hand over what is on screen.
fn format_selection_text(app: &mut App, encoding: &'static encoding_rs::Encoding) -> String {
    let start = app.hex_view.selection.start.min(app.hex_view.selection.end);
    let end = app.hex_view.selection.start.max(app.hex_view.selection.end);

    let mut bytes = Vec::with_capacity(end.saturating_sub(start) + 1);
    for offset in start..=end {
        match app.read_u8(offset) {
            Some(b) => bytes.push(b),
            None => break,
        }
    }

    // Lossy decode: a block of a binary rarely reads cleanly in any encoding, and the
    // replacement character is itself non-printable by the rule below, so it comes out
    // as the placeholder too.
    let decoded = encoding.decode(&bytes).0;
    let placeholder = app.config.hex_mode_non_graphic_char;

    decoded
        .chars()
        .map(|c| if is_printable(c) { c } else { placeholder })
        .collect()
}

/// Whether the encoding columns draw `c` as itself rather than as the placeholder.
///
/// Mirrors the rule in `hex/draw.rs`: printable ASCII stays, other ASCII (controls,
/// tab, newline) does not, and a non-ASCII character stays as long as it is neither a
/// control nor whitespace - that is what lets Hangul and CJK through.
fn is_printable(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_graphic()
    } else {
        !c.is_control() && !c.is_whitespace() && c != '\u{FFFD}'
    }
}

/// What yanking the selection should produce, given the column it was made in.
///
/// A block selected in the byte column is bytes; the same block selected in an
/// encoding column is the text those bytes spell in that encoding. Both are
/// wanted - one for patching, one for reading - and which one is meant is exactly
/// what the column the user selected in says.
pub fn format_selection_for_target(app: &mut App) -> String {
    use crate::editor::EditingTarget;

    match app.hex_view.selection_target {
        EditingTarget::Hex => format_selection_hex(app),
        EditingTarget::Enc1 => format_selection_text(app, app.text_view.table),
        EditingTarget::Enc2 => {
            let enc2 = app.hex_view.get_enc2_table();
            format_selection_text(app, enc2)
        }
    }
}

#[allow(dead_code)]
pub fn format_mouse_selection_dump(app: &mut App, start: usize, end: usize) -> String {
    let bpl = app.config.hex_mode_bytes_per_line.max(1);
    let rows = (end.saturating_sub(start) / bpl) + 2;
    // Pre-sized and written in place: this runs on every mouse-button release.
    let mut result = String::with_capacity(rows * (bpl * 4 + 16));

    // 1. Header row (Address offset space + 00 01 02 ... 0F)
    let addr_width = app.get_addr_col_width();
    for _ in 0..addr_width {
        result.push(' ');
    }
    for i in 0..bpl {
        let _ = write!(result, "{:02X} ", i);
    }
    result.push_str("\r\n");

    // 2. Row by row dump
    let start_row_offset = (start / bpl) * bpl;
    let end_row_offset = (end / bpl) * bpl;

    let mut current_row = start_row_offset;
    while current_row <= end_row_offset && current_row < app.file_info.size {
        // Address column
        let addr = if app.hex_view.show_va {
            app.get_va(current_row)
        } else {
            current_row as u64
        };
        let _ = write!(result, "{:08X} ", addr);

        // Hex bytes & ASCII part
        let mut ascii_part = String::with_capacity(bpl);
        for i in 0..bpl {
            let cur_offset = current_row + i;
            if cur_offset >= start && cur_offset <= end && cur_offset < app.file_info.size {
                if let Some(b) = app.read_u8(cur_offset) {
                    let _ = write!(result, "{:02X} ", b);
                    if b.is_ascii_graphic() || b == b' ' {
                        ascii_part.push(b as char);
                    } else {
                        ascii_part.push('.');
                    }
                } else {
                    result.push_str("   ");
                    ascii_part.push(' ');
                }
            } else {
                result.push_str("   ");
                ascii_part.push(' ');
            }
        }

        result.push_str(" ");
        result.push_str(&ascii_part);
        result.push_str("\r\n");

        current_row += bpl;
    }

    result
}

/// Colours the block at the cursor, or makes one out of the active selection.
///
/// Alt+M had two halves and only one of them was reachable. This one - "recolour
/// the block under the cursor" - lived in Normal mode, and "make a block out of
/// what is selected" lived in the `v` selection mode that the vi bindings took with
/// them when they went: a Shift+arrow selection leaves the state as Normal, so
/// there was no way left to create a block at all. Both halves are here now, and
/// both key handlers call this.
pub fn color_block_at_cursor(app: &mut App) {
    let offset = app.hex_view.offset;

    // An existing block wins, so pressing Alt+M again cycles its colour rather than
    // stacking a second block on top of the first.
    for block in &mut app.hex_view.blocks {
        if offset >= block.start && offset <= block.end {
            block.set_random_color();
            let (start, end) = (block.start, block.end);
            crate::app::App::log(
                app,
                format!("Recoloured the block at 0x{:X}..0x{:X}", start, end),
            );
            return;
        }
    }

    let (start, end) = (app.hex_view.selection.start, app.hex_view.selection.end);
    if start == end {
        // Nothing to colour and nothing selected: say so rather than doing nothing,
        // which is what made this look broken.
        app.error("Alt+M needs a selection (Shift+arrows) or a block at the cursor".to_string());
        return;
    }

    app.hex_view
        .blocks
        .push(ColoredBlock::new(start.min(end), start.max(end)));
    // Sorted so the block-jump keys walk them in file order.
    app.hex_view.blocks.sort_by_key(|k| k.start);
    crate::app::App::log(app, format!("Coloured 0x{:X}..0x{:X}", start.min(end), start.max(end)));
}

/// Fills every byte of the active selection with `value` and logs the count.
///
/// Shared by the Insert (00) and Delete (90) block-fill shortcuts in both
/// 'v' selection mode and Normal mode (Shift-selection).
pub fn fill_selection_with(app: &mut App, value: u8) {
    let mut count = 0usize;
    for offset in app.hex_view.selection {
        crate::hex::edit::record_edit(app, offset, value);
        count += 1;
    }
    crate::app::App::log(app, format!("Filled {} byte(s) with 0x{:02X}", count, value));
}

pub fn select_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.state = UIState::Normal;
            app.dialog_renderer = None;
            app.hex_view.editing_hex = true;
            app.hex_view.selection.clear();
        }
        KeyCode::Enter => {
            // Retain selected block on screen until Esc is pressed
            app.state = UIState::Normal;
            app.dialog_renderer = None;
        }

        // Navigation. The vi aliases h/j/k/l are gone with the rest of them; this
        // state is now reached by a mouse drag, where arrow keys are what a user
        // reaches for anyway.
        KeyCode::Left => {
            let new_offset = app.hex_view.offset.saturating_sub(1);

            app.hex_view.selection.select_left_or_up(1);
            app.goto(new_offset);
        }
        KeyCode::Right => {
            let new_offset = app.hex_view.offset + 1;

            // return if at the last offset
            if new_offset >= app.file_info.size {
                return Ok(true);
            }

            app.hex_view
                .selection
                .select_right_or_down(app.file_info.buffer_len().saturating_sub(1), 1);
            app.goto(new_offset);
        }
        KeyCode::Up => {
            let new_offset = app
                .hex_view
                .offset
                .saturating_sub(app.config.hex_mode_bytes_per_line);

            if app.hex_view.selection.direction == Some(Direction::RightOrDown)
                && new_offset < app.hex_view.selection.start
            {
                return Ok(true);
            }

            app.hex_view
                .selection
                .select_left_or_up(app.config.hex_mode_bytes_per_line);
            app.goto(new_offset);
        }
        KeyCode::Down => {
            // `size - 1` underflowed on a zero-byte file.
            let new_offset = app
                .hex_view
                .offset
                .saturating_add(app.config.hex_mode_bytes_per_line)
                .min(app.file_info.size.saturating_sub(1));

            if app.hex_view.selection.direction == Some(Direction::LeftOrUp)
                && new_offset > app.hex_view.selection.end
            {
                return Ok(true);
            }

            app.hex_view.selection.select_right_or_down(
                app.file_info.buffer_len().saturating_sub(1),
                app.config.hex_mode_bytes_per_line,
            );
            app.goto(new_offset);
        }

        // Actions
        // fill with zero (Insert), staying in Normal mode afterwards
        KeyCode::Insert => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoFillZero);
                return Ok(true);
            }

            fill_selection_with(app, 0x00);
            app.state = UIState::Normal;
            app.hex_view.selection.clear();
        }
        // fill with NOPs (Delete), staying in Normal mode afterwards
        KeyCode::Delete => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoFillNop);
                return Ok(true);
            }

            fill_selection_with(app, 0x90);
            app.state = UIState::Normal;
            app.hex_view.selection.clear();
        }
        // change case
        KeyCode::Char('~') => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoCase);
                return Ok(true);
            }

            // `selection.clear()` used to be inside this loop, so the very first
            // byte cleared the range being iterated and only that byte was ever
            // case-flipped.
            for offset in app.hex_view.selection {
                if let Some(b) = app.read_u8(offset) {
                    if b.is_ascii_lowercase() {
                        let up = b.to_ascii_uppercase();
                        crate::hex::edit::record_edit(app, offset, up);
                    } else if b.is_ascii_uppercase() {
                        let low = b.to_ascii_lowercase();
                        crate::hex::edit::record_edit(app, offset, low);
                    }
                }
            }
            app.hex_view.selection.clear();
            app.state = UIState::Normal;
        }
        // copy the block (Ctrl+C, was the vi-style 'y')
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let s = format_selection_for_target(app);
            if let Ok(clip) = app.clipboard.as_mut() {
                let _ = clip.set_text(s);
            }
            app.state = UIState::Normal;
            app.hex_view.selection.clear();
        }
        // revert the changed bytes in the block (Ctrl+Z, was 'u')
        KeyCode::Char('z') | KeyCode::Char('Z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let count = crate::hex::edit::revert_range(
                app,
                app.hex_view.selection.start,
                app.hex_view.selection.end,
            );

            if count > 0 {
                crate::app::App::log(app, format!("Reverted {} changed byte(s) in selected block", count));
            } else {
                crate::beep!();
            }

            app.hex_view.selection.clear();
            app.state = UIState::Normal;
        }
        // modify block data (Ctrl+K, was 'm' / Ctrl+M)
        KeyCode::Char('k') | KeyCode::Char('K') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoModifyBlock);
                return Ok(true);
            }
            app.state = UIState::DialogModifyBlock;
            app.dialog_renderer = Some(crate::hex::modify_dialog::draw_modify_dialog);
            app.hex_view.modify_dialog.reset();
        }
        // set a random color for an existing block or create a new one
        KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::ALT) => {
            color_block_at_cursor(app);
            app.state = UIState::Normal;
        }
        // Edit Data dialog (Ctrl+E), bound here too so a live selection can open it
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            crate::hex::edit_dialog::open_edit_dialog(app);
        }
        // encoding dialogs: Alt+E for primary, Alt+Shift+E for secondary
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::ALT) => {
            if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('E') {
                app.state = UIState::DialogEncoding2;
                app.dialog_renderer = Some(crate::text::dialog_encoding::dialog_encoding2_draw);
            } else {
                app.state = UIState::DialogEncoding;
                app.dialog_renderer = Some(crate::text::dialog_encoding::dialog_encoding_draw);
            }
        }
        _ => {}
    }

    Ok(false)
}

#[cfg(test)]
mod selection_bounds_tests {
    use super::*;

    /// The range is inclusive, so the forward bound is the last valid offset.
    ///
    /// With `size` passed instead of `size - 1`, `end` reached one past EOF; the
    /// fill actions then recorded a `changed_bytes` entry there and `:w` grew the
    /// file by a byte.
    #[test]
    fn forward_selection_stops_at_the_last_byte() {
        let size = 0x100usize;
        let last = size - 1;

        let mut sel = Selection::default();
        sel.start = last;
        sel.end = last;

        // Repeatedly extend well past the end of the file.
        for _ in 0..4 {
            sel.select_right_or_down(last, 0x40);
        }

        assert_eq!(sel.end, last, "selection end must not pass the last byte");
        assert!(
            !sel.contains(size),
            "offset {} is past EOF and must not be selected",
            size
        );
        for ofs in sel {
            assert!(ofs < size, "iterated offset {:X} is past EOF", ofs);
        }
    }

    /// A zero-length file must not underflow or select anything.
    #[test]
    fn empty_file_selects_nothing() {
        let last = 0usize.saturating_sub(1); // what the call sites compute for size 0
        let mut sel = Selection::default();
        sel.select_right_or_down(last, 16);
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 0);
    }
}

#[cfg(test)]
mod block_revert_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn app_with_block_edits() -> App {
        let mut app = App::new();
        app.config.database = false;
        for ofs in 0x10..0x14usize {
            app.hex_view.changed_bytes.insert(ofs, 0x90);
            app.hex_view.changed_history.push(ofs);
        }
        app.hex_view.selection.start = 0x10;
        app.hex_view.selection.end = 0x13;
        app.state = UIState::HexSelection;
        app
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = select_events(app, key);
    }

    /// Reverting a block (Ctrl+Z, formerly `u`) must be redoable.
    ///
    /// The block path did not record the removed bytes, so Ctrl+Y had nothing to
    /// put back - while undoing a single byte was redoable. Same key, two
    /// behaviours.
    #[test]
    fn block_revert_is_redoable() {
        let mut app = app_with_block_edits();

        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "the block's edits should be reverted"
        );
        assert_eq!(
            app.hex_view.redo_history.len(),
            4,
            "Ctrl+Y must be able to put all four bytes back"
        );

        // And redo actually restores them.
        while let Some((ofs, val)) = app.hex_view.redo_history.pop() {
            app.hex_view.changed_bytes.insert(ofs, val);
        }
        assert_eq!(app.hex_view.changed_bytes.len(), 4);
        for ofs in 0x10..0x14usize {
            assert_eq!(
                app.hex_view.changed_bytes.get(&ofs).copied(),
                Some(0x90)
            );
        }
    }
}

#[cfg(test)]
mod column_aware_yank_tests {
    use super::*;
    use crate::editor::EditingTarget;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// "AB" in CP949 (0xB0 0xA1 is the Hangul syllable 가), then plain ASCII.
    ///
    /// Chosen so the three columns cannot produce the same string by accident: the
    /// bytes, the UTF-8 reading and the CP949 reading all differ.
    const BYTES: &[u8] = &[0xB0, 0xA1, b'H', b'i'];

    /// `X` then three control bytes, `Y`, two more, `Z` - the shape of the screenshot
    /// that prompted this: mostly non-printable with a few letters.
    const CONTROL_BYTES: &[u8] = &[b'X', 0x00, 0x01, 0x02, b'Y', 0x1F, 0x7F, b'Z'];

    fn app_with_control_bytes() -> App {
        app_from(CONTROL_BYTES)
    }

    fn app_with_bytes() -> App {
        app_from(BYTES)
    }

    fn app_from(bytes: &[u8]) -> App {
        let dir = std::env::temp_dir().join("dz6_yank_target");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("b_{n}.bin"));
        let mut blob = bytes.to_vec();
        blob.resize(0x40, 0);
        std::fs::write(&path, &blob).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.hex_view.selection.start = 0;
        app.hex_view.selection.end = 3;
        app
    }

    /// The byte column yields hex, unchanged from before.
    #[test]
    fn the_hex_column_yields_hex_bytes() {
        let mut app = app_with_bytes();
        app.hex_view.selection_target = EditingTarget::Hex;

        assert_eq!(format_selection_for_target(&mut app), "B0 A1 48 69");
    }

    /// An encoding column yields the text those bytes spell in that encoding.
    #[test]
    fn an_encoding_column_yields_decoded_text() {
        let mut app = app_with_bytes();

        app.text_view.table = encoding_rs::EUC_KR;
        app.hex_view.selection_target = EditingTarget::Enc1;
        assert_eq!(
            format_selection_for_target(&mut app),
            "가Hi",
            "CP949 reads B0 A1 as one Hangul syllable"
        );

        app.hex_view.enc2_table = Some(encoding_rs::UTF_8);
        app.hex_view.selection_target = EditingTarget::Enc2;
        let utf8 = format_selection_for_target(&mut app);
        assert!(
            utf8.ends_with("Hi") && utf8 != "가Hi",
            "the same bytes are not valid UTF-8 and must decode differently, got '{utf8}'"
        );
    }

    /// Non-printable bytes copy as the placeholder the column draws, not as the
    /// control characters themselves.
    ///
    /// Decoding raw meant the clipboard held something other than what was on screen,
    /// and a `0x00` truncated the whole string, since Windows clipboard text ends at
    /// the first NUL - so most of a selection could silently vanish.
    #[test]
    fn non_printable_bytes_copy_as_the_placeholder() {
        let mut app = app_with_control_bytes();
        app.text_view.table = encoding_rs::UTF_8;
        app.hex_view.selection_target = EditingTarget::Enc1;
        app.hex_view.selection.start = 0;
        app.hex_view.selection.end = 7;

        let copied = format_selection_for_target(&mut app);

        assert_eq!(
            copied, "X...Y..Z",
            "the copy must read like the column: printable characters kept, the rest \
             replaced by the placeholder"
        );
        assert!(
            !copied.contains('\0'),
            "a NUL on the clipboard truncates everything after it"
        );
        assert_eq!(
            copied.chars().count(),
            8,
            "one character per byte, so the copy lines up with the selection"
        );
    }

    /// The placeholder follows `:set ctrlchar`.
    #[test]
    fn the_placeholder_follows_the_setting() {
        let mut app = app_with_control_bytes();
        app.text_view.table = encoding_rs::UTF_8;
        app.hex_view.selection_target = EditingTarget::Enc1;
        app.hex_view.selection.start = 0;
        app.hex_view.selection.end = 7;
        app.config.hex_mode_non_graphic_char = ' ';

        assert_eq!(format_selection_for_target(&mut app), "X   Y  Z");
    }

    /// The hex column is unaffected: it still hands over exact bytes.
    #[test]
    fn the_hex_column_still_copies_exact_bytes() {
        let mut app = app_with_control_bytes();
        app.hex_view.selection_target = EditingTarget::Hex;
        app.hex_view.selection.start = 0;
        app.hex_view.selection.end = 3;

        assert_eq!(
            format_selection_for_target(&mut app),
            "58 00 01 02",
            "byte-accurate copying is what the hex column is for"
        );
    }

    /// Pending edits are copied, not the bytes on disk.
    #[test]
    fn pending_edits_are_included() {
        let mut app = app_with_bytes();
        app.hex_view.selection_target = EditingTarget::Hex;
        app.hex_view.changed_bytes.insert(0, 0x41);

        assert_eq!(
            format_selection_for_target(&mut app),
            "41 A1 48 69",
            "the column shows the edited byte, so the copy must contain it"
        );
    }

    /// Shift+arrows in edit mode build a selection and record the focused column.
    ///
    /// This is the only way to select inside an encoding column: there the letters
    /// that start a selection in Normal mode are text being typed into the file.
    #[test]
    fn shift_arrows_select_inside_an_encoding_column() {
        let mut app = app_with_bytes();
        app.hex_view.selection.clear();
        app.state = UIState::HexEditing;
        app.hex_view.editing_target = EditingTarget::Enc1;
        app.hex_view.offset = 0;

        for _ in 0..3 {
            let key = KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = crate::hex::edit::edit_events(&mut app, key);
        }

        assert_eq!(app.hex_view.selection.start, 0);
        assert_eq!(app.hex_view.selection.end, 3, "the block must extend as Shift is held");
        assert_eq!(
            app.hex_view.selection_target,
            EditingTarget::Enc1,
            "the column with focus decides what a yank means"
        );
        assert!(
            app.state == UIState::HexEditing,
            "selecting must not drop out of edit mode"
        );
    }

    /// Ctrl+C in edit mode copies rather than typing a 'c' into the file.
    #[test]
    fn ctrl_c_copies_and_does_not_type() {
        let mut app = app_with_bytes();
        app.state = UIState::HexEditing;
        app.hex_view.editing_target = EditingTarget::Enc1;
        app.hex_view.selection_target = EditingTarget::Enc1;

        let key = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::hex::edit::edit_events(&mut app, key);

        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "Ctrl+C must not write a literal 'c' into the file"
        );
    }

    /// Moving without Shift drops the block, so a stale selection cannot be copied
    /// after the cursor has left it.
    #[test]
    fn plain_arrows_drop_the_selection() {
        let mut app = app_with_bytes();
        app.state = UIState::HexEditing;
        app.hex_view.offset = 0;
        app.hex_view.shift_anchor = Some(0);

        let key = KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::hex::edit::edit_events(&mut app, key);

        assert_eq!(app.hex_view.selection.start, app.hex_view.selection.end);
        assert!(app.hex_view.shift_anchor.is_none());
    }

    /// Leaving edit mode returns focus to the byte column.
    #[test]
    fn leaving_edit_mode_returns_focus_to_hex() {
        let mut app = app_with_bytes();
        app.state = UIState::HexEditing;
        app.hex_view.editing_target = EditingTarget::Enc2;

        let key = KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::hex::edit::edit_events(&mut app, key);

        assert_eq!(
            app.hex_view.editing_target,
            EditingTarget::Hex,
            "a later 'v' in Normal mode must mean a hex selection"
        );
    }
}
