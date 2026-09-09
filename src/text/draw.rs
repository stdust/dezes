use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::app::App;

pub fn get_visual_lines(app: &App) -> Vec<String> {
    let buffer = app.file_info.get_buffer_ref();
    if buffer.is_empty() {
        return vec![String::new()];
    }
    let start = app.reader.page_start.min(buffer.len());
    let width = app.text_view.area_width.max(1) as usize;
    let height = app.text_view.area_height.max(1) as usize;
    let limit = (height * width).min(buffer.len() - start);
    let (text, _) = app.text_view.table.decode_without_bom_handling(&buffer[start..start + limit]);

    let mut visual_lines = Vec::new();
    for raw_line in text.split('\n') {
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let cleaned: String = raw_line
            .chars()
            .map(|c| if c.is_ascii_control() && c != '\t' { ' ' } else { c })
            .collect();

        if cleaned.is_empty() {
            visual_lines.push(String::new());
        } else {
            let mut cur = String::new();
            let mut cur_w = 0;
            for ch in cleaned.chars() {
                let ch_w = 1;
                if cur_w + ch_w > width {
                    visual_lines.push(cur);
                    cur = String::new();
                    cur_w = 0;
                }
                cur.push(ch);
                cur_w += ch_w;
            }
            if !cur.is_empty() {
                visual_lines.push(cur);
            }
        }
    }

    if visual_lines.is_empty() {
        visual_lines.push(String::new());
    }
    visual_lines
}

pub fn text_contents_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let bg_block = Block::default().style(app.config.theme.main);
    frame.render_widget(bg_block, area);

    let text_area = Rect {
        x: area.x.saturating_add(1),
        y: area.y,
        width: area.width.saturating_sub(2).max(1),
        height: area.height,
    };

    app.text_view.area_height = text_area.height;
    app.text_view.area_width = text_area.width;

    let lines = get_visual_lines(app);
    app.text_view.lines_to_show = lines.len();

    let height = text_area.height.max(1) as usize;
    if !lines.is_empty() {
        if app.text_view.cursor.0 >= lines.len() {
            app.text_view.cursor.0 = lines.len() - 1;
        }
        let cur_line_len = lines[app.text_view.cursor.0].chars().count();
        if app.text_view.cursor.1 > cur_line_len {
            app.text_view.cursor.1 = cur_line_len;
        }
        let max_scroll_y = lines.len().saturating_sub(height) as u16;
        if app.text_view.scroll_offset.0 > max_scroll_y {
            app.text_view.scroll_offset.0 = max_scroll_y;
        }
    }

    let sel = app.text_view.normalized_selection();
    let cursor_line = app.text_view.cursor.0;
    let cursor_col = app.text_view.cursor.1;
    let main_style = app.config.theme.main;
    let highlight_style = app.config.theme.highlight;

    let mut rendered_lines: Vec<Line> = Vec::with_capacity(lines.len());

    for (line_idx, line_str) in lines.iter().enumerate() {
        let char_count = line_str.chars().count();
        if let Some((start_pos, end_pos)) = sel {
            if line_idx < start_pos.0 || line_idx > end_pos.0 {
                rendered_lines.push(Line::from(vec![Span::styled(line_str.to_string(), main_style)]));
            } else if line_idx == start_pos.0 && line_idx == end_pos.0 {
                let s_col = start_pos.1.min(char_count);
                let e_col = end_pos.1.min(char_count);
                let part1: String = line_str.chars().take(s_col).collect();
                let part2: String = line_str.chars().skip(s_col).take(e_col.saturating_sub(s_col)).collect();
                let part3: String = line_str.chars().skip(e_col).collect();
                let mut spans = Vec::new();
                if !part1.is_empty() {
                    spans.push(Span::styled(part1, main_style));
                }
                if !part2.is_empty() {
                    spans.push(Span::styled(part2, highlight_style));
                }
                if !part3.is_empty() {
                    spans.push(Span::styled(part3, main_style));
                }
                if spans.is_empty() {
                    spans.push(Span::styled("", main_style));
                }
                rendered_lines.push(Line::from(spans));
            } else if line_idx == start_pos.0 {
                let s_col = start_pos.1.min(char_count);
                let part1: String = line_str.chars().take(s_col).collect();
                let part2: String = line_str.chars().skip(s_col).collect();
                let mut spans = Vec::new();
                if !part1.is_empty() {
                    spans.push(Span::styled(part1, main_style));
                }
                if !part2.is_empty() {
                    spans.push(Span::styled(part2, highlight_style));
                }
                if spans.is_empty() {
                    spans.push(Span::styled("", main_style));
                }
                rendered_lines.push(Line::from(spans));
            } else if line_idx == end_pos.0 {
                let e_col = end_pos.1.min(char_count);
                let part1: String = line_str.chars().take(e_col).collect();
                let part2: String = line_str.chars().skip(e_col).collect();
                let mut spans = Vec::new();
                if !part1.is_empty() {
                    spans.push(Span::styled(part1, highlight_style));
                }
                if !part2.is_empty() {
                    spans.push(Span::styled(part2, main_style));
                }
                if spans.is_empty() {
                    spans.push(Span::styled("", main_style));
                }
                rendered_lines.push(Line::from(spans));
            } else {
                rendered_lines.push(Line::from(vec![Span::styled(line_str.to_string(), highlight_style)]));
            }
        } else {
            if line_idx == cursor_line {
                let cur_col = cursor_col.min(char_count);
                let part1: String = line_str.chars().take(cur_col).collect();
                let cur_char: String = if cur_col < char_count {
                    line_str.chars().skip(cur_col).take(1).collect()
                } else {
                    " ".to_string()
                };
                let part3: String = if cur_col < char_count {
                    line_str.chars().skip(cur_col + 1).collect()
                } else {
                    String::new()
                };
                let mut spans = Vec::new();
                if !part1.is_empty() {
                    spans.push(Span::styled(part1, main_style));
                }
                spans.push(Span::styled(cur_char, highlight_style));
                if !part3.is_empty() {
                    spans.push(Span::styled(part3, main_style));
                }
                rendered_lines.push(Line::from(spans));
            } else {
                rendered_lines.push(Line::from(vec![Span::styled(line_str.to_string(), main_style)]));
            }
        }
    }

    let paragraph = Paragraph::new(rendered_lines)
        .style(main_style)
        .scroll(app.text_view.scroll_offset);

    frame.render_widget(paragraph, text_area);
}
