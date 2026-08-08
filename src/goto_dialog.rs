use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers},
    layout::Alignment,
    widgets::{Block, Clear, Paragraph},
};
use std::io::Result;
use crate::{app::App, editor::UIState, util::center_widget};

fn safe_slice_parts(text: &str, start_pos: usize, end_pos: usize) -> (&str, &str, &str) {
    let char_indices: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    let total_chars = char_indices.len();

    let s = start_pos.min(end_pos).min(total_chars);
    let e = start_pos.max(end_pos).min(total_chars);

    let b_start = if s < total_chars { char_indices[s] } else { text.len() };
    let b_end = if e < total_chars { char_indices[e] } else { text.len() };

    (&text[..b_start], &text[b_start..b_end], &text[b_end..])
}

pub fn dialog_goto_draw(app: &mut App, frame: &mut Frame) {
    let width = 32.min(frame.area().width.saturating_sub(4)).max(25);
    let height = 3;
    let mut dialog_area = center_widget(width, height, frame.area());
    dialog_area.y = dialog_area.y.saturating_sub(4);

    frame.render_widget(Clear, dialog_area);

    let input_text = app.goto_input.value();
    let cursor_pos = app.goto_input.cursor();

    let paragraph = if app.goto_selection_all && !input_text.is_empty() {
        use ratatui::text::{Line, Span};
        let line = Line::from(vec![
            Span::styled(input_text.to_string(), app.config.theme.highlight),
        ]);
        Paragraph::new(line)
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::GotoTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    } else if let Some(anchor) = app.goto_selection_anchor {
        use ratatui::text::{Line, Span};
        let (before, selected, after) = safe_slice_parts(input_text, anchor, cursor_pos);

        let line = Line::from(vec![
            Span::styled(before.to_string(), app.config.theme.dialog),
            Span::styled(selected.to_string(), app.config.theme.highlight),
            Span::styled(after.to_string(), app.config.theme.dialog),
        ]);
        Paragraph::new(line)
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::GotoTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    } else {
        Paragraph::new(input_text.to_string())
            .style(app.config.theme.dialog)
            .block(
                Block::bordered()
                    .title(crate::i18n::M::GotoTitle.tr(app.config.lang))
                    .title_alignment(Alignment::Center),
            )
    };

    frame.render_widget(paragraph, dialog_area);

    let cursor_x = dialog_area.x + 1 + app.goto_input.cursor() as u16;
    let cursor_y = dialog_area.y + 1;
    if cursor_x < dialog_area.x + dialog_area.width - 1 {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

pub fn dialog_goto_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl+C: Copy selected or full text to clipboard
        if is_ctrl && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C')) {
            let input_val = app.goto_input.value();
            let text_to_copy = if app.goto_selection_all {
                input_val.to_string()
            } else if let Some(anchor) = app.goto_selection_anchor {
                let cursor = app.goto_input.cursor();
                let (_, selected, _) = safe_slice_parts(input_val, anchor, cursor);
                selected.to_string()
            } else {
                input_val.to_string()
            };

            if !text_to_copy.is_empty() {
                if let Ok(cb) = &mut app.clipboard {
                    let _ = cb.set_text(text_to_copy);
                    App::log(app, "Copied address text to clipboard".to_string());
                }
            }
            return Ok(false);
        }

        // Ctrl+V: Paste text from clipboard
        if is_ctrl && (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V')) {
            if let Ok(cb) = &mut app.clipboard {
                if let Ok(pasted) = cb.get_text() {
                    let clean_pasted = pasted.trim().replace('\n', "").replace('\r', "");
                    let pasted_char_cnt = clean_pasted.chars().count();
                    if app.goto_selection_all {
                        app.goto_selection_all = false;
                        app.goto_selection_anchor = None;
                        app.goto_input = tui_input::Input::new(clean_pasted);
                    } else if let Some(anchor) = app.goto_selection_anchor {
                        let cursor = app.goto_input.cursor();
                        let val = app.goto_input.value();
                        let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                        let before_char_cnt = before.chars().count();
                        let mut new_val = String::new();
                        new_val.push_str(before);
                        new_val.push_str(&clean_pasted);
                        new_val.push_str(after);
                        let new_cursor = before_char_cnt + pasted_char_cnt;
                        app.goto_selection_anchor = None;
                        app.goto_input = tui_input::Input::new(new_val).with_cursor(new_cursor);
                    } else {
                        let cursor = app.goto_input.cursor();
                        let val = app.goto_input.value();
                        let (before, _, after) = safe_slice_parts(val, cursor, cursor);
                        let before_char_cnt = before.chars().count();
                        let mut new_val = String::new();
                        new_val.push_str(before);
                        new_val.push_str(&clean_pasted);
                        new_val.push_str(after);
                        let new_cursor = before_char_cnt + pasted_char_cnt;
                        app.goto_input = tui_input::Input::new(new_val).with_cursor(new_cursor);
                    }
                }
            }
            return Ok(false);
        }

        // Handle Shift + Left / Right / Home / End selection
        if is_shift {
            let cursor = app.goto_input.cursor();
            let val_char_len = app.goto_input.value().chars().count();
            if app.goto_selection_anchor.is_none() {
                app.goto_selection_anchor = Some(cursor);
            }
            app.goto_selection_all = false;

            match key.code {
                KeyCode::Left => {
                    let new_cursor = cursor.saturating_sub(1);
                    app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(new_cursor);
                    return Ok(false);
                }
                KeyCode::Right => {
                    let new_cursor = (cursor + 1).min(val_char_len);
                    app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(new_cursor);
                    return Ok(false);
                }
                KeyCode::Home => {
                    app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(0);
                    return Ok(false);
                }
                KeyCode::End => {
                    app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(val_char_len);
                    return Ok(false);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            KeyCode::Enter => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                let raw_input = app.goto_input.value().trim();
                if let Some(addr) = crate::commands::eval_address_expression(app, raw_input) {
                    let filesize = app.file_info.size;
                    let target_offset = crate::commands::address_to_offset(app, addr)
                        .unwrap_or(addr as usize);

                    if target_offset < filesize {
                        app.reader.page_start = target_offset;
                        app.goto(target_offset);
                        App::log(app, format!("Jumped to address 0x{:X} (offset 0x{:X})", addr, target_offset));
                        app.state = UIState::Normal;
                        app.dialog_renderer = None;
                    } else {
                        crate::beep!();
                        app.error(format!("Address 0x{:X} out of bounds", addr));
                    }
                } else {
                    crate::beep!();
                    app.error(format!("Invalid address expression: '{}'", raw_input));
                }
            }
            KeyCode::Home => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(0);
            }
            KeyCode::End => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                let val_char_len = app.goto_input.value().chars().count();
                app.goto_input = tui_input::Input::new(app.goto_input.value().to_string()).with_cursor(val_char_len);
            }
            KeyCode::Left => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.goto_input, event);
            }
            KeyCode::Right => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.goto_input, event);
            }
            KeyCode::Char(c) if app.goto_selection_all => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                app.goto_input = tui_input::Input::new(c.to_string());
            }
            KeyCode::Char(c) if app.goto_selection_anchor.is_some() => {
                if let Some(anchor) = app.goto_selection_anchor {
                    let cursor = app.goto_input.cursor();
                    let val = app.goto_input.value();
                    let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                    let before_char_cnt = before.chars().count();
                    let mut new_val = String::new();
                    new_val.push_str(before);
                    new_val.push(c);
                    new_val.push_str(after);
                    app.goto_selection_anchor = None;
                    app.goto_input = tui_input::Input::new(new_val).with_cursor(before_char_cnt + 1);
                }
            }
            KeyCode::Backspace | KeyCode::Delete if app.goto_selection_all => {
                app.goto_selection_all = false;
                app.goto_selection_anchor = None;
                app.goto_input = tui_input::Input::default();
            }
            KeyCode::Backspace | KeyCode::Delete if app.goto_selection_anchor.is_some() => {
                if let Some(anchor) = app.goto_selection_anchor {
                    let cursor = app.goto_input.cursor();
                    let val = app.goto_input.value();
                    let (before, _, after) = safe_slice_parts(val, anchor, cursor);
                    let before_char_cnt = before.chars().count();
                    let mut new_val = String::new();
                    new_val.push_str(before);
                    new_val.push_str(after);
                    app.goto_selection_anchor = None;
                    app.goto_input = tui_input::Input::new(new_val).with_cursor(before_char_cnt);
                }
            }
            _ => {
                if app.goto_selection_all {
                    app.goto_selection_all = false;
                }
                if app.goto_selection_anchor.is_some() {
                    app.goto_selection_anchor = None;
                }
                tui_input::backend::crossterm::EventHandler::handle_event(&mut app.goto_input, event);
            }
        }
    }
    Ok(false)
}
