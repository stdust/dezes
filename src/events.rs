use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use std::io::Result;

use crate::app::App;
use crate::editor::{AppView, UIState};
use crate::global;
use crate::hex;
use crate::text;
use crate::{commands, header};

/// Longest possible x86 instruction, used to size decode windows.
const MAX_INSTR_BYTES: usize = 16;

/// Maps a column inside the hex dump to the byte index within its row.
///
/// The previous inline version hardcoded the 16-bytes-per-line geometry
/// (`rel_x < 23`, `rel_x == 23`, `8 + (rel_x - 24) / 3`), so clicking was wrong
/// for any other `:set byteline` value. These expressions reduce to exactly the
/// same numbers when bpl is 16.
fn hex_column_to_byte(rel_x: usize, bpl: usize) -> usize {
    let group1 = bpl.min(8);
    let group1_width = group1 * 3 - 1; // 23 when bpl >= 8
    let idx = if rel_x < group1_width {
        rel_x / 3
    } else if rel_x == group1_width {
        group1 - 1
    } else {
        group1 + (rel_x - group1_width - 1) / 3
    };
    idx.min(bpl - 1)
}

/// Byte index within a row for a mouse click at absolute column `col_idx`.
fn mouse_column_to_byte(col_idx: usize, addr_width: usize, bpl: usize) -> usize {
    let extra_sep = if bpl >= 16 { 1 } else { 0 };
    let hex_dump_width = (bpl * 3).saturating_sub(1) + extra_sep;
    let hex_start = addr_width + 1;

    if col_idx >= hex_start && col_idx < hex_start + hex_dump_width {
        hex_column_to_byte(col_idx - hex_start, bpl)
    } else if col_idx >= hex_start + hex_dump_width + 1 {
        (col_idx - (hex_start + hex_dump_width + 1)).min(bpl - 1)
    } else {
        0
    }
}

/// File offset of the instruction drawn on `target_row` of the disassembly view.
///
/// The scan window is sized from the requested row instead of the old hardcoded
/// 500 bytes, which made rows past roughly row 100 unclickable.
fn disasm_offset_at_row(
    buffer: &[u8],
    page_start: usize,
    initial_va: u64,
    bitness: u32,
    filesize: usize,
    target_row: usize,
) -> usize {
    if page_start >= buffer.len() {
        return page_start;
    }
    let scan_end = page_start
        .saturating_add((target_row + 2) * MAX_INSTR_BYTES)
        .min(filesize)
        .min(buffer.len());
    let bytes = &buffer[page_start..scan_end];
    let decoder = iced_x86::Decoder::with_ip(bitness, bytes, initial_va, iced_x86::DecoderOptions::NONE);

    let mut current_offset = page_start;
    let mut found_ofs = page_start;
    let mut row_count = 0;
    for instr in decoder {
        found_ofs = current_offset;
        if row_count == target_row {
            break;
        }
        current_offset += instr.len();
        row_count += 1;
        if current_offset >= filesize {
            break;
        }
    }
    found_ofs
}

/// Length of the first instruction at `offset`, used for wheel scrolling.
fn first_instr_len(buffer: &[u8], offset: usize, initial_va: u64, bitness: u32, filesize: usize) -> usize {
    if offset >= buffer.len() {
        return 1;
    }
    let end = offset
        .saturating_add(MAX_INSTR_BYTES)
        .min(filesize)
        .min(buffer.len());
    let bytes = &buffer[offset..end];
    let dec = iced_x86::Decoder::with_ip(bitness, bytes, initial_va, iced_x86::DecoderOptions::NONE);
    dec.into_iter().next().map(|i| i.len()).unwrap_or(1)
}

#[allow(dead_code)]
pub fn handle_dialog_error_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
        }
        _ => {}
    }
    Ok(false)
}

/// Reads and dispatches one batch of input.
///
/// Returns whether anything was actually handled, which is what the main loop uses
/// to decide whether a repaint is needed: redrawing on every poll timeout meant a
/// full-screen write 20 times a second while idle.
pub fn handle_events(app: &mut App) -> Result<bool> {
    if !event::poll(std::time::Duration::from_millis(50))? {
        return Ok(false);
    }
    let event = event::read()?;
    dispatch_event(app, event)
}

