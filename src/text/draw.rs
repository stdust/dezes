use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph, Wrap},
};

use crate::app::App;

// FIXME: Show the entire file contents in text view. Currently,
// it only shows up to APP_CACHE_SIZE bytes from the file.
pub fn text_contents_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let buffer = app.file_info.get_buffer();
    let start = app.reader.page_start.min(buffer.len());
    let limit = ((area.height as usize) * (area.width as usize)).min(buffer.len() - start);
    let (text, _) = app.text_view.table.decode_without_bom_handling(&buffer[start..start + limit]);

    let text: String = text
        .chars()
        .map(|c| if c.is_ascii_control() && c != '\n' && c != '\r' && c != '\t' { ' ' } else { c })
        .collect();

    app.text_view.lines_to_show = text.lines().count();
    // Recorded here rather than in `crate::draw`, so the arrows step by the area
    // this function actually decoded from.
    app.text_view.area_height = area.height;
    app.text_view.area_width = area.width;

    let paragraph = Paragraph::new(text)
        .style(app.config.theme.main)
        .wrap(Wrap { trim: true })
        .scroll(app.text_view.scroll_offset);

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
