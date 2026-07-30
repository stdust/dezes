use crate::beep;
use crate::{app::App, commands::Commands, editor::UIState, hex};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Result;

/// Bytes to paste: hex when the clipboard holds hex, otherwise the text encoded
/// through `encoding`.
///
/// `encoding` is the column that has focus. It used to be hardcoded to UTF-8 -
/// literally `clean.as_bytes()` - so pasting into a CP949 column wrote UTF-8:
/// copying `가` out of that column gave back `EA B0 80` where the file had
/// `B0 A1`, three bytes instead of two and none of them the same. Typing the very
/// same character already went through `encode_char`, so paste was the one path
/// that ignored the column.
pub(crate) fn parse_hex_or_text_bytes(
    text: &str,
    encoding: &'static encoding_rs::Encoding,
) -> Vec<u8> {
    let clean = text.trim();
    if clean.is_empty() {
        return Vec::new();
    }

    let cleaned = clean.replace("0x", " ").replace("0X", " ").replace(',', " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();

    let mut bytes = Vec::new();
    let mut all_valid_hex = !tokens.is_empty();

    for token in &tokens {
        if token.len() == 2 || token.len() == 1 {
            if let Ok(b) = u8::from_str_radix(token, 16) {
                bytes.push(b);
            } else {
                all_valid_hex = false;
                break;
            }
        } else if token.len() % 2 == 0 {
            let mut sub_bytes = Vec::new();
            let mut valid_chunk = true;
            for chunk in token.as_bytes().chunks(2) {
                if let Ok(s) = std::str::from_utf8(chunk) {
                    if let Ok(b) = u8::from_str_radix(s, 16) {
                        sub_bytes.push(b);
                    } else {
                        valid_chunk = false;
                        break;
                    }
                } else {
                    valid_chunk = false;
                    break;
                }
            }
            if valid_chunk {
                bytes.extend(sub_bytes);
            } else {
                all_valid_hex = false;
                break;
            }
        } else {
            all_valid_hex = false;
            break;
        }
    }

    if all_valid_hex && !bytes.is_empty() {
        bytes
    } else {
        // Per character, through the same helper the keyboard uses, so a
        // multi-byte character comes out in the column's encoding rather than
        // UTF-8.
        clean
            .chars()
            .flat_map(|c| crate::util::encode_char(c, encoding))
            .collect()
    }
}

/// Where a plain movement key would take the cursor, or `None` if this key
/// isn't a movement.
///
/// Kept as one function, at module scope, so Normal mode, Shift-selection and the
/// F2 edit mode all navigate by the same arithmetic instead of each carrying a copy
/// that drifts out of step - which is how edit mode ended up supporting only the
/// four arrow keys while Home, End, PageUp and PageDown did nothing there.
pub fn movement_target(app: &App, key: &KeyEvent) -> Option<usize> {
    let bpl = app.config.hex_mode_bytes_per_line.max(1);
    let ofs = app.hex_view.offset;
    // The mapping bounds the cursor. `file_info.size` can exceed it, which put
    // the cursor where nothing is readable and drew an empty view.
    let last = app.file_info.buffer_len().saturating_sub(1);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    let target = match key.code {
        KeyCode::Left => ofs.saturating_sub(1),
        KeyCode::Right => (ofs + 1).min(last),
        KeyCode::Up => ofs.saturating_sub(bpl),
        KeyCode::Down => (ofs + bpl).min(last),
        KeyCode::Home => {
            if ctrl {
                0
            } else {
                ofs.saturating_sub(app.hex_view.cursor.x)
            }
        }
        KeyCode::End => {
            if ctrl {
                last
            } else {
                ofs.saturating_add(bpl)
                    .saturating_sub(app.hex_view.cursor.x)
                    .saturating_sub(1)
                    .min(last)
            }
        }
        KeyCode::PageDown => (ofs + app.reader.page_current_size).min(last),
        KeyCode::PageUp => ofs.saturating_sub(app.reader.page_current_size),
        _ => return None,
    };
    Some(target)
}

pub fn hex_mode_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    // this local function goes to the next/previous other byte
    // it is called when the user sends either 'o' or 'O'
    // Kept without a binding on purpose: 'o'/'O' were retired with the other bare
    // letters, and this is the implementation a menu or command entry will reuse.
    #[allow(dead_code)]
    fn goto_other_byte(app: &mut App, delta: isize) {
        let mut ofs = app.hex_view.offset;
        // `unwrap()` here aborted the process on an empty file or a stale offset.
        let Some(current_byte) = app.read_u8(ofs) else {
            crate::beep!();
            return;
        };

        while ofs < app.file_info.buffer_len() {
            if let Some(b) = app.read_u8(ofs)
                && b != current_byte
            {
                app.goto(ofs);
                break;
            }
            ofs = ofs.saturating_add_signed(delta);
            // this is needed because it can start at 0,
            // but it cannot be zero afterwards.
            // without it, `O` doesn't work at offset 0
            if ofs == 0 {
                app.goto(0);
                break;
            }
        }
    }

    fn perform_undo(app: &mut App) {
        if app.hex_view.selection.start != app.hex_view.selection.end {
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
        } else if let Some(ofs) = app.hex_view.changed_history.pop() {
            if let Some(val) = app.hex_view.changed_bytes.remove(&ofs) {
                app.hex_view.redo_history.push((ofs, val));
            }

            crate::app::App::log(app, format!("Undid change at offset 0x{:X}", ofs));
        } else {
            crate::beep!();
        }
    }

    fn perform_redo(app: &mut App) {
        if let Some((ofs, val)) = app.hex_view.redo_history.pop() {
            match u8::from_str_radix(val.trim(), 16) {
                Ok(b) => crate::hex::edit::record_edit(app, ofs, b),
                Err(_) => {
                    app.hex_view.changed_bytes.insert(ofs, val);
                    app.hex_view.changed_history.push(ofs);
                }
            }
            crate::app::App::log(app, format!("Redid change at offset 0x{:X}", ofs));
        } else {
            crate::beep!();
        }
    }

    /// Reverts *only* the byte under the cursor back to its on-disk value,
    /// regardless of when it was edited (Hiew's Alt+F3). The plain Undo walks
    /// `changed_history` in reverse chronological order, so it can't target a
    /// specific offset - this can, since `changed_bytes` is keyed by offset.
    fn revert_byte_at_cursor(app: &mut App) {
        let ofs = app.hex_view.offset;
        match app.hex_view.changed_bytes.remove(&ofs) {
            Some(val) => {
                // Drop it from the undo trail too, otherwise a later Ctrl+Z
                // would pop an offset that no longer has a pending edit and
                // silently do nothing.
                app.hex_view.changed_history.retain(|o| *o != ofs);
                app.hex_view.redo_history.push((ofs, val));
    
                // Advance one byte, the way the F2 edit-mode fills do: reverting
                // a run of patched bytes is the common case, so holding Alt+F3
                // walks through them. `goto` also recomputes cursor.x/y from
                // page_start and scrolls the page if the next byte is off-screen.
                // Clamped at EOF so the last byte in the file doesn't get stuck
                // (goto ignores out-of-range offsets).
                let next = (ofs + 1).min(app.file_info.buffer_len().saturating_sub(1));
                app.goto_with_history(next, false);
                crate::app::App::log(app, format!("Reverted byte at offset 0x{:X} to its original value", ofs));
            }
            None => crate::beep!(),
        }
    }


    /// Copies the active block, in the terms of the column it was selected in.
    ///
    /// This is the old 'y' (vi yank) body, now on Ctrl+C so that Normal mode and
    /// edit mode - which has always used Ctrl+C - agree.
    fn copy_selection_to_clipboard(app: &mut App) {
        if app.hex_view.selection.start == app.hex_view.selection.end {
            let message = crate::i18n::M::ErrNothingSelected.tr(app.config.lang).to_string();
            app.error(message);
            return;
        }

        let s = hex::selection::format_selection_for_target(app);
        let what = match app.hex_view.selection_target {
            crate::editor::EditingTarget::Hex => "hex bytes".to_string(),
            crate::editor::EditingTarget::Enc1 => format!("{} text", app.text_view.table.name()),
            crate::editor::EditingTarget::Enc2 => {
                format!("{} text", app.hex_view.get_enc2_table().name())
            }
        };
        if let Ok(clip) = app.clipboard.as_mut() {
            let _ = clip.set_text(s);
        }
        crate::app::App::log(app, format!("Selection copied to clipboard as {}", what));
    }

    fn paste_hex_bytes(app: &mut App) {
        if app.file_info.is_read_only {
            app.read_only_error(crate::i18n::M::RoPaste);
            return;
        }

        let text = if let Ok(clip) = app.clipboard.as_mut() {
            if let Ok(t) = clip.get_text() {
                t
            } else {
                crate::beep!();
                return;
            }
        } else {
            crate::beep!();
            return;
        };

        // The column with focus decides how text is encoded, matching what copying
        // out of that column produced.
        let encoding = match app.hex_view.editing_target {
            crate::editor::EditingTarget::Enc2 => app.hex_view.get_enc2_table(),
            // The byte column has no encoding of its own; enc1 is what its ASCII pane
            // shows, and it is also the sensible default for a plain text paste.
            _ => app.text_view.table,
        };
        let bytes = parse_hex_or_text_bytes(&text, encoding);
        if bytes.is_empty() {
            crate::beep!();
            return;
        }

        let start_ofs = app.hex_view.offset;
        // Bounded by the mapping: pasting past it would record edits at offsets
        // `:w` then seeks to, growing the file.
        let filesize = app.file_info.buffer_len();

        let mut count = 0;
        for (i, &b) in bytes.iter().enumerate() {
            let target_ofs = start_ofs + i;
            if target_ofs >= filesize {
                break;
            }
            crate::hex::edit::record_edit(app, target_ofs, b);
            count += 1;
        }

        if count > 0 {
            let new_ofs = (start_ofs + count).min(filesize.saturating_sub(1));
            app.goto(new_ofs);
            crate::app::App::log(app, format!("Pasted {} byte(s) from clipboard at 0x{:X}", count, start_ofs));
        }


    }

    // Shift + movement selects, exactly as it already does in the Disassembly
    // view - the Hex view used to require entering 'v' selection mode first,
    // so the same program had two different ways to mark a block. 'v' still
    // works and is still the better option for very long ranges.
    let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if let Some(target) = movement_target(app, &key) {
        if is_shift && app.file_info.buffer_len() > 0 {
            let anchor = *app.hex_view.shift_anchor.get_or_insert(app.hex_view.offset);
            // The column with focus decides what a yank of this block means.
            app.hex_view.selection_target = app.hex_view.editing_target;
            app.hex_view.selection.start = anchor.min(target);
            app.hex_view.selection.end = anchor.max(target);
            app.hex_view.selection.direction = None;
            app.hex_view.selection.is_mouse = false;
            app.goto(target);
            return Ok(false);
        }
        // Moving without Shift drops the selection, matching the Disasm view.
        if app.hex_view.shift_anchor.is_some() {
            app.hex_view.shift_anchor = None;
            app.hex_view.selection.clear();
        }
    }

    // it is important to call goto as it looks for the offset in the
    // cache and, in case it is not there, it reads the needed block, and
    // also checks and updates offset position, cursor position, etc.
    match key.code {
        // move left
        KeyCode::Left if app.hex_view.offset > 0 && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.goto(app.hex_view.offset - 1);
        }

        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::ALT) => {
            if let Some(b) = app.read_u8(app.hex_view.offset) {
                if app.hex_view.highlights.contains(&b) {
                    app.hex_view.highlights.remove(&b);
                } else {
                    app.hex_view.highlights.insert(b);
                }
            }
        }
        // move right
        KeyCode::Right if !key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.goto(app.hex_view.offset + 1);
        }
        // move up
        KeyCode::Up if app.hex_view.offset >= app.config.hex_mode_bytes_per_line => {
            app.goto(app.hex_view.offset - app.config.hex_mode_bytes_per_line);
        }
        // move down
        KeyCode::Down => {
            app.goto(app.hex_view.offset + app.config.hex_mode_bytes_per_line);
        }
        KeyCode::Home => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                app.goto(0);
            } else {
                // `saturating_sub`: cursor.x can exceed offset right after a
                // resize or a `:set byteline` change, before goto re-syncs it.
                app.goto(app.hex_view.offset.saturating_sub(app.hex_view.cursor.x));
            }
        }

        // EOL. The vi-style '$' alias is gone; End alone does this.
        KeyCode::End if app.file_info.buffer_len() > 0 => {
            let last_offset = app.file_info.buffer_len() - 1;
            // `Ctrl+End` goes to EOF too
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                app.goto(last_offset);
            } else {
                // `End` or `$` alone go to EOL
                let eol = app
                    .hex_view
                    .offset
                    .saturating_add(app.config.hex_mode_bytes_per_line)
                    .saturating_sub(app.hex_view.cursor.x)
                    .saturating_sub(1);
                app.goto(eol.min(last_offset));
            }
        }
        // go down one page
        KeyCode::PageDown => {
            app.goto(app.hex_view.offset + app.reader.page_current_size);
        }
        // go up one page
        KeyCode::PageUp => {
            app.goto(
                app.hex_view
                    .offset
                    .saturating_sub(app.reader.page_current_size),
            );
        }
        // paste (Shift+V), replacing the old bare 'b'/'B'/'p'/'P'
        //
        // Matched on the uppercase character rather than on the SHIFT flag, since
        // that is what a terminal reports for Shift+letter either way. Ctrl and Alt
        // are excluded so nothing else in those ranges is swallowed.
        KeyCode::Char('V')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            paste_hex_bytes(app);
        }
        // copy the selection (Ctrl+C), replacing the old vi-style 'y' yank. Edit
        // mode has had this key all along; Normal mode used to differ.
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            copy_selection_to_clipboard(app);
        }

        // go to last visited offset
        KeyCode::Backspace if !key.modifiers.contains(KeyModifiers::ALT) => {
            app.goto(app.hex_view.last_visited_offset);
        }
        // Edit Data dialog (Ctrl+E, as in x64dbg's Binary -> Edit). Was a bare 'd',
        // which is vi's delete operator.
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoEditData);
            } else {
                app.state = UIState::DialogEditData;
                app.hex_view.edit_dialog.reset();
                // First field follows the primary encoding, so it matches the
                // text column shown in the hex view.
                let enc1 = app.text_view.table;
                app.hex_view.edit_dialog.set_enc1(enc1);
                app.dialog_renderer = Some(hex::edit_dialog::dialog_edit_draw);
            }
        }
        // The next/previous differing byte ('o'/'O') lost its binding along with the
        // other bare letters; `goto_other_byte` is kept for a future menu entry.

        // change case
        //
        // The read-only check used to sit in this guard, so the key fell through to
        // `_ => {}` and did nothing without saying why. It is inside the arm now.
        KeyCode::Char('~') if app.hex_view.offset < app.file_info.size => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoCase);
                return Ok(false);
            }
            // With a selection, flip the case of every byte in it; otherwise
            // just the byte under the cursor (and advance, as before).
            if app.hex_view.selection.start != app.hex_view.selection.end {
                let mut count = 0;
                for ofs in app.hex_view.selection {
                    if let Some(b) = app.read_u8(ofs) {
                        let flipped = if b.is_ascii_lowercase() {
                            b.to_ascii_uppercase()
                        } else if b.is_ascii_uppercase() {
                            b.to_ascii_lowercase()
                        } else {
                            continue;
                        };
                        crate::hex::edit::record_edit(app, ofs, flipped);
                        count += 1;
                    }
                }
                crate::app::App::log(app, format!("Toggled case of {} byte(s) in selection", count));
                app.hex_view.selection.clear();
                app.hex_view.shift_anchor = None;
            } else if let Some(b) = app.read_u8(app.hex_view.offset) {
                if b.is_ascii_lowercase() {
                    hex::edit::fill_with(app, b.to_ascii_uppercase(), true);
                } else if b.is_ascii_uppercase() {
                    hex::edit::fill_with(app, b.to_ascii_lowercase(), true);
                } else {
                    beep!();
                }
            }
        }

        // F1 (help) is handled globally now, so it works in every view.
        // replace / edit mode (F2)
        //
        // Guarded on ALT: Alt+F2 is the Offset <-> VA toggle in `global/events.rs`,
        // and a global handler that leaves the state alone falls through to the view
        // - so an unguarded arm here toggled the address column *and* dropped into
        // edit mode.
        KeyCode::F(2) if !key.modifiers.contains(KeyModifiers::ALT) => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoEditMode);
            } else if app.hex_view.offset < app.file_info.size {
                app.state = UIState::HexEditing;
            }
        }
        // strings list (F6; moved off plain 's' to avoid colliding with the
        // planned Ctrl+S shortcut it used to sit next to). Alt+F6 is the image-base
        // dialog, handled globally.
        KeyCode::F(6) if !key.modifiers.contains(KeyModifiers::ALT) => {
            Commands::strings(app);
        }
        // names dialog (Alt+N). Plain 'n'/'N' used to open/repeat the '/' hex
        // search, which has moved to Ctrl+F (open) and F3/Shift+F3 (repeat).
        KeyCode::Char('n') | KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.state = UIState::DialogNames;
            app.dialog_renderer = Some(hex::names::dialog_names_draw);
            if app.hex_view.names_list_state.selected().is_none() {
                app.hex_view.names_list_state.select_first();
            }
        }
        // ';' (comment) is handled in `global/events.rs` so it also works in the
        // Disasm view, where the comment column is the whole point of having one.
        // clear selection on Esc key
        KeyCode::Esc => {
            app.hex_view.selection.clear();
            app.hex_view.shift_anchor = None;
        }
        // Selection mode used to be entered with 'v' (vi's visual mode). Shift with
        // any movement key is the only way in now - one mental model instead of two.
        // `UIState::HexSelection` still exists because a mouse drag uses it.

        // Fill with 0x00 (Insert) / 0x90 NOPs (Delete). With a Shift-selection
        // active these cover the whole range; with none they act on the byte
        // under the cursor, which is a useful shorthand in its own right.
        KeyCode::Insert | KeyCode::Delete => {
            if app.file_info.is_read_only {
                let what = if key.code == KeyCode::Insert {
                    crate::i18n::M::RoFillZero
                } else {
                    crate::i18n::M::RoFillNop
                };
                app.read_only_error(what);
                return Ok(false);
            }
            let value = if key.code == KeyCode::Insert { 0x00 } else { 0x90 };
            let has_selection = app.hex_view.selection.start != app.hex_view.selection.end;
            if has_selection {
                hex::selection::fill_selection_with(app, value);
                app.hex_view.selection.clear();
                app.hex_view.shift_anchor = None;
            } else {
                hex::edit::fill_with(app, value, false);
                crate::app::App::log(
                    app,
                    format!("Filled byte at 0x{:X} with 0x{:02X}", app.hex_view.offset, value),
                );
            }
        }
        // Reverting a selection ('u') and yanking it ('y') were the last two vi
        // verbs here. Ctrl+Z already reverts a selection when one is active - see
        // `perform_undo` - and Ctrl+C above copies the block.

        // undo / revert selected block or single step (Ctrl+Z or Alt+Backspace)
        KeyCode::Char('z') | KeyCode::Char('Z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            perform_undo(app);
        }
        // redo last undone change (Ctrl+Y)
        KeyCode::Char('y') | KeyCode::Char('Y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            perform_redo(app);
        }
        // revert just the byte under the cursor (Alt+F3)
        KeyCode::F(3) if key.modifiers.contains(KeyModifiers::ALT) => {
            revert_byte_at_cursor(app);
        }
        // The Offset <-> VA toggle moved off plain 'z' to Alt+F2, and lives in
        // `global/events.rs` so it also works from the Disasm view.
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => {
            perform_undo(app);
        }
        // Xrefs are Ctrl+R and the Modify Block dialog is Ctrl+K, both handled in
        // `global/events.rs` so they work from the Disasm view too. The bare 'r' and
        // 'm' they replaced are gone.

        // Colour the selected range, or recolour the block under the cursor.
        KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::ALT) => {
            hex::selection::color_block_at_cursor(app);
            app.hex_view.selection.clear();
            app.hex_view.shift_anchor = None;
        }
        // Mark the block: '[' sets its start, ']' sets its end, both at the cursor.
        //
        // This is the two-key way of marking a range that Hiew and Total Commander
        // use, and it is the only way to mark one without holding a key down: with
        // Shift+arrows a long block means holding Shift across pages, and the mouse
        // cannot reach past the screen. The keys used to *jump* to the ends of an
        // existing block; that moved to Alt+[ and Alt+].
        KeyCode::Char('[') if !key.modifiers.contains(KeyModifiers::ALT) => {
            let ofs = app.hex_view.offset;
            app.hex_view.selection.start = ofs;
            // A start past the current end would be an inverted range, which the
            // fill and copy paths read as empty.
            if app.hex_view.selection.end < ofs {
                app.hex_view.selection.end = ofs;
            }
            app.hex_view.selection.direction = None;
            app.hex_view.selection.is_mouse = false;
            app.hex_view.selection_target = app.hex_view.editing_target;
            app.hex_view.shift_anchor = None;
            let len = app.hex_view.selection.end - app.hex_view.selection.start;
            crate::app::App::log(app, format!("Block start at 0x{:X} ({} byte(s))", ofs, len + 1));
        }
        KeyCode::Char(']') if !key.modifiers.contains(KeyModifiers::ALT) => {
            let ofs = app.hex_view.offset;
            app.hex_view.selection.end = ofs;
            if app.hex_view.selection.start > ofs {
                app.hex_view.selection.start = ofs;
            }
            app.hex_view.selection.direction = None;
            app.hex_view.selection.is_mouse = false;
            app.hex_view.selection_target = app.hex_view.editing_target;
            app.hex_view.shift_anchor = None;
            let len = app.hex_view.selection.end - app.hex_view.selection.start;
            crate::app::App::log(app, format!("Block end at 0x{:X} ({} byte(s))", ofs, len + 1));
        }
        // Alt+[ / Alt+]: jump to the ends of the marked block, or to the nearest
        // coloured block boundary when nothing is marked.
        KeyCode::Char('[') if key.modifiers.contains(KeyModifiers::ALT) => {
            if app.hex_view.selection.start != app.hex_view.selection.end {
                app.goto(app.hex_view.selection.start);
            } else {
                for b in app.hex_view.blocks.iter().rev() {
                    if b.end < app.hex_view.offset {
                        app.goto(b.end);
                        break;
                    } else if b.start < app.hex_view.offset {
                        app.goto(b.start);
                        break;
                    }
                }
            }
        }
        KeyCode::Char(']') if key.modifiers.contains(KeyModifiers::ALT) => {
            if app.hex_view.selection.start != app.hex_view.selection.end {
                app.goto(app.hex_view.selection.end);
            } else {
                for b in &app.hex_view.blocks {
                    if b.start > app.hex_view.offset {
                        app.goto(b.start);
                        break;
                    } else if b.end > app.hex_view.offset {
                        app.goto(b.end);
                        break;
                    }
                }
            }
        }
        // encoding dialogs: Alt+E for the primary one, Alt+Shift+E for the second.
        //
        // They were bare 'e'/'E', which is the one letter a hex editor cannot
        // afford to spend: it is also a hex digit. Alt+E keeps them next to the
        // other display settings (Alt+F2 address mode, Alt+F7 decoding width)
        // instead of in the Ctrl range, which is editing and clipboard.
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
mod paste_encoding_tests {
    use super::parse_hex_or_text_bytes;

