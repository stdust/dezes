use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Clear, Paragraph},
};

use std::fmt::Write;

use crate::app::App;

pub fn ruler_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let pad_len = app.get_addr_col_width() as u16;
    let bpl = app.config.hex_mode_bytes_per_line.max(1);

    // `bpl` is user-settable via `:set byteline`, so every width derived from it
    // is converted saturatingly rather than with `as u16`, which would wrap a
    // large value round to a small one and scramble the header layout.
    let bpl_u16 = u16::try_from(bpl).unwrap_or(u16::MAX);

    let enc1_name_len = u16::try_from(app.text_view.table.name().len() + 2).unwrap_or(u16::MAX);
    let enc1_width = bpl_u16.max(enc1_name_len);

    // Widths derived from bytes-per-line rather than hardcoded to 23. For the
    // default bpl of 16 these evaluate to exactly 23 and 23, so the ruler is
    // unchanged; for any other bpl the header text now lines up with the dump
    // instead of being truncated or padded.
    let hex_part1_len = u16::try_from(bpl.min(8) * 3 - 1).unwrap_or(u16::MAX); // "00 .. 07"
    let hex_part2_len = if bpl > 8 {
        u16::try_from((bpl - 8) * 3 - 1).unwrap_or(u16::MAX) // "08 .. 0F"
    } else {
        0
    };

    let is_dual = app.hex_view.enc2_table.is_some();
    let enc2_table = app.hex_view.get_enc2_table();
    let enc2_name_len = u16::try_from(enc2_table.name().len() + 2).unwrap_or(u16::MAX);
    let enc2_width = bpl_u16.max(enc2_name_len);

    let header_layout = if is_dual {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Length(pad_len),
                Constraint::Length(1),             // separator 1 (│)
                Constraint::Length(hex_part1_len), // 00..07
                Constraint::Length(1),             // mid separator (07-08 │)
                Constraint::Length(hex_part2_len), // 08..0F
                Constraint::Length(1),             // separator 2 (│)
                Constraint::Length(enc1_width),    // Encoding 1 Header
                Constraint::Length(1),             // separator 3 (│)
                Constraint::Length(enc2_width),    // Encoding 2 Header
                Constraint::Length(1),             // separator 4 (│)
                Constraint::Min(0),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Length(pad_len),
                Constraint::Length(1),             // separator 1 (│)
                Constraint::Length(hex_part1_len), // 00..07
                Constraint::Length(1),             // mid separator (07-08 │)
                Constraint::Length(hex_part2_len), // 08..0F
                Constraint::Length(1),             // separator 2 (│)
                Constraint::Length(enc1_width),    // Encoding 1 Header
                Constraint::Length(1),             // separator 3 (│)
                Constraint::Min(0),
            ])
            .split(area)
    };

    // Header style: Dynamic topbar style from .theme file
    let header_style = app.config.theme.topbar.add_modifier(Modifier::BOLD);

    let sep_header_bg = app.config.theme.topbar.bg.unwrap_or(Color::Rgb(180, 188, 198));
    let sep_header_fg = app.config.theme.dimmed.fg.unwrap_or(Color::Rgb(90, 100, 115));
    let sep_header_style = Style::default()
        .bg(sep_header_bg)
        .fg(sep_header_fg)
        .add_modifier(Modifier::BOLD);

    if app.editor_view == crate::editor::AppView::Disasm {
        // Shared with disasm::draw so the ruler and the table can't drift apart.
        let va_len = crate::disasm::draw::va_col_width(app.is_64());
        let bytes_len = crate::disasm::draw::BYTES_COL_WIDTH;
        let disasm_len = crate::disasm::draw::DISASM_COL_WIDTH;

        let disasm_header_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Length(va_len),
                Constraint::Length(1), // separator 1
                Constraint::Length(bytes_len),
                Constraint::Length(1), // separator 2
                Constraint::Length(disasm_len), // instruction column
                Constraint::Length(1), // separator 3
                Constraint::Min(0),   // Comment column
            ])
            .split(area);

        let addr_title = "Address";
        let addr_str = format!("{:^width$}", addr_title, width = va_len as usize);
        let addr_para = Paragraph::new(addr_str).style(header_style);

        let bytes_title = "Hex dump";
        let bytes_str = format!(" {:<width$}", bytes_title, width = (bytes_len - 1) as usize);
        let bytes_para = Paragraph::new(bytes_str).style(header_style);

        let disasm_title = "Disassembly";
        let disasm_str = format!(" {:<width$}", disasm_title, width = (disasm_len - 1) as usize);
        let disasm_para = Paragraph::new(disasm_str).style(header_style);

        let comment_title = "Comment";
        let comment_para = Paragraph::new(format!(" {}", comment_title)).style(header_style);

        let sep1 = Paragraph::new("│").style(sep_header_style);
        let sep2 = Paragraph::new("│").style(sep_header_style);
        let sep3 = Paragraph::new("│").style(sep_header_style);

        let bg_fill = Paragraph::new(" ".repeat(area.width as usize)).style(header_style);
        frame.render_widget(Clear, area);
        frame.render_widget(bg_fill, area);

        frame.render_widget(addr_para, disasm_header_layout[0]);
        frame.render_widget(sep1, disasm_header_layout[1]);
        frame.render_widget(bytes_para, disasm_header_layout[2]);
        frame.render_widget(sep2, disasm_header_layout[3]);
        frame.render_widget(disasm_para, disasm_header_layout[4]);
        frame.render_widget(sep3, disasm_header_layout[5]);
        frame.render_widget(comment_para, disasm_header_layout[6]);
        return;
    }

    // 1. Offset / VA title
    let addr_title = if app.hex_view.show_va { "VA" } else { "Offset" };
    let addr_str = format!("{:^width$}", addr_title, width = pad_len as usize);
    let addr_para = Paragraph::new(addr_str).style(header_style);

    // 2. Hex column headers (Part 1: 00 01 02 03 04 05 06 07, Part 2: 08 09 0A 0B 0C 0D 0E 0F)
    let (hex_part1, hex_part2) = if bpl == 16 {
        // Fast path for the default: two borrowed literals, no allocation.
        (
            std::borrow::Cow::Borrowed("00 01 02 03 04 05 06 07"),
            std::borrow::Cow::Borrowed("08 09 0A 0B 0C 0D 0E 0F"),
        )
    } else {
        let group1 = bpl.min(8);
        let mut p1 = String::with_capacity(group1 * 3);
        for i in 0..group1 {
            if i == 7 || i == bpl - 1 {
                let _ = write!(p1, "{:02X}", i);
            } else {
                let _ = write!(p1, "{:02X} ", i);
            }
        }
        let mut p2 = String::with_capacity(bpl.saturating_sub(8) * 3);
        if bpl > 8 {
            for i in 8..bpl {
                if i == bpl - 1 {
                    let _ = write!(p2, "{:02X}", i);
                } else {
                    let _ = write!(p2, "{:02X} ", i);
                }
            }
        }
        (std::borrow::Cow::Owned(p1), std::borrow::Cow::Owned(p2))
    };
    let hex_para1 = Paragraph::new(hex_part1).style(header_style);
    let hex_para2 = Paragraph::new(hex_part2).style(header_style);

    // 3. Encoding titles (Enc 1 and Enc 2)
    let enc1_title = app.text_view.table.name();
    let enc1_para = Paragraph::new(enc1_title).alignment(Alignment::Center).style(header_style);

    let enc2_title = enc2_table.name();
    let enc2_para = Paragraph::new(enc2_title).alignment(Alignment::Center).style(header_style);

    let sep1 = Paragraph::new("│").style(sep_header_style);
    let mid_sep = Paragraph::new("│").style(sep_header_style);
    let sep2 = Paragraph::new("│").style(sep_header_style);
    let sep3 = Paragraph::new("│").style(sep_header_style);
    let sep4 = Paragraph::new("│").style(sep_header_style);

    let bg_fill = Paragraph::new(" ".repeat(area.width as usize)).style(header_style);
    frame.render_widget(Clear, area);
    frame.render_widget(bg_fill, area);

    frame.render_widget(addr_para, header_layout[0]);
    frame.render_widget(sep1, header_layout[1]);
    frame.render_widget(hex_para1, header_layout[2]);
    frame.render_widget(mid_sep, header_layout[3]);
    frame.render_widget(hex_para2, header_layout[4]);
    frame.render_widget(sep2, header_layout[5]);
    frame.render_widget(enc1_para, header_layout[6]);
    frame.render_widget(sep3, header_layout[7]);

    if is_dual {
        frame.render_widget(enc2_para, header_layout[8]);
        frame.render_widget(sep4, header_layout[9]);
    }
}
