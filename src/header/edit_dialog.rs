use std::io::Result;
use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode},
    layout::{Alignment, Constraint, Direction, Layout},
    widgets::{Block, Clear, Paragraph},
};

use crate::{app::App, editor::UIState};

pub fn draw_header_edit_dialog(app: &mut App, frame: &mut Frame) {
    let constraints = vec![
        Constraint::Min(0),          // Header view main content area
        Constraint::Length(1),       // status bar
        Constraint::Length(1),       // command bar
    ];

    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    let layout = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]);
    let [_, detail_area] = vertical_layout[0].layout(&layout);

    let is_name_field = app.header_view.edit_name == "Name" || app.header_view.edit_name.ends_with(".Name");
    let title_str = if is_name_field {
        crate::i18n::M::LblSectionNameMax8.tr(app.config.lang).to_string()
    } else {
        format!(" {} ", app.header_view.edit_name)
    };

    // Wide enough for the title (plus its corner decorations) and the value,
    // but no wider - the old fixed 54 columns left a lot of empty box around a
    // short field name like "ImageBase".
    let min_width = (title_str.chars().count() + 6) as u16;
    let width = min_width
        .max(24)
        .min(detail_area.width.saturating_sub(4).max(24));
    let height = 4;
    // Slightly above dead centre, matching the hex dialogs.
    let dialog_area = crate::hex::field_box::centered_rect_above(width, height, detail_area);

    frame.render_widget(Clear, dialog_area);

    let input_text = app.goto_input.value();

    let block = Block::bordered()
        .title(title_str.as_str())
        .title_alignment(Alignment::Center);

    // Opened with the whole current value selected (and the cursor at the end),
    // like the Goto dialog: typing replaces it outright, Enter keeps it.
    let paragraph = if app.goto_selection_all && !input_text.is_empty() {
        use ratatui::text::{Line, Span};
        Paragraph::new(vec![
            Line::raw(""),
            Line::from(vec![Span::styled(
                input_text.to_string(),
                app.config.theme.highlight,
            )]),
        ])
        .style(app.config.theme.dialog)
        .block(block)
    } else {
        use ratatui::text::Line;
        Paragraph::new(vec![
            Line::raw(""),
            crate::text_field::render_line(
                &app.goto_input,
                app.goto_selection_anchor,
                app.config.theme.dialog,
                app.config.theme.highlight,
            ),
        ])
        .style(app.config.theme.dialog)
        .block(block)
    };

    frame.render_widget(paragraph, dialog_area);

    // Render Cursor
    let cursor_x = dialog_area.x + 1 + app.goto_input.cursor() as u16;
    let cursor_y = dialog_area.y + 2;
    frame.set_cursor_position((cursor_x.min(dialog_area.x + width - 2), cursor_y));
}

/// The header-edit box and its selection anchor.
///
/// The box borrows the Goto dialog's `Input`, so it borrows its anchor too; the two
/// are never open at the same time.
fn header_edit_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (&mut app.goto_input, &mut app.goto_selection_anchor)
}