/// Routes one already-read event.
///
/// Split from the reading so a test can feed an event in: mouse geometry in
/// particular is arithmetic against the screen size, and checking it by hand is how
/// the Header view ended up with no mouse handling at all.
pub fn dispatch_event(app: &mut App, event: Event) -> Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // A refusal message stays on the command bar until the user presses
            // something else; cleared here so the handlers below are free to set a
            // new one for this very key.
            app.status_error = None;
            match app.state {
                UIState::Normal | UIState::Error => {
                    if app.editor_view == AppView::Header && key.code == KeyCode::Enter {
                        // In Header view, Enter is handled by the header handler (jump to offset)
                        header::events::header_view_events(app, key)?;
                    } else {
                        let view_before = app.editor_view;
                        let state_before = app.state;
                        global::events::handle_global_events(app, key)?;
                        // If global handler changed state (e.g. opened a dialog), don't pass event to view
                        if app.state == state_before {
                            if key.code != KeyCode::Enter || app.editor_view == view_before {
                                match app.editor_view {
                                    AppView::Hex => { hex::events::hex_mode_events(app, key)?; },
                                    AppView::Text => { text::events::text_mode_events(app, key)?; },
                                    AppView::Header => { header::events::header_view_events(app, key)?; },
                                    AppView::Disasm => { crate::disasm::events::disasm_mode_events(app, key)?; },
                                }
                            }
                        }
                    }
                }
                UIState::DialogAbout => { global::about::dialog_about_events(app, key)?; },
                UIState::DialogAssemble => { crate::disasm::assemble::dialog_assemble_events(app, &event)?; },
                UIState::DialogHelp => { hex::help::dialog_help_events(app, key)?; },
                UIState::DialogEncoding => { text::dialog_encoding::dialog_encoding_events(app, key)?; },
                UIState::DialogEncoding2 => { text::dialog_encoding::dialog_encoding2_events(app, key)?; },
                UIState::DialogEditData => { hex::edit_dialog::dialog_edit_events(app, &event)?; },
                UIState::DialogModifyBlock => { hex::modify_dialog::dialog_modify_events(app, &event)?; },
                UIState::DialogReplacePattern => { handle_replace_pattern_events(app, &event)?; },
                UIState::DialogFindPattern => { hex::find_dialog::dialog_find_events(app, &event)?; },
                UIState::DialogSectionSize => {
                    crate::header::formats::pe::section_tools::dialog_section_size_events(app, &event)?;
                },
                UIState::DialogGoto => { crate::goto_dialog::dialog_goto_events(app, &event)?; },
                UIState::DialogHeaderEdit => { header::edit_dialog::handle_dialog_header_edit_events(app, &event)?; },
                UIState::Command => { commands::command_events(app, &event)?; },
                UIState::HexEditing => { hex::edit::edit_events(app, key)?; },
                UIState::HexSelection => { hex::selection::select_events(app, key)?; },
                UIState::DialogStrings => { hex::strings::dialog_strings_events(app, &event)?; },
                UIState::DialogStringEdit => { hex::strings::dialog_string_edit_events(app, &event)?; },
                UIState::DialogLog => { global::log::dialog_log_events(app, key)?; },
                UIState::Matrix => { global::matrix::events(app, key)?; },
                UIState::DialogSettings => { global::settings::dialog_settings_events(app, key)?; },
                UIState::DialogComment => { hex::comment::dialog_comment_events(app, &event)?; },
                UIState::DialogBase => { global::base::dialog_base_events(app, &event)?; },
                UIState::DialogNames => { hex::names::dialog_names_events(app, &event)?; },
                UIState::DialogNamesRegex => { hex::names::dialog_names_regex_events(app, &event)?; },
                UIState::DialogCalculator => {
                    global::calculator::dialog_calculator_events(app, &event)?;
                }
                UIState::DialogXref => {
                    crate::disasm::xref_dialog::dialog_xref_events(app, &event)?;
                }
                UIState::DialogStringRef => {
                    crate::disasm::string_ref_dialog::dialog_string_ref_events(app, &event)?;
                }
                UIState::DialogFileDialog => {
                    crate::file_dialog::dialog_file_events(app, &event)?;
                }
                UIState::DialogDriveSelect => {
                    crate::file_dialog::dialog_drive_events(app, key)?;
                }
            };
        }
        Event::Resize(width, _height) if app.config.hex_mode_bytes_per_line_auto => {
            // Dragging the terminal narrower used to underflow twice here
            // (`width - 9` on a u16, then `max - 1` when max was 0) and abort.
            app.config.hex_mode_bytes_per_line = crate::util::max_bytes_per_line(width);
        }
        // Any dialog (help, search, calculator, etc.) owns the mouse while it is
        // open. Without this guard, a wheel scroll while a dialog was showing
        // fell through to the hex/disasm scroll handling below, which forces
        // `app.state = UIState::Normal` - leaving the dialog rendered (its
        // `dialog_renderer` was never cleared) but with a state that routes Esc
        // to the underlying view instead of the dialog's own close handler, so
        // the dialog became stuck on screen.
        Event::Mouse(_)
            if !matches!(app.state, UIState::Normal | UIState::HexSelection | UIState::Error) =>
        {
            return Ok(false);
        }
        Event::Mouse(mouse) => {
            // The Header view has its own geometry - a sidebar and a table of rows -
            // and used to fall through to the hex branch below, which silently moved
            // the hex cursor instead. Clicking a row did nothing visible, which is
            // why the Section Tools actions looked unclickable.
            if app.editor_view == crate::editor::AppView::Header {
                header_mouse(app, &mouse);
                return Ok(false);
            }

            let bpl = app.config.hex_mode_bytes_per_line.max(1);
            let addr_width = app.get_addr_col_width();
            let page_start = app.reader.page_start;

            if app.editor_view == crate::editor::AppView::Disasm {
                let bitness = app.bitness();
                let filesize = app.file_info.size;
                let initial_va = app.get_va(page_start);

                let in_rows =
                    mouse.row >= 1 && (mouse.row as usize) < (app.screen.height as usize).saturating_sub(2);

                match mouse.kind {
                    ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left)
                        if in_rows =>
                    {
                        let target_row = (mouse.row - 1) as usize;
                        let buffer = app.file_info.get_buffer_ref();
                        let found_ofs =
                            disasm_offset_at_row(buffer, page_start, initial_va, bitness, filesize, target_row);
                        app.hex_view.offset = found_ofs;
                        app.disasm_selection_anchor = None;
                    }
                    ratatui::crossterm::event::MouseEventKind::Drag(ratatui::crossterm::event::MouseButton::Left)
                        if in_rows =>
                    {
                        let target_row = (mouse.row - 1) as usize;
                        let buffer = app.file_info.get_buffer_ref();
                        let found_ofs =
                            disasm_offset_at_row(buffer, page_start, initial_va, bitness, filesize, target_row);
                        if app.disasm_selection_anchor.is_none() {
                            app.disasm_selection_anchor = Some(app.hex_view.offset);
                        }
                        app.hex_view.offset = found_ofs;
                    }
                    ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                        let buffer = app.file_info.get_buffer_ref();
                        let advance = first_instr_len(buffer, page_start, initial_va, bitness, filesize);
                        let new_ofs = page_start.saturating_sub(advance);
                        app.reader.page_start = new_ofs;
                        app.hex_view.offset = new_ofs;
                        app.disasm_selection_anchor = None;
                    }
                    ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                        let buffer = app.file_info.get_buffer_ref();
                        let advance = first_instr_len(buffer, page_start, initial_va, bitness, filesize);
                        let new_ofs = (page_start + advance).min(filesize.saturating_sub(1));
                        app.reader.page_start = new_ofs;
                        app.hex_view.offset = new_ofs;
                        app.disasm_selection_anchor = None;
                    }
                    _ => {}
                }
                return Ok(false);
            }

            match mouse.kind {
                ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
                    if mouse.row >= 1 && (mouse.row as usize) < (app.screen.height as usize).saturating_sub(2) {
                        let row_idx = (mouse.row - 1) as usize;
                        let col_idx = mouse.column as usize;

                        let byte_in_row = mouse_column_to_byte(col_idx, addr_width, bpl);

                        let offset = (page_start + row_idx * bpl + byte_in_row).min(app.file_info.size.saturating_sub(1));
                        app.hex_view.offset = offset;
                        app.hex_view.cursor.y = row_idx;
                        app.hex_view.cursor.x = byte_in_row;

                        app.hex_view.selection.start = offset;
                        app.hex_view.selection.end = offset;
                        app.hex_view.selection.direction = None;
                        app.hex_view.selection.is_mouse = false;
                        app.state = UIState::Normal;
                    }
                }
                ratatui::crossterm::event::MouseEventKind::Drag(ratatui::crossterm::event::MouseButton::Left) => {
                    if mouse.row >= 1 && (mouse.row as usize) < (app.screen.height as usize).saturating_sub(2) {
                        let row_idx = (mouse.row - 1) as usize;
                        let col_idx = mouse.column as usize;

                        let byte_in_row = mouse_column_to_byte(col_idx, addr_width, bpl);

                        let offset = (page_start + row_idx * bpl + byte_in_row).min(app.file_info.size.saturating_sub(1));
                        let start = app.hex_view.selection.start;
                        if offset < start {
                            app.hex_view.selection.start = offset;
                            app.hex_view.selection.end = start;
                        } else {
                            app.hex_view.selection.end = offset;
                        }
                        app.hex_view.selection.is_mouse = true;
                        // A mouse drag happens over the byte grid, so it is a hex
                        // selection regardless of which column last had keyboard
                        // focus.
                        app.hex_view.selection_target = crate::editor::EditingTarget::Hex;
                        app.hex_view.offset = offset;
                        app.hex_view.cursor.y = row_idx;
                        app.hex_view.cursor.x = byte_in_row;
                        app.state = UIState::HexSelection;
                    }
                }
                ratatui::crossterm::event::MouseEventKind::Up(ratatui::crossterm::event::MouseButton::Left) => {
                    if app.hex_view.selection.start != app.hex_view.selection.end {
                        let dump = hex::selection::format_mouse_selection_dump(
                            app,
                            app.hex_view.selection.start,
                            app.hex_view.selection.end,
                        );
                        if let Ok(clip) = app.clipboard.as_mut() {
                            let _ = clip.set_text(dump);
                        }
                    }
                }
                ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                    let step = bpl * 3;
                    let new_offset = app.reader.page_start.saturating_sub(step);
                    app.reader.page_start = new_offset;
                    app.hex_view.offset = new_offset;
                    app.hex_view.selection.start = 0;
                    app.hex_view.selection.end = 0;
                    app.state = UIState::Normal;
                }
                ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                    let step = bpl * 3;
                    let new_offset = (app.reader.page_start + step).min(app.file_info.size.saturating_sub(1));
                    app.reader.page_start = new_offset;
                    app.hex_view.offset = new_offset;
                    app.hex_view.selection.start = 0;
                    app.hex_view.selection.end = 0;
                    app.state = UIState::Normal;
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(true)
}

