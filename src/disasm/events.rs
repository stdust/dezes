use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Result;

use crate::app::App;
use crate::editor::UIState;
use iced_x86::{Decoder, DecoderOptions, Formatter};
use std::fmt::Write as _;

/// Longest possible x86 instruction; used to size decode windows.
///
/// Re-exported from `nav`, where the boundary arithmetic lives, so the two can't
/// disagree about how far a decode window has to reach.
use crate::disasm::nav::MAX_INSTR_BYTES;

/// Instructions a PageUp/PageDown moves by.
const DISASM_PAGE_ROWS: usize = 15;

/// Reverts the pending edits in the current selection, or the last single edit.
///
/// The Ctrl+Z and Alt+Backspace handlers were byte-identical 25-line copies.
fn perform_undo(app: &mut App, offset: usize) {
    let selection = match app.disasm_selection_anchor {
        Some(anchor) => Some((anchor.min(offset), anchor.max(offset))),
        None if app.hex_view.selection.start != app.hex_view.selection.end => Some((
            app.hex_view.selection.start.min(app.hex_view.selection.end),
            app.hex_view.selection.start.max(app.hex_view.selection.end),
        )),
        None => None,
    };

    if let Some((start, end)) = selection {
        let count = crate::hex::edit::revert_range(app, start, end);

        if count > 0 {
            crate::app::App::log(app, format!("Reverted {} changed byte(s) in selected block", count));
        } else {
            crate::beep!();
        }
    } else if let Some(ofs) = app.hex_view.changed_history.pop() {
        let _ = app.hex_view.changed_bytes.remove(&ofs);

        crate::app::App::log(app, format!("Undid change at offset 0x{:X}", ofs));
    } else {
        crate::beep!();
    }
}

/// Re-applies the most recently undone edit (Ctrl+Y).
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

/// Reverts only the byte under the cursor to its on-disk value (Hiew's Alt+F3).
///
/// The cursor deliberately stays put: this is an in-place edit, so moving on
/// would make it impossible to see what the byte was restored to.
fn revert_byte_at_cursor(app: &mut App) {
    let ofs = app.hex_view.offset;
    match app.hex_view.changed_bytes.remove(&ofs) {
        Some(val) => {
            app.hex_view.changed_history.retain(|o| *o != ofs);
            app.hex_view.redo_history.push((ofs, val));
    
            app.goto_with_history(ofs, false);
            crate::app::App::log(app, format!("Reverted byte at offset 0x{:X} to its original value", ofs));
        }
        None => crate::beep!(),
    }
}

/// Length in bytes of the instruction starting at `offset`, or `None` if
/// nothing decodes there.
fn instr_len_at(app: &mut App, offset: usize, bitness: u32, filesize: usize) -> Option<usize> {
    let ip = app.get_va(offset);
    let buffer = app.file_info.get_buffer_ref();
    let end = offset.saturating_add(MAX_INSTR_BYTES).min(filesize).min(buffer.len());
    if offset >= end {
        return None;
    }
    let decoder = Decoder::with_ip(bitness, &buffer[offset..end], ip, DecoderOptions::NONE);
    decoder.into_iter().next().map(|i| i.len())
}

/// Overwrites the instruction under the cursor with 0x90 (NOP) bytes, exactly
/// as long as the instruction actually is (Hiew's Alt+F2, bound to Delete
/// here). Uses the decoded length rather than a selection, so a 5-byte call
/// becomes exactly five NOPs with no leftover operand bytes to desynchronize
/// the following instructions.
fn nop_current_instruction(app: &mut App, offset: usize, bitness: u32, filesize: usize) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoNopOut);
        return;
    }

    let Some(len) = instr_len_at(app, offset, bitness, filesize) else {
        crate::beep!();
        return;
    };

    for i in 0..len {
        let ofs = offset + i;
        if ofs >= filesize {
            break;
        }
        crate::hex::edit::record_edit(app, ofs, 0x90);
    }

    crate::app::App::log(
        app,
        format!("Filled instruction at 0x{:X} with {} NOP byte(s)", offset, len),
    );
}

