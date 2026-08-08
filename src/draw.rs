use ratatui::{
    Frame,
    prelude::*,
    widgets::{Clear, Paragraph},
};

use crate::{
    app::App,
    editor::{AppView, UIState},
    global, header,
    hex::{self, comment},
    ruler, text,
};

/// This is the main drawing/rendering function that
/// draws the layout areas and renders all Ratatui
/// widgets by calling the right functions to do so.
/// It is passed as callback function to terminal.draw()
/// in the main() loop.
pub fn draw(frame: &mut Frame, app: &mut App) {
    if frame.area().width < 68 || frame.area().height < 10 {
        let err = Paragraph::new("dezes needs at least a 68x10 terminal.");
        frame.render_widget(err, frame.area());
        return;
    }

    // The easter egg owns the whole screen: no ruler, no status bar, no theme.
    if app.state == UIState::Matrix {
        global::matrix::draw(app, frame);
        return;
    }

    // Fill entire terminal background with theme's main background color
    let bg_fill = Paragraph::new("").style(app.config.theme.main);
    frame.render_widget(bg_fill, frame.area());

    // Draw things depending on the view
    match app.editor_view {
        AppView::Hex => {
            let constraints = vec![
                Constraint::Length(1),       // ruler
                Constraint::Percentage(100), // middle area (hex content)
                Constraint::Length(1),       // status bar
                Constraint::Length(1),       // command bar
            ];

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            // Draw ruler at the top
            ruler::ruler_draw(app, frame, vertical_layout[0]);

            // Draw status bar at the bottom
            global::status_bar::status_bar_draw(app, frame, vertical_layout[2]);

            app.command_area = vertical_layout[3];

            let addr_col_width = app.get_addr_col_width() as u16;
            // `max(1)` + `saturating_sub`: `:set byteline 0` used to make this
            // `0 * 3 - 1` and panic on the next frame.
            let bpl = (app.config.hex_mode_bytes_per_line.max(1) as u16).min(u16::MAX / 3);
            let hex_width = (bpl * 3).saturating_sub(1);

            let enc1_name_len = (app.text_view.table.name().len() + 2) as u16;
            let enc1_width = bpl.max(enc1_name_len);

            if let Some(enc2_table) = app.hex_view.enc2_table {
                let enc2_name_len = (enc2_table.name().len() + 2) as u16;
                let enc2_width = bpl.max(enc2_name_len);

                let horizontal_layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![
                        Constraint::Length(addr_col_width),
                        Constraint::Length(1), // vertical separator 1
                        Constraint::Length(hex_width),
                        Constraint::Length(1), // vertical separator 2
                        Constraint::Length(enc1_width),
                        Constraint::Length(1), // vertical separator 3
                        Constraint::Length(enc2_width),
                        Constraint::Length(1), // vertical separator 4 (right of enc2)
                        Constraint::Min(0),    // remaining space
                    ])
                    .split(vertical_layout[1]);

                let sep_height = horizontal_layout[0].height as usize;
                let bg_color = app.config.theme.main.bg.unwrap_or(Color::Reset);
                let sep_style = Style::default().fg(Color::Rgb(90, 100, 115)).bg(bg_color);
                // Built once and borrowed by all four separators, instead of
                // cloning the String three extra times every frame.
                let sep_str = "│\n".repeat(sep_height);
                let sep_para1 = Paragraph::new(sep_str.as_str()).style(sep_style);
                let sep_para2 = Paragraph::new(sep_str.as_str()).style(sep_style);
                let sep_para3 = Paragraph::new(sep_str.as_str()).style(sep_style);
                let sep_para4 = Paragraph::new(sep_str.as_str()).style(sep_style);

                hex::draw::draw_hex_offsets(app, frame, horizontal_layout[0]);
                frame.render_widget(sep_para1, horizontal_layout[1]);
                hex::draw::draw_hex_contents(app, frame, horizontal_layout[2]);
                frame.render_widget(sep_para2, horizontal_layout[3]);
                hex::draw::draw_hex_ascii(app, frame, horizontal_layout[4], false);
                frame.render_widget(sep_para3, horizontal_layout[5]);
                hex::draw::draw_hex_ascii(app, frame, horizontal_layout[6], true);
                frame.render_widget(sep_para4, horizontal_layout[7]);
            } else {
                let horizontal_layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![
                        Constraint::Length(addr_col_width),
                        Constraint::Length(1), // vertical separator 1
                        Constraint::Length(hex_width),
                        Constraint::Length(1), // vertical separator 2
                        Constraint::Length(enc1_width),
                        Constraint::Length(1), // vertical separator 3 (right of enc1)
                        Constraint::Min(0),    // remaining space
                    ])
                    .split(vertical_layout[1]);

                let sep_height = horizontal_layout[0].height as usize;
                let bg_color = app.config.theme.main.bg.unwrap_or(Color::Reset);
                let sep_style = Style::default().fg(Color::Rgb(90, 100, 115)).bg(bg_color);
                let sep_str = "│\n".repeat(sep_height);
                let sep_para1 = Paragraph::new(sep_str.as_str()).style(sep_style);
                let sep_para2 = Paragraph::new(sep_str.as_str()).style(sep_style);
                let sep_para3 = Paragraph::new(sep_str.as_str()).style(sep_style);

                hex::draw::draw_hex_offsets(app, frame, horizontal_layout[0]);
                frame.render_widget(sep_para1, horizontal_layout[1]);
                hex::draw::draw_hex_contents(app, frame, horizontal_layout[2]);
                frame.render_widget(sep_para2, horizontal_layout[3]);
                hex::draw::draw_hex_ascii(app, frame, horizontal_layout[4], false);
                frame.render_widget(sep_para3, horizontal_layout[5]);
            }
            comment::comment_show_draw(app, frame);
        }
        AppView::Text => {
            let constraints = vec![
                Constraint::Percentage(100), // middle area (text content)
                Constraint::Length(1),       // status bar
                Constraint::Length(1),       // command bar
            ];

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            // Draw status bar at the bottom
            global::status_bar::status_bar_draw(app, frame, vertical_layout[1]);

            app.command_area = vertical_layout[2];

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Percentage(100)])
                .split(vertical_layout[0]);

            text::draw::text_contents_draw(app, frame, horizontal_layout[0]);
            app.text_view.area_height = horizontal_layout[0].height;
        }
        AppView::Header => {
            let constraints = vec![
                Constraint::Percentage(100), // middle area (text content)
                Constraint::Length(1),       // status bar
                Constraint::Length(1),       // command bar
            ];

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            // Draw status bar at the bottom
            global::status_bar::status_bar_draw(app, frame, vertical_layout[1]);

            app.command_area = vertical_layout[2];

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Percentage(100)])
                .split(vertical_layout[0]);

            header::draw::header_contents_draw(app, frame, horizontal_layout[0]);
            app.text_view.area_height = horizontal_layout[0].height;
        }
        AppView::Disasm => {
            let constraints = vec![
                Constraint::Length(1),       // ruler header
                Constraint::Min(0),          // middle area (disasm content)
                Constraint::Length(1),       // status bar
                Constraint::Length(1),       // command bar
            ];

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            ruler::ruler_draw(app, frame, vertical_layout[0]);

            // Draw status bar at the bottom
            global::status_bar::status_bar_draw(app, frame, vertical_layout[2]);

            app.command_area = vertical_layout[3];

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Percentage(100)])
                .split(vertical_layout[1]);

            crate::disasm::draw::draw_disasm_view(app, frame, horizontal_layout[0]);
        }
    }

    // The right event handler function is set by the keypress
    // for example, in hex/events.rs, F5 (Goto) will set app.dialog_renderer
    // to Some(global::goto::draw_hex_dialog_goto). The code below just executes
    // the function pointed by this field if there's any.
    if app.state == UIState::DialogReplacePattern {
        crate::hex::replace_dialog::draw_replace_dialog(app, frame, frame.area());
    } else if app.state == UIState::DialogHeaderEdit {
        header::edit_dialog::draw_header_edit_dialog(app, frame);
    } else if let Some(f) = app.dialog_renderer {
        f(app, frame);
    }

    // The replacement box is a second layer over the strings list, so the row being
    // replaced stays visible behind it.
    if app.state == UIState::DialogStringEdit
        && let Some(f) = app.dialog_renderer
    {
        f(app, frame);
    }
    if let Some(f) = app.dialog_2nd_renderer {
        f(app, frame);
    }

    // Refusal messages (read-only shortcuts, failed saves) own the command bar
    // until the next key press. Drawn last so it also covers a stale
    // `dialog_renderer` that renders into the same one-line area.
    if let Some(message) = app.status_error.clone() {
        let para = Paragraph::new(message).style(app.config.theme.error);
        frame.render_widget(Clear, app.command_area);
        frame.render_widget(para, app.command_area);
    } else if global::hint_bar::should_show(app) {
        // Last claim on the command-bar row: the command line, dialogs and messages
        // all get it first. See `global/hint_bar.rs`.
        let area = app.command_area;
        global::hint_bar::hint_bar_draw(app, frame, area);
    }
}