/// Click and wheel handling for the Header view.
///
/// The layout is the one `header/formats/pe/draw.rs` builds: a 25% sidebar on the
/// left, the detail table on the right, each inside a bordered box whose first
/// inner row is the column header.
fn header_mouse(app: &mut App, mouse: &ratatui::crossterm::event::MouseEvent) {
    use crate::header::header_view::HeaderPane;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    let sidebar_width = app.screen.width / 4;
    let in_sidebar = mouse.column < sidebar_width;

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Row 0 is the box's top border; the detail table spends its first inner
            // row on column headings, the sidebar list does not.
            let Some(row) = mouse.row.checked_sub(1) else { return };

            if in_sidebar {
                app.header_view.active_pane = HeaderPane::Sidebar;
                let max = crate::header::formats::pe::events::SIDEBAR_CATEGORIES - 1;
                let picked = (row as usize).min(max);
                if picked != app.header_view.sidebar_index {
                    app.header_view.sidebar_index = picked;
                    app.header_view.detail_index = 0;
                    app.header_view.detail_col_index = 0;
                }
                return;
            }

            app.header_view.active_pane = HeaderPane::Detail;
            let Some(row) = row.checked_sub(1) else { return }; // column headings
            let picked = row as usize;
            let max = crate::header::formats::pe::events::max_detail_index_for_mouse(app);
            if picked > max {
                return;
            }
            app.header_view.detail_index = picked;
            if app.header_view.sidebar_index == 4 {
                app.header_view.tools_section_index = picked;
            }

            // On the Section Tools tab the rows *are* the actions, so a click runs
            // the one it lands on. Both are recoverable: "Add New Section" only
            // opens a size prompt, and the alignment is a staged edit like any
            // other.
            if app.header_view.sidebar_index == 6 {
                crate::header::formats::pe::events::run_section_tool(app, picked);
            }
        }
        MouseEventKind::ScrollDown => {
            let max = crate::header::formats::pe::events::max_detail_index_for_mouse(app);
            app.header_view.detail_index = (app.header_view.detail_index + 3).min(max);
        }
        MouseEventKind::ScrollUp => {
            app.header_view.detail_index = app.header_view.detail_index.saturating_sub(3);
        }
        _ => {}
    }
}