pub fn handle_dialog_header_edit_events(app: &mut App, event: &Event) -> Result<()> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.state = UIState::Normal;
                app.goto_input = tui_input::Input::default();
                app.goto_selection_all = false;
            }
            // First keystroke over a fully-selected value replaces it, rather
            // than appending to the value the field was opened with.
            KeyCode::Char(c) if app.goto_selection_all => {
                app.goto_selection_all = false;
                app.goto_input = tui_input::Input::new(c.to_string());
            }
            KeyCode::Backspace | KeyCode::Delete if app.goto_selection_all => {
                app.goto_selection_all = false;
                app.goto_input = tui_input::Input::default();
            }
            KeyCode::Enter => {
                // Owned, so staging the bytes below can take `app` mutably: the
                // input lives on `app` and the borrow would otherwise still be held.
                let input_str = app.goto_input.value().to_string();
                // A DOS/COFF/Optional field cannot move the import directory, so
                // those edits take the cheap re-parse. A section header or a data
                // directory does move it, and takes the full one.
                let scope = if app.header_view.sidebar_index <= 2 {
                    crate::app::HeaderScope::Headers
                } else {
                    crate::app::HeaderScope::Everything
                };
                let offset = app.header_view.edit_offset;
                let size = app.header_view.edit_size;

                // Refuse to stage bytes outside the file.
                let span = if app.header_view.edit_name == "Name" || app.header_view.edit_name.ends_with(".Name") {
                    8
                } else {
                    size.min(8)
                };
                let limit = app.file_info.buffer_len();
                let end = offset.checked_add(span);
                if end.is_none_or(|end| end > limit) {
                    App::log(
                        app,
                        format!(
                            "Refusing to edit '{}': offset 0x{:X}+{} is outside the file (0x{:X} bytes)",
                            app.header_view.edit_name, offset, span, limit
                        ),
                    );
                    crate::beep!();
                    app.state = UIState::Normal;
                    app.goto_input = tui_input::Input::default();
                    app.goto_selection_all = false;
                    return Ok(());
                }

                if app.header_view.edit_name == "Name" || app.header_view.edit_name.ends_with(".Name") {
                    let name_bytes = input_str.as_bytes();
                    for i in 0..8 {
                        let byte_val = if i < name_bytes.len() {
                            name_bytes[i]
                        } else {
                            0x00
                        };
                        crate::hex::edit::record_edit(app, offset + i, byte_val);
                    }
                    let final_name = input_str.chars().take(8).collect::<String>();
                    App::log(
                        app,
                        format!(
                            "Section name at 0x{:X} modified to '{}'",
                            offset, final_name
                        ),
                    );
                    app.update_file_headers_scoped(scope);
                } else {
                    let clean_str = input_str.trim().trim_start_matches("0x").trim_start_matches("0X");
                    let parsed_val = if input_str.trim().starts_with("0x") || input_str.trim().starts_with("0X") {
                        u64::from_str_radix(clean_str, 16).ok()
                    } else {
                        input_str.trim().parse::<u64>().or_else(|_| u64::from_str_radix(clean_str, 16)).ok()
                    };

                    if let Some(val) = parsed_val {
                        // Convert value to little-endian bytes and write to changed_bytes
                        let le_bytes = val.to_le_bytes();
                        for (i, &byte_val) in le_bytes[..size.min(8)].iter().enumerate() {
                            crate::hex::edit::record_edit(app, offset + i, byte_val);
                        }

                        App::log(
                            app,
                            format!(
                                "Header field '{}' at 0x{:X} modified to 0x{:X}",
                                app.header_view.edit_name, offset, val
                            ),
                        );

                        // Alignment validation warning
                        if let Some(pe) = &app.header_view.pe {
                            let (sec_align, file_align) = if let Some(opt) = pe.optional_header {
                                (opt.windows_fields.section_alignment.max(1) as u64, opt.windows_fields.file_alignment.max(1) as u64)
                            } else {
                                (0x1000, 0x200)
                            };

                            if app.header_view.edit_name.ends_with(".VirtualAddress") && val % sec_align != 0 {
                                App::log(app, format!("Warning: 0x{:X} is not aligned to SectionAlignment (0x{:X})", val, sec_align));
                            } else if app.header_view.edit_name.ends_with(".PointerToRawData") && val % file_align != 0 {
                                App::log(app, format!("Warning: 0x{:X} is not aligned to FileAlignment (0x{:X})", val, file_align));
                            } else if app.header_view.edit_name == "SectionAlignment" && (val < 0x200 || !val.is_power_of_two()) {
                                App::log(app, format!("Warning: SectionAlignment 0x{:X} is unusual (expected power of 2 >= 0x200)", val));
                            } else if app.header_view.edit_name == "FileAlignment" && (val < 0x200 || !val.is_power_of_two()) {
                                App::log(app, format!("Warning: FileAlignment 0x{:X} is unusual (expected power of 2 >= 0x200)", val));
                            }
                        }

                        // Re-parse header structure
                        app.update_file_headers_scoped(scope);

                        app.state = UIState::Normal;
                        app.goto_input = tui_input::Input::default();
                        app.goto_selection_all = false;
                    } else {
                        let msg = crate::i18n::fill(
                            crate::i18n::M::ErrInvalidNumericValue.tr(app.config.lang),
                            &[&input_str],
                        );
                        app.error(msg);
                    }
                }
            }
            _ => {
                app.goto_selection_all = false;
                if app.header_view.edit_name == "Name" || app.header_view.edit_name.ends_with(".Name") {
                    let val_before = app.goto_input.value().to_string();
                    crate::text_field::handle_key(app, header_edit_field, event);
                    if app.goto_input.value().chars().count() > 8 {
                        app.goto_input = tui_input::Input::new(val_before.chars().take(8).collect::<String>());
                        app.goto_selection_anchor = None;
                    }
                } else {
                    crate::text_field::handle_key(app, header_edit_field, event);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod header_edit_bounds_tests {
    use crate::app::App;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        Some(app)
    }

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: ratatui::crossterm::event::KeyEventState::NONE,
        })
    }

    fn commit(app: &mut App, value: &str) {
        app.goto_input = tui_input::Input::new(value.to_string());
        let _ = super::handle_dialog_header_edit_events(app, &press(KeyCode::Enter));
    }

    #[test]
    fn offset_past_eof_is_refused() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.edit_name = "Fake.Field".to_string();
        app.header_view.edit_offset = 0x7FFF_0000;
        app.header_view.edit_size = 4;

        commit(&mut app, "0x1234");

        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "an edit outside the file was staged; ':w' would grow the file to reach it"
        );
    }

    #[test]
    fn edit_crossing_eof_is_refused() {
        let Some(mut app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        app.header_view.edit_name = "Fake.Field".to_string();
        app.header_view.edit_offset = len - 2;
        app.header_view.edit_size = 4;

        commit(&mut app, "0x11223344");

        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "a field straddling EOF was staged"
        );
    }

    #[test]
    fn offset_near_usize_max_does_not_wrap() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.edit_name = "Fake.Field".to_string();
        app.header_view.edit_offset = usize::MAX - 2;
        app.header_view.edit_size = 8;

        commit(&mut app, "0x1");

        assert!(app.hex_view.changed_bytes.is_empty());
    }

    #[test]
    fn in_bounds_edit_is_still_applied() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.edit_name = "Fake.Field".to_string();
        app.header_view.edit_offset = 0x40;
        app.header_view.edit_size = 4;

        commit(&mut app, "0x11223344");

        assert_eq!(app.hex_view.changed_bytes.get(&0x40).copied(), Some(0x44));
        assert_eq!(app.hex_view.changed_bytes.get(&0x41).copied(), Some(0x33));
        assert_eq!(app.hex_view.changed_bytes.get(&0x42).copied(), Some(0x22));
        assert_eq!(app.hex_view.changed_bytes.get(&0x43).copied(), Some(0x11));
    }

    #[test]
    fn section_name_past_eof_is_refused() {
        let Some(mut app) = loaded_app() else { return };
        let len = app.file_info.buffer_len();
        app.header_view.edit_name = "Section.Name".to_string();
        app.header_view.edit_offset = len - 4;
        app.header_view.edit_size = 8;

        commit(&mut app, ".text");

        assert!(app.hex_view.changed_bytes.is_empty());
    }
}