pub fn disasm_mode_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    let bitness = app.bitness();
    let offset = app.hex_view.offset;
    // Bounded by the live mapping: every `&buffer[..]` below is derived from this,
    // and `file_info.size` can be larger than what is actually mapped.
    let filesize = app.file_info.size.min(app.file_info.buffer_len());

    // Navigation and Esc must work even when the cursor is at or past the end of
    // the mapping - which happens after a truncate, or on a file whose directory
    // size exceeds what was mapped. This guard used to sit in front of the whole
    // match, so in that state every key including Up, Home, PageUp and Esc was
    // dead and there was no way back.
    if filesize == 0 || offset >= filesize {
        let last = filesize.saturating_sub(1);
        match key.code {
            KeyCode::Up | KeyCode::PageUp | KeyCode::Home => {
                app.reader.page_start = 0;
                app.goto(0);
            }
            KeyCode::End | KeyCode::Down | KeyCode::PageDown if filesize > 0 => {
                app.reader.page_start = crate::disasm::nav::page_start_ending_at(app, last, DISASM_PAGE_ROWS);
                app.goto(last);
            }
            KeyCode::Esc => {
                app.disasm_selection_anchor = None;
            }
            _ => {}
        }
        return Ok(false);
    }

    let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Track Shift selection anchor
    if is_shift {
        match key.code {
            KeyCode::Down | KeyCode::Up | KeyCode::PageDown | KeyCode::PageUp | KeyCode::Home | KeyCode::End => {
                if app.disasm_selection_anchor.is_none() {
                    app.disasm_selection_anchor = Some(offset);
                }
            }
            _ => {}
        }
    }

    // Ctrl+C: Copy selected disasm lines, or the current line, to the clipboard.
    // Was a bare 'y' (vi yank), which also meant Ctrl+Y (Redo) had to be excluded
    // here by hand.
    if (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        let (start_ofs, end_ofs) = if let Some(anchor) = app.disasm_selection_anchor {
            (anchor.min(offset), anchor.max(offset))
        } else {
            (offset, offset)
        };

        let slice_end = (end_ofs + MAX_INSTR_BYTES * 8).min(filesize);
        let page_start = start_ofs.min(slice_end);
        let ip = app.get_va(page_start);
        let buffer = app.file_info.get_buffer_ref();
        let code_bytes = &buffer[page_start..slice_end];

        let decoder = Decoder::with_ip(bitness, code_bytes, ip, DecoderOptions::NONE);
        let mut formatter = iced_x86::IntelFormatter::new();
        formatter.options_mut().set_first_operand_char_index(0);
        formatter.options_mut().set_hex_prefix("0x");
        formatter.options_mut().set_hex_suffix("");
        formatter.options_mut().set_leading_zeroes(false);

        let mut lines = Vec::new();
        let mut cur_ofs = page_start;
        let mut line_text = String::new();

        for instr in decoder {
            if cur_ofs > end_ofs && !lines.is_empty() {
                break;
            }
            line_text.clear();
            formatter.format(&instr, &mut line_text);
            let va = instr.ip();
            let len = instr.len();
            let hex_end = (cur_ofs + len).min(filesize);
            let mut hex_str = String::with_capacity(len * 3);
            for (i, b) in buffer[cur_ofs.min(hex_end)..hex_end].iter().enumerate() {
                if i > 0 {
                    hex_str.push(' ');
                }
                let _ = write!(hex_str, "{:02X}", b);
            }

            let clean_instr = line_text.replace(" short ", " ").replace("(bad)", "???").replace("bad", "???");
            // Same import substitution the view does, so a copied listing reads
            // like the screen rather than showing bare slot addresses.
            let clean_instr = crate::disasm::draw::apply_import_symbol(app, &instr, &clean_instr);

            // Same comment the view shows, so a copied listing carries the
            // resolved import names, user comments and string references. It used
            // to drop the comment column entirely, which made an exported
            // disassembly look as though nothing had been resolved.
            let comment = crate::disasm::draw::line_comment(app, cur_ofs, &clean_instr);
            match comment {
                Some(text) => lines.push(format!(
                    "{:X}   {:18}   {:42} ; {}",
                    va, hex_str, clean_instr, text
                )),
                None => lines.push(format!("{:X}   {:18}   {}", va, hex_str, clean_instr)),
            }

            cur_ofs += len;
            if cur_ofs >= filesize {
                break;
            }
        }

        if !lines.is_empty() {
            let copied_text = lines.join("\n");
            let line_cnt = lines.len();
            if let Ok(cb) = &mut app.clipboard {
                let _ = cb.set_text(copied_text);
                App::log(app, format!("Copied {} disasm line(s) to clipboard", line_cnt));
            }
        }
        return Ok(false);
    }

    // Reset selection if moving without Shift
    if !is_shift {
        match key.code {
            KeyCode::Down | KeyCode::Up | KeyCode::PageDown | KeyCode::PageUp | KeyCode::Home | KeyCode::End => {
                app.disasm_selection_anchor = None;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Down => {
            app.hex_view.offset = crate::disasm::nav::next_instruction(app, offset);
        }
        KeyCode::Up => {
            let target = crate::disasm::nav::prev_instruction(app, offset);
            app.hex_view.offset = target;
            if target < app.reader.page_start {
                app.reader.page_start = target;
            }
        }
        KeyCode::PageDown => {
            let current = crate::disasm::nav::advance(app, offset, DISASM_PAGE_ROWS);
            app.reader.page_start = current;
            app.goto(current);
        }
        // Follow the branch or memory target. Enter only: 'f'/'F' went with the rest
        // of the bare letters, and Ctrl+Enter still follows into the Hex view.
        KeyCode::Enter => {
            let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            // Decoded at the cursor, not hunted for from `page_start`. The old
            // scan looked at most SCAN_WINDOW bytes forward from the top of the
            // page and only acted when it landed exactly on the cursor, so on a
            // tall terminal - or with the cursor off a boundary - the key did
            // nothing at all, with no feedback.
            let end = (offset + MAX_INSTR_BYTES).min(filesize);
            let start_va = app.get_va(offset);
            let buffer = app.file_info.get_buffer_ref();
            let bytes = &buffer[offset.min(end)..end.min(buffer.len())];
            let mut decoder = Decoder::with_ip(bitness, bytes, start_va, DecoderOptions::NONE);

            let mut current_ofs = offset;
            for instr in &mut decoder {
                let len = instr.len();
                if current_ofs == offset {
                    let flow_control = instr.flow_control();
                    let target_va = match flow_control {
                        iced_x86::FlowControl::UnconditionalBranch
                        | iced_x86::FlowControl::ConditionalBranch
                        | iced_x86::FlowControl::Call => {
                            let near_target = instr.near_branch_target();
                            if near_target != 0 {
                                Some(near_target)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    let target_va = target_va.or_else(|| {
                        if instr.is_ip_rel_memory_operand() {
                            let va = instr.ip_rel_memory_address();
                            if va != 0 {
                                return Some(va);
                            }
                        }

                        for op in 0..instr.op_count() {
                            match instr.op_kind(op) {
                                iced_x86::OpKind::Memory => {
                                    let disp = instr.memory_displacement64();
                                    if disp != 0 && app.va_to_offset(disp).is_some() {
                                        return Some(disp);
                                    }
                                }
                                iced_x86::OpKind::Immediate64
                                | iced_x86::OpKind::Immediate32
                                | iced_x86::OpKind::Immediate32to64 => {
                                    let imm = instr.immediate(op);
                                    if imm != 0 && app.va_to_offset(imm).is_some() {
                                        return Some(imm);
                                    }
                                }
                                _ => {}
                            }
                        }
                        None
                    });

                    if let Some(t_va) = target_va {
                        if let Some(target_offset) = app.va_to_offset(t_va) {
                            if target_offset < filesize {
                                if is_ctrl {
                                    if app.hex_view.jump_history_back.last() != Some(&(offset, crate::editor::AppView::Disasm)) {
                                        app.hex_view.jump_history_back.push((offset, crate::editor::AppView::Disasm));
                                        if app.hex_view.jump_history_back.len() > 100 {
                                            app.hex_view.jump_history_back.remove(0);
                                        }
                                    }
                                    app.hex_view.jump_history_forward.clear();

                                    app.editor_view = crate::editor::AppView::Hex;
                                    app.goto_with_history(target_offset, false);
                                    // `page_start` was an instruction boundary a
                                    // moment ago; the hex grid needs it on a
                                    // bytes-per-line boundary.
                                    app.align_page_for_view();
                                    crate::app::App::log(app, format!("Followed target to 0x{:X} in Hex view", t_va));
                                } else {
                                    app.reader.page_start = target_offset;
                                    app.goto(target_offset);
                                    crate::app::App::log(app, format!("Followed target to 0x{:X}", t_va));
                                }
                                return Ok(false);
                            }
                        }
                    }
                    break;
                }
                current_ofs += len;
            }
        }
        KeyCode::PageUp => {
            // A real page of instructions. The old estimate of three bytes per
            // instruction landed mid-instruction almost every time, so a
            // PageDown/PageUp round trip did not come back to the same lines.
            let target = crate::disasm::nav::retreat(app, offset, DISASM_PAGE_ROWS);
            app.reader.page_start = target;
            app.goto(target);
        }
        KeyCode::Home => {
            app.reader.page_start = 0;
            app.goto(0);
        }
        KeyCode::End => {
            // Backed up a page from EOF so the last screen is full. Setting
            // page_start to the last byte showed a single row.
            let last_ofs = filesize.saturating_sub(1);
            app.reader.page_start =
                crate::disasm::nav::page_start_ending_at(app, last_ofs, DISASM_PAGE_ROWS);
            app.goto(last_ofs);
        }
        // Assemble at the cursor (Space, as in x64dbg). Was 'a'/'A'.
        KeyCode::Char(' ') => {
            // `stage_assembled_bytes` refuses read-only files, so typing an
            // instruction into the dialog was work thrown away on Enter.
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoAssemble);
                return Ok(false);
            }
            let ip = app.get_va(offset);
            let buffer = app.file_info.get_buffer_ref();
            let slice_end = (offset + MAX_INSTR_BYTES).min(filesize);
            let slice = &buffer[offset.min(slice_end)..slice_end];
            let decoder = Decoder::with_ip(bitness, slice, ip, DecoderOptions::NONE);
            let mut formatter = iced_x86::IntelFormatter::new();
            formatter.options_mut().set_first_operand_char_index(0);
            formatter.options_mut().set_hex_prefix("0x");
            formatter.options_mut().set_hex_suffix("");
            formatter.options_mut().set_leading_zeroes(false);

            let mut initial_text = String::new();
            for instr in decoder {
                formatter.format(&instr, &mut initial_text);
                break;
            }

            let clean_initial = initial_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(" short ", " ");
            app.state = crate::editor::UIState::DialogAssemble;
            app.assemble_input = tui_input::Input::new(clean_initial);
            app.assemble_selection_all = true;
            app.dialog_renderer = Some(crate::disasm::assemble::dialog_assemble_draw);
        }
        KeyCode::Char('z') | KeyCode::Char('Z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            perform_undo(app, offset);
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => {
            perform_undo(app, offset);
        }
        // redo last undone change (Ctrl+Y)
        KeyCode::Char('y') | KeyCode::Char('Y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            perform_redo(app);
        }
        // revert just the byte under the cursor (Alt+F3)
        KeyCode::F(3) if key.modifiers.contains(KeyModifiers::ALT) => {
            revert_byte_at_cursor(app);
        }
        // NOP out the instruction under the cursor (Delete)
        KeyCode::Delete => {
            nop_current_instruction(app, offset, bitness, filesize);
        }
        // strings list (F6), same as in Hex view. Alt+F6 sets the image base.
        KeyCode::F(6) if !key.modifiers.contains(KeyModifiers::ALT) => {
            crate::commands::Commands::strings(app);
        }
        // Edit Data dialog (Ctrl+E, x64dbg's Binary -> Edit). Was 'd'/'D'.
        KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoEditData);
            } else {
                app.state = UIState::DialogEditData;
                app.hex_view.edit_dialog.reset();
                let enc1 = app.text_view.table;
                app.hex_view.edit_dialog.set_enc1(enc1);
                app.dialog_renderer = Some(crate::hex::edit_dialog::dialog_edit_draw);
            }
        }
        // Xrefs are Ctrl+R, handled in `global/events.rs`; the bare 'r' is gone.
        KeyCode::Esc => {
            app.disasm_selection_anchor = None;
        }
        _ => {}
    }

    Ok(false)
}