pub fn handle_replace_pattern_events(app: &mut App, event: &Event) -> Result<bool> {
    use ratatui::crossterm::event::KeyModifiers;

    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match key.code {
            KeyCode::Esc => {
                app.hex_view.replace_dialog.reset();
                app.state = UIState::Normal;
                return Ok(false);
            }
            // Changing field drops the block: it belonged to the field being left.
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                app.hex_view.replace_dialog.active_field =
                    (app.hex_view.replace_dialog.active_field + 1) % 2;
                app.hex_view.replace_dialog.anchor = None;
                return Ok(false);
            }
            // Find Next (Enter)
            KeyCode::Enter => {
                execute_find(app, true);
                return Ok(false);
            }
            // Replace 1 match (Alt+R)
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::ALT) => {
                execute_replace_one(app);
                return Ok(false);
            }
            // Replace All (Alt+A)
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::ALT) => {
                execute_replace_all(app);
                return Ok(false);
            }
            _ => {}
        }
    }

    // Shift+arrows, Shift+Home/End and Ctrl+C/X/V over the block.
    crate::text_field::handle_key(app, replace_field, event);

    Ok(false)
}

/// The focused field of the Replace dialog, and the dialog's selection anchor.
fn replace_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    let dialog = &mut app.hex_view.replace_dialog;
    let input = if dialog.active_field == 0 {
        &mut dialog.search_input
    } else {
        &mut dialog.replace_input
    };
    (input, &mut dialog.anchor)
}