    /// Hex on the clipboard is still taken as bytes, whatever the column.
    #[test]
    fn hex_text_is_still_read_as_bytes() {
        for enc in [encoding_rs::UTF_8, encoding_rs::EUC_KR] {
            assert_eq!(parse_hex_or_text_bytes("B0 A1 48 69", enc), vec![0xB0, 0xA1, 0x48, 0x69]);
            assert_eq!(parse_hex_or_text_bytes("0x41,0x42", enc), vec![0x41, 0x42]);
        }
    }

    /// Text is encoded through the column, not through UTF-8.
    ///
    /// Copying `가` out of a CP949 column and pasting it back used to write the UTF-8
    /// `EA B0 80` where the file held `B0 A1`: three bytes instead of two, none of
    /// them the same. Typing that character already used the column's encoding, so
    /// paste was the one path that disagreed.
    #[test]
    fn text_is_encoded_through_the_column() {
        assert_eq!(
            parse_hex_or_text_bytes("가", encoding_rs::EUC_KR),
            vec![0xB0, 0xA1],
            "a CP949 column must round-trip its own two bytes"
        );
        assert_eq!(
            parse_hex_or_text_bytes("가", encoding_rs::UTF_8),
            vec![0xEA, 0xB0, 0x80],
            "a UTF-8 column encodes the same character in three"
        );
        assert_ne!(
            parse_hex_or_text_bytes("가", encoding_rs::EUC_KR),
            parse_hex_or_text_bytes("가", encoding_rs::UTF_8),
            "if these matched, the column would be being ignored"
        );
    }

