use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    widgets::{Block, Clear, Paragraph},
};

use crate::{app::App, header::formats, i18n::M};

pub fn header_contents_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    if app.file_info.r#type.starts_with("PE") || app.header_view.pe.is_some() {
        formats::pe::draw::pe_draw(app, frame, area);
    } else if app.file_info.r#type.starts_with("ELF") || app.header_view.elf.is_some() {
        formats::elf::draw::elf_draw(app, frame, area);
    } else {
        frame.render_widget(Clear, area);
        let block = Block::default().style(app.config.theme.main);
        frame.render_widget(block, area);

        let msg = M::ErrNoPEHeader.tr(app.config.lang);
        let para = Paragraph::new(msg)
            .alignment(Alignment::Center)
            .style(app.config.theme.dimmed);
        let centered_area = Rect {
            x: area.x,
            y: area.y + area.height / 2,
            width: area.width,
            height: 1,
        };
        frame.render_widget(para, centered_area);
    }
}