fn execute_find(app: &mut App, forward: bool) {
    use crate::hex::pattern_engine::{HexPattern, HexReplacer};

    let search_str = app.hex_view.replace_dialog.search_input.value().to_string();
    let replace_str = app.hex_view.replace_dialog.replace_input.value().to_string();

    let search_pat = match HexPattern::parse(&search_str) {
        Ok(p) => p,
        Err(e) => {
            app.hex_view.replace_dialog.error_message = Some(e.to_string());
            app.hex_view.replace_dialog.status_message = None;
            return;
        }
    };

    let replace_pat = match HexPattern::parse(&replace_str) {
        Ok(p) => p,
        Err(e) => {
            app.hex_view.replace_dialog.error_message = Some(e.to_string());
            app.hex_view.replace_dialog.status_message = None;
            return;
        }
    };

    // Direction picks the neighbouring hit out of the list collected below.
    // Searched against the buffer with pending edits applied, matching what the
    // hex view shows and what `hex/search.rs` does. Reading the raw mmap here
    // meant Find Next in this dialog matched the bytes on disk while the screen
    // showed something else.
    let cursor = app.hex_view.offset;
    let wrap = app.config.search_wrap;
    let outcome = app.with_effective_buffer(|buffer| {
        let replacer = match HexReplacer::new(search_pat, replace_pat, buffer) {
            Ok(r) => r,
            Err(e) => return Err(e.to_string()),
        };
        // The whole hit list, not just the next one: the status row reports "which
        // of how many", the view paints every hit, and stepping backwards becomes a
        // lookup in the list instead of a second scan.
        let all = replacer.find_all(buffer);
        // `all` is in scan order, so the neighbour in either direction is one
        // partition point away.
        let hit = if forward {
            let idx = all.partition_point(|&ofs| ofs <= cursor);
            all.get(idx).copied().or(if wrap { all.first().copied() } else { None })
        } else {
            let idx = all.partition_point(|&ofs| ofs < cursor);
            if idx > 0 {
                Some(all[idx - 1])
            } else if wrap {
                all.last().copied()
            } else {
                None
            }
        };
        Ok(hit.map(|h| (h, replacer.pattern_len(), all)))
    });

    let hit = match outcome {
        Ok(hit) => hit,
        Err(message) => {
            app.hex_view.replace_dialog.error_message = Some(message);
            app.hex_view.replace_dialog.status_message = None;
            return;
        }
    };

    if let Some((hit, pattern_len, all)) = hit {
        // Scroll the view to the match and keep it clear of the dialog. This used
        // to set `offset` and the cursor coordinates by hand without touching
        // `page_start`, so the page never moved: the match counter changed and
        // nothing else did.
        // Same label widths the dialog draws with, so the rect matches what is on
        // screen in any interface language.
        let lang = app.config.lang;
        let labels = [
            crate::i18n::M::LblSearch.tr(lang),
            crate::i18n::M::LblReplace.tr(lang),
        ];
        let label_width = crate::hex::field_box::label_width(&labels);
        let dialog = crate::hex::field_box::dialog_rect(app, label_width, 2, 2);
        crate::hex::field_box::reveal_behind_dialog(app, hit, dialog);
        app.hex_view.selection.start = hit;
        // Clamp to EOF: an unclamped end feeds Selection::contains in the draw
        // loops with offsets past the end of the file.
        app.hex_view.selection.end = (hit + pattern_len)
            .min(app.file_info.size.saturating_sub(1))
            .max(hit);
        // Which match this is, so repeated Enter presses read as progress through
        // the file rather than as an unlabelled jump.
        let index = all.iter().position(|&o| o == hit).map(|i| i + 1).unwrap_or(1);

        // Hand the hits to the shared search state: the view paints every one of
        // them, the status bar shows the counter, and F3 / Shift+F3 can step
        // through them after the dialog is closed.
        app.hex_view.search.matches = all.clone();
        app.hex_view.search.match_index = Some(index - 1);
        app.hex_view.search.match_len = pattern_len;
        app.hex_view.replace_dialog.error_message = None;
        // Same wording as the Find dialog and F3: "Match (1341/2063) offset : 0x2290".
        app.hex_view.replace_dialog.status_message =
            Some(crate::hex::search::match_position_message(app, hit));
    } else {
        app.hex_view.replace_dialog.error_message = None;
        app.hex_view.replace_dialog.status_message =
            Some(crate::i18n::M::FindNoMatch.tr(app.config.lang).to_string());
    }
}

