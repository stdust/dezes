use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{app::App, beep, commands, editor::{AppView, UIState}, global};

use std::io::Result;

pub fn handle_global_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // Esc: return from Header or Text view to previous view OR clear selection in Disasm view
        KeyCode::Esc => {
            // Clear any active Ctrl+F search result (and its "(n/m)" counter
            // in the status bar) first - Esc is otherwise a no-op / beeps
            // when there's nothing else selected, so this doesn't fight with
            // any of the branches below.
            if !app.hex_view.search.matches.is_empty() {
                app.hex_view.search.matches.clear();
                app.hex_view.search.match_index = None;
                app.hex_view.search.match_len = 0;
            }
            if app.editor_view == AppView::Header {
                let ret = app.prev_editor_view;
                app.editor_view = ret;
                if ret == AppView::Text {
                    app.prev_editor_view = app.last_primary_view;
                } else if ret == AppView::Hex || ret == AppView::Disasm {
                    app.last_primary_view = ret;
                }
            } else if app.editor_view == AppView::Text {
                let target = if app.prev_editor_view == AppView::Text || app.prev_editor_view == AppView::Header {
                    app.last_primary_view
                } else {
                    app.prev_editor_view
                };
                app.editor_view = target;
                app.last_primary_view = target;
                app.prev_editor_view = target;
            } else if app.editor_view == AppView::Disasm && app.disasm_selection_anchor.is_some() {
                app.disasm_selection_anchor = None;
            } else {
                beep!();
            }
        }
        // Alt+F1: Drive selection popup
        KeyCode::F(1) if key.modifiers.contains(KeyModifiers::ALT) => {
            app.open_drive_dialog();
        }
        // F1: help, from any view.
        //
        // It used to be handled only in `hex/events.rs`, so pressing F1 in the
        // Disasm, Text or Header view did nothing - even though the help text
        // documents those views. The Alt+F1 arm above matches first, so this one
        // only ever sees a bare F1.
        KeyCode::F(1) => {
            app.state = UIState::DialogHelp;
            app.dialog_renderer = Some(crate::hex::help::dialog_help_draw);
        }
        // F8: About / program info. F8 rather than F10 or F11, which terminals
        // tend to intercept for their own menu and fullscreen bindings.
        KeyCode::F(8) => {
            app.open_about_dialog();
        }
        // F9 or Ctrl+O: Open File Dialog
        KeyCode::F(9) | KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::F(9) => {
            app.open_file_dialog();
        }
        // F12: save and quit, same as ':wq'.
        //
        // A failed write must not quit, otherwise the edits are lost with no
        // indication of why - so the error is logged, the terminal beeps and
        // the session stays open (read-only files are the common case here).
        KeyCode::F(12) => {
            match app.write_to_file() {
                Ok(()) => {
                    app.persist_annotations();
                    app.running = false;
                }
                Err(e) => {
                    // Read-only is the common case here, and a bare beep gave no
                    // hint that the edits were still unsaved.
                    let message = crate::i18n::fill(
                        crate::i18n::M::ErrSaveFailedQuit.tr(app.config.lang),
                        &[&e.to_string()],
                    );
                    app.error(message);
                }
            }
        }
        // F3: repeat last pattern search forward. Shift+F3: repeat backward.
        // (Replaces the old '/' forward-search / 'n'/'N' repeat shortcuts.)
        // Alt+F3 is a different feature (revert the byte under the cursor),
        // handled per-view, so it must not be treated as a search repeat here.
        KeyCode::F(3) if !key.modifiers.contains(KeyModifiers::ALT) => {
            let forward = !key.modifiers.contains(KeyModifiers::SHIFT);
            crate::hex::search::goto_adjacent_match(app, forward);
        }
        // F7: jump to Text view from any view.
        //
        // Guarded on ALT, like the F3 search-repeat arm above: an unguarded
        // `KeyCode::F(7)` also swallows Alt+F7, which made the decoding-width cycle
        // below unreachable.
        KeyCode::F(7) if !key.modifiers.contains(KeyModifiers::ALT) => {
            if app.editor_view != AppView::Text {
                if app.editor_view == AppView::Hex || app.editor_view == AppView::Disasm {
                    app.last_primary_view = app.editor_view;
                }
                app.prev_editor_view = app.editor_view;
                app.editor_view = AppView::Text;
            } else {
                // Pressed again in the Text view: the same key takes you back.
                app.return_to_primary_view();
            }
        }
        // F4: jump to Header view from any view.
        //
        // Guarded on ALT so Alt+F4 stays the terminal's "close window" rather than
        // switching views on the way out.
        KeyCode::F(4) if !key.modifiers.contains(KeyModifiers::ALT) => {
            if app.editor_view != AppView::Header {
                if app.editor_view == AppView::Hex || app.editor_view == AppView::Disasm {
                    app.last_primary_view = app.editor_view;
                }
                app.prev_editor_view = app.editor_view;
                app.editor_view = AppView::Header;
            } else {
                app.return_to_primary_view();
            }
        }
        // F5: open String References dialog
        KeyCode::F(5) => {
            let items = crate::disasm::string_ref::scan_string_references(app);
            let count = items.len();
            let truncated = crate::disasm::string_ref::is_truncated(&items);
            App::log(
                app,
                format!(
                    "String references: {}{}",
                    count,
                    if truncated { " (limit reached)" } else { "" }
                ),
            );
            app.disasm_string_ref_dialog.items = items;
            app.disasm_string_ref_dialog.filter_input = tui_input::Input::default();
            app.disasm_string_ref_dialog.focus_filter = false;
            app.disasm_string_ref_dialog.selected_index = 0;
            app.disasm_string_ref_dialog.update_filter();
            app.state = UIState::DialogStringRef;
            app.dialog_renderer = Some(|app, frame| crate::disasm::string_ref_dialog::draw_string_ref_dialog(app, frame, app.screen));
        }
        // Tab: switch view (Hex -> Text -> Disasm -> Hex, skipping Disasm if non-executable)
        KeyCode::Tab | KeyCode::BackTab => {
            if app.editor_view != AppView::Header {
                app.switch_editor_view();
            }
        }
        // copy current VA address to clipboard (Ctrl+X)
        //
        // Was a bare 'x', which is vi's delete-character - the one mapping most
        // likely to be pressed by accident with the opposite intent.
        KeyCode::Char('x') | KeyCode::Char('X') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let va = app.get_va(app.hex_view.offset);
            let va_hex = format!("{:X}", va);
            if let Ok(clipboard) = &mut app.clipboard {
                let _ = clipboard.set_text(va_hex.clone());
            }
            App::log(app, format!("Copied address 0x{} to clipboard", va_hex));
        }
        // Ctrl+G: Goto Address (HEX / VA), same key as x64dbg's goto expression.
        KeyCode::Char('g') | KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let is_disasm = app.editor_view == crate::editor::AppView::Disasm;
            let addr_str = if app.hex_view.show_va || is_disasm {
                let va = app.get_va(app.hex_view.offset);
                format!("{:X}", va)
            } else {
                format!("{:X}", app.hex_view.offset)
            };
            app.state = UIState::DialogGoto;
            app.goto_input = tui_input::Input::new(addr_str);
            app.goto_selection_all = true;
            app.goto_selection_anchor = None;
            app.dialog_renderer = Some(crate::goto_dialog::dialog_goto_draw);
        }
        // '-' key or Ctrl + Left: Jump Backward to previous cursor position
        KeyCode::Char('-') | KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::Char('-') => {
            app.jump_back();
        }
        // '+' key or Ctrl + Right: Jump Forward to next cursor position
        KeyCode::Char('+') | KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::Char('+') => {
            app.jump_forward();
        }
        // Alt+F6: override the image base every address is computed from.
        //
        // Alt+F3 already reverts the byte under the cursor, so the base did not go
        // there; F6 is the next free slot that displaces nothing.
        KeyCode::F(6) if key.modifiers.contains(KeyModifiers::ALT) => {
            crate::global::base::open_base_dialog(app);
        }
        // Alt+F7: cycle the forced decoding width, auto -> 16 -> 32 -> 64 -> auto.
        //
        // Next to Alt+F6 on purpose: the base and the width are the two "how should
        // these bytes be read" settings, and they are usually changed together on a
        // raw dump.
        KeyCode::F(7) if key.modifiers.contains(KeyModifiers::ALT) => {
            let label = app.cycle_bitness();
            App::log(app, format!("Decoding width: {}", label));
        }
        // Alt+F2: toggle the address column between file offset and VA.
        //
        // Was plain 'z' in the Hex view only. A bare letter is a poor fit for a
        // display mode - it collides with the byte the user might mean to type -
        // and the toggle also drives `:goto`, `x` and the status bar, so it belongs
        // in every view. Alt+F1 was taken by the drive popup, hence F2.
        KeyCode::F(2) if key.modifiers.contains(KeyModifiers::ALT) => {
            app.hex_view.show_va = !app.hex_view.show_va;
            let mode = if app.hex_view.show_va {
                "VA (Virtual Address)"
            } else {
                "File Offset"
            };
            App::log(app, format!("Address display mode switched to {}", mode));
        }
        // ';': comment the byte under the cursor, from any view.
        //
        // This was bound in `hex/events.rs` only, so pressing ';' in the Disasm
        // view did nothing at all - even though that view has a comment column and
        // is where annotating an address is most useful.
        KeyCode::Char(';') if app.file_info.size > 0 => {
            crate::hex::comment::open_comment_dialog(app);
        }
        // log window
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::ALT) => {
            global::log::open_log_dialog(app);
        }

        // command bar
        KeyCode::Char(':') => {
            app.state = UIState::Command;
            app.dialog_renderer = Some(commands::command_draw);
        }
        // calculator
        KeyCode::Char('=') => {
            app.state = UIState::DialogCalculator;
            app.dialog_renderer = Some(global::calculator::dialog_calculator_draw);
        }
        // modify block data (Ctrl+K)
        //
        // Not Ctrl+M: that is ASCII CR, which many terminals cannot tell apart from
        // Enter - and Enter is bound (selection commit, header field edit).
        KeyCode::Char('k') | KeyCode::Char('K') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // The apply step refuses read-only files, so opening the dialog would
            // only lead to the operation being dropped in silence.
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoModifyBlock);
                return Ok(false);
            }
            app.state = UIState::DialogModifyBlock;
            app.dialog_renderer = Some(crate::hex::modify_dialog::draw_modify_dialog);
            app.hex_view.modify_dialog.reset();
        }
        // Ctrl + H: Wildcard Hex Pattern Replace
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.state = UIState::DialogReplacePattern;
            app.hex_view.replace_dialog = crate::hex::replace_dialog::ReplaceDialog::new();
        }
        // Ctrl+B: Find Pattern, the same key x64dbg uses for a binary search.
        //
        // Ctrl+F was an alias for this and has been removed, so the key is free.
        KeyCode::Char('b') | KeyCode::Char('B') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.state = UIState::DialogFindPattern;
            app.hex_view.find_dialog.reset();
            app.dialog_renderer = Some(crate::hex::find_dialog::draw_find_dialog);
        }
        // Ctrl + R: Cross References (Xrefs) Search & Popup Dialog (Hex & Disasm Views)
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            crate::disasm::xref_dialog::open_xref_dialog(app);
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod view_toggle_tests {
    use crate::app::App;
    use crate::editor::AppView;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dezes_view_toggle");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("t_{}_{}.bin", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0x90u8; 0x800]).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = crate::global::events::handle_global_events(app, key);
    }

    /// F4 and F7 are toggles. Pressing them in the view they open used to do
    /// nothing, so the only way out was Esc.
    #[test]
    fn f4_returns_from_the_header_view() {
        let mut app = app_with_file();
        assert_eq!(app.editor_view, AppView::Hex);

        press(&mut app, KeyCode::F(4));
        assert_eq!(app.editor_view, AppView::Header);

        press(&mut app, KeyCode::F(4));
        assert_eq!(app.editor_view, AppView::Hex, "F4 has to come back");
    }

    #[test]
    fn f7_returns_from_the_text_view() {
        let mut app = app_with_file();

        press(&mut app, KeyCode::F(7));
        assert_eq!(app.editor_view, AppView::Text);

        press(&mut app, KeyCode::F(7));
        assert_eq!(app.editor_view, AppView::Hex, "F7 has to come back");
    }

    /// Coming back lands on the primary view that was left, not on the other
    /// secondary one.
    #[test]
    fn the_return_remembers_disasm() {
        let mut app = app_with_file();
        app.editor_view = AppView::Disasm;
        app.last_primary_view = AppView::Disasm;
        app.prev_editor_view = AppView::Disasm;

        press(&mut app, KeyCode::F(7)); // Text
        press(&mut app, KeyCode::F(4)); // Header, from the Text view
        assert_eq!(app.editor_view, AppView::Header);

        press(&mut app, KeyCode::F(4));
        assert_eq!(
            app.editor_view,
            AppView::Disasm,
            "the way back is the primary view, not Text"
        );
    }
}