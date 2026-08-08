use ratatui::crossterm::event::KeyModifiers;

use crate::app::App;


use crate::editor::UIState;

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::io::Result;

/// Reverts every pending edit in `start..=end`, returning how many were undone.
///
/// The redo stack is the reason this is shared. Four places reverted a range -
/// `perform_undo` in `hex/events.rs` and in `disasm/events.rs`, plus the `u`
/// handlers in `hex/events.rs` and `hex/selection.rs` - and they had drifted:
/// only one pushed the removed values onto `redo_history`, so reverting a block
/// with `u` could not be taken back with Ctrl+Y while a single-byte undo could.
pub fn revert_range(app: &mut App, start: usize, end: usize) -> usize {
    let (start, end) = (start.min(end), start.max(end));

    let mut count = 0;
    for ofs in start..=end {
        if let Some(val) = app.hex_view.changed_bytes.remove(&ofs) {
            app.hex_view.redo_history.push((ofs, val));
            count += 1;
        }
    }
    app.hex_view
        .changed_history
        .retain(|ofs| *ofs < start || *ofs > end);


    // A half-typed byte inside the reverted range no longer exists.
    if app
        .hex_view
        .nibble_pending
        .is_some_and(|ofs| ofs >= start && ofs <= end)
    {
        app.hex_view.nibble_pending = None;
    }

    count
}

/// Byte currently shown at `offset`: the pending edit if there is one, otherwise
/// what is on disk.
pub fn displayed_byte(app: &App, offset: usize) -> u8 {
    if let Some(&b) = app.hex_view.changed_bytes.get(&offset) {
        return b;
    }
    app.file_info
        .get_buffer_ref()
        .get(offset)
        .copied()
        .unwrap_or(0)
}

/// Stages `new` at `offset` as a pending edit.
///
/// The single place a byte becomes pending. The call sites used to do the two
/// steps by hand - insert into `changed_bytes`, push onto `changed_history` - and
/// they disagreed: some pushed the offset for every byte of a multi-byte write and
/// some pushed it once for the whole field, so Ctrl+Z either needed several
/// presses for one edit or left most of a header field staged.
pub fn record_edit(app: &mut App, offset: usize, new: u8) {
    if offset >= app.file_info.size {
        return;
    }

    // One `changed_history` entry per offset, recorded when the byte is first
    // touched, so a single Ctrl+Z takes the whole edit back.
    if !app.hex_view.changed_bytes.contains_key(&offset) {
        app.hex_view.changed_history.push(offset);
    }
    app.hex_view
        .changed_bytes
        .insert(offset, new);
}
/// Applies one typed hex digit to the byte under the cursor.
///
/// Two nibbles make a byte, and `changed_bytes` only ever holds whole ones. The
/// first digit replaces the high nibble and keeps the low one, the second
/// replaces the low nibble and moves on.
///
/// This replaces a scheme where the first digit was stored as a one-character
/// string. Every consumer of `changed_bytes` parses it with `from_str_radix`, so
/// that half-typed state was indistinguishable from a finished `0x0N`: the hex
/// view drew it, and `:w` wrote it to the file. The old code also pushed the
/// offset onto `changed_history` for *each* digit, so undoing one byte took two
/// Ctrl+Z presses.
fn type_hex_nibble(app: &mut App, digit: char) {
    let offset = app.hex_view.offset;
    let Some(value) = digit.to_digit(16) else {
        return;
    };
    let value = value as u8;

    let completing = app.hex_view.nibble_pending == Some(offset);
    let current = displayed_byte(app, offset);

    let new_byte = if completing {
        (current & 0xF0) | value
    } else {
        (value << 4) | (current & 0x0F)
    };

    // Coalescing, because the second digit stages the same byte again: one byte
    // typed is one row in the history, not two.
    record_edit(app, offset, new_byte);

    if completing {
        app.hex_view.nibble_pending = None;
        app.goto(offset + 1);
    } else {
        app.hex_view.nibble_pending = Some(offset);
    }
}

pub fn fill_with(app: &mut App, with: u8, advance: bool) {
    let offset = app.hex_view.offset;
    record_edit(app, offset, with);
    if advance {
        app.goto(app.hex_view.offset + 1);
    }
}