fn execute_replace_one(app: &mut App) {
    use crate::hex::pattern_engine::{HexPattern, HexReplacer};

    let search_str = app.hex_view.replace_dialog.search_input.value().to_string();
    let replace_str = app.hex_view.replace_dialog.replace_input.value().to_string();

    let search_pat = match HexPattern::parse(&search_str) { Ok(p) => p, Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; } };
    let replace_pat = match HexPattern::parse(&replace_str) { Ok(p) => p, Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; } };

    // Pending edits applied first: without this, replacing over a region that had
    // already been edited matched the on-disk bytes and then wrote raw-derived
    // bytes back over those edits.
    let mut buffer_vec = app.with_effective_buffer(|b| b.to_vec());
    let replacer = match HexReplacer::new(search_pat, replace_pat, &buffer_vec) {
        Ok(r) => r,
        Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; }
    };

    let offset = app.hex_view.offset;
    if replacer.replace_at(&mut buffer_vec, offset) {
        let len = replacer.pattern_len();
        for i in 0..len {
            let pos = offset + i;
            let b = buffer_vec[pos];
            crate::hex::edit::record_edit(app, pos, b);
        }
        app.hex_view.replace_dialog.error_message = None;
        let message = crate::i18n::fill(
            crate::i18n::M::ReplacedAt.tr(app.config.lang),
            &[&format!("{:X}", offset)],
        );
        // `execute_find_next` overwrites the status with the next match and its
        // count, which is the more useful of the two, so this is only what shows
        // when there is no next match.
        app.hex_view.replace_dialog.status_message = Some(message);
        execute_find(app, true);
    } else {
        app.hex_view.replace_dialog.error_message = None;
        app.hex_view.replace_dialog.status_message =
            Some(crate::i18n::M::NotAtAMatch.tr(app.config.lang).to_string());
    }
}