    /// ASCII is the same in every encoding here, so a plain paste is unaffected.
    #[test]
    fn ascii_is_unchanged() {
        for enc in [encoding_rs::UTF_8, encoding_rs::EUC_KR] {
            // "Hi!" is not valid hex, so it takes the text path.
            assert_eq!(parse_hex_or_text_bytes("Hi!", enc), b"Hi!".to_vec());
        }
    }

    #[test]
    fn empty_input_pastes_nothing() {
        assert!(parse_hex_or_text_bytes("   ", encoding_rs::UTF_8).is_empty());
    }
}

#[cfg(test)]
mod alt_m_tests {
    use crate::app::App;
    use crate::editor::UIState;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dezes_altm");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("b_{}_{}.bin", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0x41u8; 0x400]).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::hex::events::hex_mode_events(app, key);
    }

    fn alt_m(app: &mut App) {
        press(app, KeyCode::Char('m'), KeyModifiers::ALT);
    }

    /// A Shift+arrow selection must be colourable.
    ///
    /// This is the half of Alt+M that went missing: creating a block lived in the
    /// old `v` selection mode, and Shift-selection leaves the state as Normal, so
    /// Normal-mode Alt+M could only ever recolour a block that already existed -
    /// and nothing could create the first one.
    #[test]
    fn alt_m_colours_a_shift_selection() {
        let mut app = app_with_file();
        assert!(app.state == UIState::Normal);
        assert!(app.hex_view.blocks.is_empty());

        app.hex_view.offset = 0x10;
        for _ in 0..4 {
            press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        }
        assert_ne!(
            app.hex_view.selection.start, app.hex_view.selection.end,
            "Shift+Right did not select anything"
        );

        alt_m(&mut app);

        assert_eq!(app.hex_view.blocks.len(), 1, "Alt+M did not create a block");
        assert_eq!(app.hex_view.blocks[0].start, 0x10);
        assert_eq!(app.hex_view.blocks[0].end, 0x14);
    }

    /// Pressing it again on that block recolours it rather than stacking another.
    #[test]
    fn alt_m_recolours_instead_of_stacking() {
        let mut app = app_with_file();
        app.hex_view.offset = 0x20;
        for _ in 0..3 {
            press(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        }
        alt_m(&mut app);
        assert_eq!(app.hex_view.blocks.len(), 1);

        // The cursor is inside the block now, with no selection.
        app.hex_view.offset = 0x21;
        let before = app.hex_view.blocks[0].bg_color;
        let mut recoloured = false;
        for _ in 0..20 {
            alt_m(&mut app);
            assert_eq!(app.hex_view.blocks.len(), 1, "a second block was stacked on top");
            if app.hex_view.blocks[0].bg_color != before {
                recoloured = true;
            }
        }
        assert!(recoloured, "the colour never changed");
    }

    /// With nothing selected and no block at the cursor, it says so.
    #[test]
    fn alt_m_reports_when_there_is_nothing_to_colour() {
        let mut app = app_with_file();
        app.hex_view.offset = 0x30;

        alt_m(&mut app);

        assert!(app.hex_view.blocks.is_empty());
        assert!(
            app.status_error.is_some(),
            "doing nothing silently is what made this look broken"
        );
    }
}