pub fn edit_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Any key other than a hex digit ends the half-typed byte, so the next digit
    // starts a fresh high nibble instead of completing one at a stale offset.
    if !matches!(key.code, KeyCode::Char(c) if c.is_ascii_hexdigit()) {
        app.hex_view.nibble_pending = None;
    }

    // Navigation goes through the same helper Normal mode uses, so every movement
    // key works here too. Edit mode used to handle only the four arrows, which left
    // Home, End, PageUp, PageDown and Ctrl+Home/End dead the moment F2 was pressed.
    //
    // Shift extends a selection instead of just moving, which is what makes a block
    // in an encoding column reachable at all: there the letters that start a
    // selection in Normal mode are text being typed into the file.
    if let Some(target) = crate::hex::events::movement_target(app, &key) {
        if key.modifiers.contains(KeyModifiers::SHIFT) && app.file_info.buffer_len() > 0 {
            let anchor = *app.hex_view.shift_anchor.get_or_insert(app.hex_view.offset);
            app.hex_view.selection_target = app.hex_view.editing_target;
            app.hex_view.selection.start = anchor.min(target);
            app.hex_view.selection.end = anchor.max(target);
            app.hex_view.selection.direction = None;
            app.hex_view.selection.is_mouse = false;
        } else {
            // Moving without Shift drops the block, so a stale selection cannot be
            // copied after the cursor has left it.
            app.hex_view.selection.clear();
            app.hex_view.shift_anchor = None;
        }
        app.goto(target);
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.state = UIState::Normal;
            // app.hex_view.changed_bytes.clear();
            app.dialog_renderer = None;
            app.hex_view.editing_hex = true;
            // Focus returns to the byte column, so a later `v` in Normal mode means
            // a hex selection rather than inheriting whichever encoding column was
            // last edited.
            app.hex_view.editing_target = crate::editor::EditingTarget::Hex;
            app.hex_view.shift_anchor = None;
        }

        // Ctrl+C copies the selection in the terms of the column that has focus.
        //
        // Handled before the `Char` arm below, which would otherwise type a literal
        // 'c' into the file when an encoding column has focus.
        KeyCode::Char('c') | KeyCode::Char('C')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            if app.hex_view.selection.start == app.hex_view.selection.end {
                crate::beep!();
            } else {
                let s = crate::hex::selection::format_selection_for_target(app);
                let what = match app.hex_view.selection_target {
                    crate::editor::EditingTarget::Hex => "hex bytes".to_string(),
                    crate::editor::EditingTarget::Enc1 => {
                        format!("{} text", app.text_view.table.name())
                    }
                    crate::editor::EditingTarget::Enc2 => {
                        format!("{} text", app.hex_view.get_enc2_table().name())
                    }
                };
                if let Ok(clip) = app.clipboard.as_mut() {
                    let _ = clip.set_text(s);
                }
                crate::app::App::log(app, format!("Selection copied to clipboard as {}", what));
            }
        }

        // Ctrl+E opens the Edit Data dialog from inside edit mode too.
        //
        // It was Normal-mode only, which meant leaving edit mode (Esc), pressing
        // Ctrl+E and coming back - for the one dialog whose whole purpose is
        // entering bytes at the cursor. Handled before the `Char` arm below, like
        // Ctrl+C, so an encoding column does not type a literal 'e' into the file.
        KeyCode::Char('e') | KeyCode::Char('E')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoEditData);
            } else {
                app.state = UIState::DialogEditData;
                app.hex_view.edit_dialog.reset();
                let enc1 = app.text_view.table;
                app.hex_view.edit_dialog.set_enc1(enc1);
                app.dialog_renderer = Some(super::edit_dialog::dialog_edit_draw);
            }
        }

        // Backspace also steps left, which is not a movement key elsewhere.
        KeyCode::Backspace if app.hex_view.offset > 0 => {
            app.hex_view.selection.clear();
            app.hex_view.shift_anchor = None;
            app.goto(app.hex_view.offset - 1);
        }

        KeyCode::Tab => {
            use crate::editor::EditingTarget;
            let mut next_target = app.hex_view.editing_target.next();
            if next_target == EditingTarget::Enc2 && app.hex_view.enc2_table.is_none() {
                next_target = EditingTarget::Hex;
            }
            app.hex_view.editing_target = next_target;
            app.hex_view.editing_hex = app.hex_view.editing_target == EditingTarget::Hex;
            let target_name = match app.hex_view.editing_target {
                EditingTarget::Hex => "HEX".to_string(),
                EditingTarget::Enc1 => format!("ENC1 ({})", app.text_view.table.name()),
                EditingTarget::Enc2 => format!("ENC2 ({})", app.hex_view.get_enc2_table().name()),
            };
            crate::app::App::log(app, format!("Edit target switched to {}", target_name));
        }

        // Ctrl-combinations are commands, never text. Without this guard an
        // encoding column typed a literal 'z' into the file for Ctrl+Z, and the byte
        // column ran its `z` fill-with-zero shortcut.
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            use crate::editor::EditingTarget;
            match app.hex_view.editing_target {
                EditingTarget::Hex => {
                    if c.is_ascii_hexdigit() && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        type_hex_nibble(app, c);
                    // Edit mode is for typing bytes and nothing else now. The bare
                    // letters it used to carry are gone: 'z'/'n' (fill 0x00 / 0x90)
                    // duplicated Insert and Delete, and 't'/'T' truncated the file -
                    // a destructive, size-changing operation reached by one letter in
                    // the middle of typing.
                    } else if c == '~'
                        && let Some(b) = app.read_u8(app.hex_view.offset)
                    {
                        if b.is_ascii_lowercase() {
                            fill_with(app, b.to_ascii_uppercase(), true);
                        } else if b.is_ascii_uppercase() {
                            fill_with(app, b.to_ascii_lowercase(), true);
                        } else {
                            app.goto(app.hex_view.offset.saturating_add(1));
                        }
                    }
                }
                EditingTarget::Enc1 => {
                    let encoded_bytes = crate::util::encode_char(c, app.text_view.table);
                    let mut ofs = app.hex_view.offset;
                    for &b in encoded_bytes.iter() {
                        if ofs < app.file_info.size {
                            record_edit(app, ofs, b);
                            ofs += 1;
                        } else {
                            break;
                        }
                    }
                    app.goto(ofs);
                }
                EditingTarget::Enc2 => {
                    let enc2 = app.hex_view.get_enc2_table();
                    let encoded_bytes = crate::util::encode_char(c, enc2);
                    let mut ofs = app.hex_view.offset;
                    for &b in encoded_bytes.iter() {
                        if ofs < app.file_info.size {
                            record_edit(app, ofs, b);
                            ofs += 1;
                        } else {
                            break;
                        }
                    }
                    app.goto(ofs);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod nibble_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn app_with(bytes: &[u8]) -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dz6_nibble");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("b_{}.bin", id));
        std::fs::write(&path, bytes).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true)
            .expect("open");
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = edit_events(app, key);
    }

    fn byte_at(app: &App, offset: usize) -> Option<u8> {
        app.hex_view
            .changed_bytes
            .get(&offset)
            .copied()
    }

    /// `changed_bytes` must only ever hold whole bytes.
    ///
    /// A half-typed nibble used to be stored as a one-character string, and every
    /// consumer parses that map with `from_str_radix` - so one keystroke was
    /// already a committed `0x0N` that the view drew and `:w` wrote to disk.
    #[test]
    fn a_single_digit_never_commits_a_half_byte() {
        let mut app = app_with(&[0xAB; 0x40]);
        app.hex_view.offset = 0x10;

        press(&mut app, KeyCode::Char('7'));

        let stored = app.hex_view.changed_bytes.get(&0x10).copied();
        assert_eq!(
            stored,
            Some(0x7B),
            "the first digit replaces the high nibble and keeps the low one"
        );
        assert_ne!(
            byte_at(&app, 0x10),
            Some(0x07),
            "a lone digit must not be interpreted as 0x07"
        );
    }

    /// The second digit completes the byte and moves on.
    #[test]
    fn two_digits_make_the_byte_and_advance() {
        let mut app = app_with(&[0xAB; 0x40]);
        app.hex_view.offset = 0x10;

        press(&mut app, KeyCode::Char('7'));
        press(&mut app, KeyCode::Char('f'));

        assert_eq!(byte_at(&app, 0x10), Some(0x7F));
        assert_eq!(app.hex_view.offset, 0x11, "cursor advances after a full byte");
        assert!(app.hex_view.nibble_pending.is_none());
    }

    /// One byte edited, one undo entry.
    ///
    /// The old code pushed the offset for each digit, so undoing a single byte
    /// took two Ctrl+Z presses.
    #[test]
    fn one_byte_produces_one_undo_entry() {
        let mut app = app_with(&[0xAB; 0x40]);
        app.hex_view.offset = 0x10;

        press(&mut app, KeyCode::Char('7'));
        press(&mut app, KeyCode::Char('f'));

        assert_eq!(
            app.hex_view.changed_history,
            vec![0x10],
            "the offset must appear exactly once"
        );
    }

    /// Typing over an already-edited byte doesn't add a second entry either.
    #[test]
    fn retyping_the_same_byte_adds_no_extra_history() {
        let mut app = app_with(&[0xAB; 0x40]);
        app.hex_view.offset = 0x10;

        press(&mut app, KeyCode::Char('1'));
        press(&mut app, KeyCode::Char('2'));
        app.hex_view.offset = 0x10;
        press(&mut app, KeyCode::Char('3'));
        press(&mut app, KeyCode::Char('4'));

        assert_eq!(byte_at(&app, 0x10), Some(0x34));
        assert_eq!(app.hex_view.changed_history, vec![0x10]);
    }

    /// Moving the cursor abandons the half-typed byte, so the next digit starts a
    /// new high nibble rather than completing one somewhere else.
    #[test]
    fn moving_the_cursor_ends_the_pending_byte() {
        let mut app = app_with(&[0xAB; 0x40]);
        app.hex_view.offset = 0x10;

        press(&mut app, KeyCode::Char('7')); // 0x10 -> 0x7B, pending
        press(&mut app, KeyCode::Right); // cursor to 0x11, pending cleared
        assert!(app.hex_view.nibble_pending.is_none());

        press(&mut app, KeyCode::Char('c'));
        assert_eq!(byte_at(&app, 0x10), Some(0x7B), "0x10 keeps its edit");
        assert_eq!(
            byte_at(&app, 0x11),
            Some(0xCB),
            "the digit at 0x11 starts a fresh high nibble"
        );
    }

    /// Leaving edit mode must not leave a dangling phase behind.
    #[test]
    fn leaving_edit_mode_clears_the_pending_byte() {
        let mut app = app_with(&[0x00; 0x40]);
        app.hex_view.offset = 0;
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(app.hex_view.nibble_pending, Some(0));

        press(&mut app, KeyCode::Esc);
        assert!(app.hex_view.nibble_pending.is_none());
    }
}