fn execute_replace_all(app: &mut App) {
    use crate::hex::pattern_engine::{HexPattern, HexReplacer};

    let search_str = app.hex_view.replace_dialog.search_input.value().to_string();
    let replace_str = app.hex_view.replace_dialog.replace_input.value().to_string();

    let search_pat = match HexPattern::parse(&search_str) { Ok(p) => p, Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; } };
    let replace_pat = match HexPattern::parse(&replace_str) { Ok(p) => p, Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; } };

    // Same reason as Replace One: the search and the bytes written back both have
    // to see the pending edits.
    let mut buffer_vec = app.with_effective_buffer(|b| b.to_vec());
    let replacer = match HexReplacer::new(search_pat, replace_pat, &buffer_vec) {
        Ok(r) => r,
        Err(e) => { app.hex_view.replace_dialog.error_message = Some(e.to_string()); return; }
    };

    let hits = replacer.replace_all(&mut buffer_vec);
    let count = hits.len();
    let len = replacer.pattern_len();

    // The pattern no longer matches anything, so the highlight has to go with it -
    // otherwise the old hits stay painted over bytes that have already changed.
    app.hex_view.search.matches.clear();
    app.hex_view.search.match_index = None;
    app.hex_view.search.match_len = 0;

    for &start in &hits {
        for i in 0..len {
            let pos = start + i;
            let b = buffer_vec[pos];
            crate::hex::edit::record_edit(app, pos, b);
        }
    }

    app.hex_view.replace_dialog.error_message = None;
    app.hex_view.replace_dialog.status_message = Some(crate::i18n::fill(
        crate::i18n::M::ReplacedCount.tr(app.config.lang),
        &[&count.to_string()],
    ));
}

#[cfg(test)]
mod replace_dialog_tests {
    use crate::app::App;

    fn scratch(name: &str, bytes: &[u8]) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("dz6_replace_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, bytes).expect("write");
        let s = path.to_str().expect("utf-8").to_string();
        (dir, s)
    }

    fn app_with(bytes: &[u8], name: &str) -> (std::path::PathBuf, App) {
        let (dir, path) = scratch(name, bytes);
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, true).expect("open");
        (dir, app)
    }

    fn set_patterns(app: &mut App, find: &str, replace: &str) {
        app.hex_view.replace_dialog.search_input = tui_input::Input::new(find.to_string());
        app.hex_view.replace_dialog.replace_input = tui_input::Input::new(replace.to_string());
    }

    /// Find Next must match what the screen shows, i.e. the buffer with pending
    /// edits applied - not the bytes on disk.
    ///
    /// The pattern below exists only *after* an edit, so with the old raw-buffer
    /// search there was nothing to find.
    #[test]
    fn find_next_sees_pending_edits() {
        let (dir, mut app) = app_with(&[0x00; 0x40], "find_edits");

        // Create `AA BB` at offset 0x10 as an unsaved edit.
        app.hex_view.changed_bytes.insert(0x10, "AA".to_string());
        app.hex_view.changed_bytes.insert(0x11, "BB".to_string());

        set_patterns(&mut app, "AA BB", "CC DD");
        super::execute_find(&mut app, true);

        let status = app.hex_view.replace_dialog.status_message.clone();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            app.hex_view.offset, 0x10,
            "the edited pattern should have been found at 0x10; status was {:?}",
            status
        );
    }

    /// Replace must not resurrect on-disk bytes over an existing edit.
    ///
    /// Offset 0x11 is edited to 0xBB. Replacing a wildcard pattern at 0x10 used to
    /// read the raw buffer, so the byte written back for the wildcard position was
    /// the on-disk 0x00 - silently discarding the edit.
    #[test]
    fn replace_one_preserves_earlier_edits_under_a_wildcard() {
        let (dir, mut app) = app_with(&[0x00; 0x40], "replace_wildcard");

        app.hex_view.changed_bytes.insert(0x10, "11".to_string());
        app.hex_view.changed_bytes.insert(0x11, "BB".to_string());
        app.hex_view.offset = 0x10;

        // Match the edited 0x11 and leave the next byte alone.
        set_patterns(&mut app, "11 ??", "22 ??");
        super::execute_replace_one(&mut app);

        let at_10 = app.hex_view.changed_bytes.get(&0x10).cloned();
        let at_11 = app.hex_view.changed_bytes.get(&0x11).cloned();
        let status = app.hex_view.replace_dialog.status_message.clone();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(at_10.as_deref(), Some("22"), "status was {:?}", status);
        assert_eq!(
            at_11.as_deref(),
            Some("BB"),
            "the wildcard position must keep the pending edit, not revert to the on-disk byte"
        );
    }

    /// Replace All goes through the same buffer.
    #[test]
    fn replace_all_sees_pending_edits() {
        let (dir, mut app) = app_with(&[0x00; 0x40], "replace_all_edits");

        app.hex_view.changed_bytes.insert(0x20, "77".to_string());

        set_patterns(&mut app, "77", "88");
        super::execute_replace_all(&mut app);

        let at_20 = app.hex_view.changed_bytes.get(&0x20).cloned();
        let status = app.hex_view.replace_dialog.status_message.clone();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            at_20.as_deref(),
            Some("88"),
            "the edited byte should have been matched and replaced; status was {:?}",
            status
        );
    }

    /// A clean file still works, so the change didn't only fix the edited case.
    #[test]
    fn replace_works_without_any_edits() {
        let mut bytes = vec![0x00; 0x40];
        bytes[0x08] = 0x99;
        let (dir, mut app) = app_with(&bytes, "replace_clean");
        app.hex_view.offset = 0x08;

        set_patterns(&mut app, "99", "5A");
        super::execute_replace_one(&mut app);

        let at_8 = app.hex_view.changed_bytes.get(&0x08).cloned();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(at_8.as_deref(), Some("5A"));
    }
}