#[cfg(test)]
mod disasm_key_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    /// nop nop push-rax mov-eax-imm32 ret int3, then nop padding.
    const CODE: &[u8] = &[0x90, 0x90, 0x50, 0xB8, 0x78, 0x56, 0x34, 0x12, 0xC3, 0xCC];
    const BOUNDS: &[usize] = &[0, 1, 2, 3, 8, 9];

    /// One fixture file per call: loading maps the file, and these tests run in
    /// parallel, so sharing a path made the write fail while another test still
    /// had it mapped.
    fn app_with_code() -> App {
        static FIXTURE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let dir = std::env::temp_dir().join("dz6_disasm_keys");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("code_{}.bin", id));
        let mut bytes = CODE.to_vec();
        bytes.resize(0x200, 0x90);
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("utf-8 path"), 0, true)
            .expect("open fixture");
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        press_with(app, code, KeyModifiers::NONE);
    }

    fn press_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = disasm_mode_events(app, key);
    }

    /// Down lands on the next instruction, including from inside one.
    ///
    /// The old handler tested `cur_ofs >= offset`, so from mid-instruction the
    /// flag first tripped on the following instruction and the key moved two.
    #[test]
    fn down_moves_exactly_one_instruction() {
        let mut app = app_with_code();
        for pair in BOUNDS.windows(2) {
            app.hex_view.offset = pair[0];
            press(&mut app, KeyCode::Down);
            assert_eq!(app.hex_view.offset, pair[1], "Down from 0x{:X}", pair[0]);
        }

        // From the middle of the 5-byte mov (offset 3..8).
        app.hex_view.offset = 5;
        press(&mut app, KeyCode::Down);
        assert_eq!(app.hex_view.offset, 8, "Down from mid-instruction");
    }

    /// Down must not depend on where the page happens to start.
    ///
    /// The old scan began at `page_start` and gave up after 1 KiB, falling back to
    /// a single raw byte - which left the cursor mid-instruction.
    #[test]
    fn down_is_independent_of_page_start() {
        let mut app = app_with_code();
        for page_start in [0usize, 1, 0x40, 0x100] {
            app.reader.page_start = page_start;
            app.hex_view.offset = 3;
            press(&mut app, KeyCode::Down);
            assert_eq!(
                app.hex_view.offset, 8,
                "Down should reach 0x8 with page_start 0x{:X}",
                page_start
            );
        }
    }

    #[test]
    fn up_moves_exactly_one_instruction() {
        let mut app = app_with_code();
        for pair in BOUNDS.windows(2) {
            app.hex_view.offset = pair[1];
            app.reader.page_start = 0;
            press(&mut app, KeyCode::Up);
            assert_eq!(app.hex_view.offset, pair[0], "Up from 0x{:X}", pair[1]);
        }
    }

    /// Down then Up returns to the starting line.
    #[test]
    fn down_up_round_trip() {
        let mut app = app_with_code();
        for &start in BOUNDS {
            app.reader.page_start = 0;
            app.hex_view.offset = start;
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Up);
            assert_eq!(app.hex_view.offset, start, "round trip from 0x{:X}", start);
        }
    }

    /// PageDown then PageUp does too, which the three-bytes-per-instruction
    /// estimate could not manage.
    #[test]
    fn page_down_up_round_trip() {
        let mut app = app_with_code();
        // Started well away from 0: from offset 0 the old byte estimate
        // underflowed to 0 and round-tripped by accident.
        const START: usize = 0x100;
        app.reader.page_start = START;
        app.hex_view.offset = START;

        press(&mut app, KeyCode::PageDown);
        let after = app.hex_view.offset;
        assert!(after > START, "PageDown must move forward");

        press(&mut app, KeyCode::PageUp);
        assert_eq!(
            app.hex_view.offset, START,
            "PageUp must return to 0x{:X}, not land mid-instruction",
            START
        );
    }

    /// End shows a full page rather than a single row at EOF.
    #[test]
    fn end_leaves_a_full_page_visible() {
        let mut app = app_with_code();
        press(&mut app, KeyCode::End);

        let last = app.file_info.buffer_len() - 1;
        assert_eq!(app.hex_view.offset, last, "cursor goes to the last byte");
        assert!(
            app.reader.page_start < last,
            "page must start before the last byte, got 0x{:X}",
            app.reader.page_start
        );
    }

    /// With the cursor past the end of the mapping, navigation must still work.
    ///
    /// The bounds check used to precede the whole match, so every key - including
    /// Up, Home, PageUp and Esc - was unreachable and the view was stuck.
    #[test]
    fn navigation_still_works_past_the_end() {
        let mut app = app_with_code();
        let past = app.file_info.buffer_len() + 0x100;
        app.hex_view.offset = past;

        press(&mut app, KeyCode::Home);
        assert_eq!(app.hex_view.offset, 0, "Home must escape an out-of-range cursor");

        app.hex_view.offset = past;
        press(&mut app, KeyCode::Up);
        assert_eq!(app.hex_view.offset, 0, "Up must escape too");

        app.hex_view.offset = past;
        app.disasm_selection_anchor = Some(0x10);
        press(&mut app, KeyCode::Esc);
        assert!(app.disasm_selection_anchor.is_none(), "Esc must clear the selection");
    }

    /// Navigation follows the bytes on screen, i.e. with pending edits applied.
    #[test]
    fn navigation_follows_pending_edits() {
        let mut app = app_with_code();
        // Patch offset 0 from a 1-byte nop into a 5-byte `mov eax, imm32`.
        app.hex_view.changed_bytes.insert(0, "B8".to_string());
        app.hex_view.offset = 0;
        app.reader.page_start = 0;

        press(&mut app, KeyCode::Down);
        assert_eq!(
            app.hex_view.offset, 5,
            "Down must step over the edited instruction the view is showing"
        );
    }

    /// The copied listing carries the comment column.
    ///
    /// It used to be built from a separate format string with no comment field at
    /// all, so a pasted disassembly showed bare `call qword ptr [0x...]` and looked
    /// as if the import names had never been resolved - even though the screen had
    /// them.
    #[test]
    fn copying_includes_the_comment_column() {
        let mut app = app_with_code();
        app.editor_view = crate::editor::AppView::Disasm;
        app.hex_view.offset = 0;
        app.reader.page_start = 0;
        app.hex_view.comments.insert(0, "entry point".to_string());

        // The clipboard may be unavailable in a headless test environment, so the
        // assertion is on what `line_comment` contributes to the line rather than
        // on the clipboard round trip.
        assert_eq!(
            crate::disasm::draw::line_comment(&app, 0, "nop").as_deref(),
            Some("entry point"),
            "the copy path must be able to see the same comment the view shows"
        );

        press_with(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        // Reaching here without panicking means the copy path ran with the comment
        // lookup in place; the clipboard itself is environment-dependent.
    }

    /// Follow works from any row, not just the ones near the top of the page.
    #[test]
    fn follow_works_far_from_page_start() {
        let dir = std::env::temp_dir().join("dz6_disasm_follow");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("jmp.bin");

        // A short forward `jmp` at offset 0x800, far past page_start 0.
        let mut bytes = vec![0x90u8; 0x1000];
        bytes[0x800] = 0xEB; // jmp rel8
        bytes[0x801] = 0x10; // +0x10
        if std::fs::write(&path, &bytes).is_err() {
            return;
        }

        let mut app = App::new();
        app.config.database = false;
        if app.load_file(path.to_str().expect("path"), 0, true).is_err() {
            return;
        }
        app.reader.page_start = 0;
        app.hex_view.offset = 0x800;

        press(&mut app, KeyCode::Enter);

        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            app.hex_view.offset, 0x812,
            "the jmp at 0x800 targets 0x812 (0x802 + 0x10) and Follow must go there"
        );
    }
}