#[cfg(test)]
mod revert_range_tests {
    use super::*;

    fn app_with_edits() -> App {
        let mut app = App::new();
        app.config.database = false;
        for ofs in 0x10..0x18usize {
            app.hex_view
                .changed_bytes
                .insert(ofs, ofs as u8);
            app.hex_view.changed_history.push(ofs);
        }
        app
    }

    /// Reverting a block must leave the removed values on the redo stack.
    ///
    /// Only one of the four range-revert paths did this, so `u` over a block could
    /// not be taken back with Ctrl+Y while a single-byte undo could.
    #[test]
    fn reverting_a_range_fills_the_redo_stack() {
        let mut app = app_with_edits();

        let count = revert_range(&mut app, 0x12, 0x14);

        assert_eq!(count, 3);
        assert_eq!(app.hex_view.redo_history.len(), 3, "Ctrl+Y needs these");
        let mut redone: Vec<usize> = app.hex_view.redo_history.iter().map(|(o, _)| *o).collect();
        redone.sort();
        assert_eq!(redone, vec![0x12, 0x13, 0x14]);
        // The values have to come back intact, not just the offsets.
        for (ofs, val) in &app.hex_view.redo_history {
            assert_eq!(*val, *ofs as u8);
        }
    }

    /// The reverted bytes are gone from the pending set and from the undo history;
    /// everything outside the range is untouched.
    #[test]
    fn only_the_range_is_reverted() {
        let mut app = app_with_edits();

        revert_range(&mut app, 0x12, 0x14);

        for ofs in 0x12..=0x14usize {
            assert!(!app.hex_view.changed_bytes.contains_key(&ofs));
            assert!(!app.hex_view.changed_history.contains(&ofs));
        }
        for ofs in [0x10usize, 0x11, 0x15, 0x16, 0x17] {
            assert!(app.hex_view.changed_bytes.contains_key(&ofs), "0x{:X}", ofs);
            assert!(app.hex_view.changed_history.contains(&ofs));
        }
    }