#[cfg(test)]
mod modified_fkey_tests {
    use crate::app::App;
    use crate::editor::{AppView, UIState};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dz6_modified_fkeys");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("f_{}.bin", SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0x90u8; 0x200]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        // Writable, so nothing is refused for being read-only.
        app.load_file(path.to_str().expect("path"), 0, false).expect("open");
        app.file_info.is_read_only = false;
        app.editor_view = AppView::Hex;
        app.state = UIState::Normal;
        app
    }

    /// Dispatches the way `handle_events` does for a view: global first, then the
    /// view - but only if the global handler did not change the state.
    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let state_before = app.state;
        let _ = crate::global::events::handle_global_events(app, key);
        if app.state == state_before {
            match app.editor_view {
                AppView::Hex => {
                    let _ = crate::hex::events::hex_mode_events(app, key);
                }
                AppView::Disasm => {
                    let _ = crate::disasm::events::disasm_mode_events(app, key);
                }
                AppView::Text => {
                    let _ = crate::text::events::text_mode_events(app, key);
                }
                AppView::Header => {
                    let _ = crate::header::events::header_view_events(app, key);
                }
            }
        }
    }

    /// Alt+F2 toggles the address column and nothing else.
    ///
    /// The global handler for it does not change the state, so the key went on to
    /// the Hex view as well - where a bare `F(2)` arm dropped into edit mode. Every
    /// modified F-key needs the plain arm to exclude the modifier.
    #[test]
    fn alt_f2_only_toggles_the_address_column() {
        let mut app = app_with_file();
        let before = app.hex_view.show_va;

        press(&mut app, KeyCode::F(2), KeyModifiers::ALT);

        assert_eq!(app.hex_view.show_va, !before, "the toggle must happen");
        assert!(
            app.state == UIState::Normal,
            "Alt+F2 must not enter edit mode"
        );
    }

    /// Plain F2 still enters edit mode.
    #[test]
    fn plain_f2_still_enters_edit_mode() {
        let mut app = app_with_file();
        press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        assert!(app.state == UIState::HexEditing);
    }

    /// Alt+F6 opens the image-base dialog instead of the strings list, and Alt+F7
    /// cycles the decoding width without the view seeing the key.
    #[test]
    fn other_modified_fkeys_do_not_fall_through() {
        let mut app = app_with_file();
        press(&mut app, KeyCode::F(6), KeyModifiers::ALT);
        assert!(
            app.state == UIState::DialogBase,
            "Alt+F6 should open the base dialog, not the strings list"
        );

        let mut app = app_with_file();
        press(&mut app, KeyCode::F(7), KeyModifiers::ALT);
        assert!(
            app.state == UIState::Normal && app.editor_view == AppView::Hex,
            "Alt+F7 must not switch to the Text view"
        );
    }
}