    /// A reversed range is accepted, since the selection can be built either way.
    #[test]
    fn reversed_bounds_work() {
        let mut app = app_with_edits();
        assert_eq!(revert_range(&mut app, 0x14, 0x12), 3);
    }

    /// Nothing to revert reports zero rather than pretending it worked.
    #[test]
    fn empty_range_reports_zero() {
        let mut app = app_with_edits();
        assert_eq!(revert_range(&mut app, 0x100, 0x200), 0);
        assert!(app.hex_view.redo_history.is_empty());
    }

    /// A half-typed byte inside the reverted range must not survive.
    #[test]
    fn pending_nibble_inside_the_range_is_dropped() {
        let mut app = app_with_edits();
        app.hex_view.nibble_pending = Some(0x13);

        revert_range(&mut app, 0x12, 0x14);

        assert!(
            app.hex_view.nibble_pending.is_none(),
            "the byte being typed no longer exists"
        );
    }

    /// A pending nibble elsewhere is left alone.
    #[test]
    fn pending_nibble_outside_the_range_survives() {
        let mut app = app_with_edits();
        app.hex_view.nibble_pending = Some(0x20);

        revert_range(&mut app, 0x12, 0x14);

        assert_eq!(app.hex_view.nibble_pending, Some(0x20));
    }
}

#[cfg(test)]
mod edit_navigation_tests {
    use super::*;
    use crate::editor::EditingTarget;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_in_edit_mode() -> App {
        let dir = std::env::temp_dir().join("dz6_edit_nav");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("n_{n}.bin"));
        std::fs::write(&path, vec![0xAAu8; 0x400]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.config.hex_mode_bytes_per_line = 16;
        // A page has to be non-zero for PageUp/PageDown to move; the draw loop sets
        // this from the terminal height at runtime.
        app.reader.page_current_size = 0x100;
        app.state = UIState::HexEditing;
        app
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = edit_events(app, key);
    }

    /// Every movement key works in edit mode, not just the arrows.
    ///
    /// F2 used to handle Left/Right/Up/Down only, so Home, End, PageUp, PageDown and
    /// Ctrl+Home/End were dead the moment edit mode was entered - the cursor simply
    /// would not move, with no feedback.
    #[test]
    fn page_and_line_keys_move_the_cursor() {
        let mut app = app_in_edit_mode();
        let page = app.reader.page_current_size;
        let last = app.file_info.buffer_len() - 1;

        app.hex_view.offset = 0x180;
        press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(app.hex_view.offset, 0x180 + page, "PageDown");

        press(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(app.hex_view.offset, 0x180, "PageUp");

        // Home goes to the start of the row, End to its last byte.
        app.hex_view.offset = 0x185;
        app.hex_view.cursor.x = 5;
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.hex_view.offset, 0x180, "Home");

        app.hex_view.offset = 0x185;
        app.hex_view.cursor.x = 5;
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(app.hex_view.offset, 0x18F, "End");

        press(&mut app, KeyCode::Home, KeyModifiers::CONTROL);
        assert_eq!(app.hex_view.offset, 0, "Ctrl+Home");

        press(&mut app, KeyCode::End, KeyModifiers::CONTROL);
        assert_eq!(app.hex_view.offset, last, "Ctrl+End");
    }

    /// Edit mode and Normal mode must agree on where a movement key lands.
    ///
    /// Both go through `movement_target` now; when edit mode had its own copy of the
    /// arithmetic the two could disagree.
    #[test]
    fn edit_mode_lands_where_normal_mode_does() {
        for code in [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ] {
            let mut editing = app_in_edit_mode();
            editing.hex_view.offset = 0x185;
            editing.hex_view.cursor.x = 5;

            let expected = crate::hex::events::movement_target(
                &editing,
                &KeyEvent {
                    code,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                },
            )
            .expect("every key here is a movement");

            press(&mut editing, code, KeyModifiers::NONE);
            assert_eq!(
                editing.hex_view.offset, expected,
                "{code:?} landed somewhere other than Normal mode would"
            );
        }
    }

    /// Movement must not be mistaken for typing: no byte may be edited by it.
    #[test]
    fn moving_does_not_change_any_byte() {
        let mut app = app_in_edit_mode();
        app.hex_view.editing_target = EditingTarget::Enc1;
        app.hex_view.offset = 0x100;

        for code in [
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Up,
            KeyCode::Down,
        ] {
            press(&mut app, code, KeyModifiers::NONE);
        }

        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "navigation wrote {} byte(s) into the file",
            app.hex_view.changed_bytes.len()
        );
        assert!(app.state == UIState::HexEditing, "must stay in edit mode");
    }

    /// Shift extends a selection with the page keys too, not only the arrows.
    #[test]
    fn shift_page_keys_extend_the_selection() {
        let mut app = app_in_edit_mode();
        app.hex_view.editing_target = EditingTarget::Enc2;
        app.hex_view.offset = 0x100;

        press(&mut app, KeyCode::End, KeyModifiers::SHIFT);

        assert_eq!(app.hex_view.selection.start, 0x100);
        assert_eq!(app.hex_view.selection.end, 0x10F, "Shift+End to the row's end");
        assert_eq!(
            app.hex_view.selection_target,
            EditingTarget::Enc2,
            "the focused column decides what a copy of this block means"
        );
    }

    /// Backspace still steps left; it is not one of the movement keys.
    #[test]
    fn backspace_still_steps_left() {
        let mut app = app_in_edit_mode();
        app.hex_view.offset = 0x40;
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.hex_view.offset, 0x3F);
    }
}
