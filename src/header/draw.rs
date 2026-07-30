use ratatui::{Frame, layout::Rect};

use crate::{app::App, header::formats};

pub fn header_contents_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    if app.file_info.r#type.starts_with("PE") || app.header_view.pe.is_some() {
        let _ = formats::pe::draw::pe_draw(app, frame, area);
    } else if app.file_info.r#type.starts_with("ELF") || app.header_view.elf.is_some() {
        let _ = formats::elf::draw::elf_draw(app, frame, area);
    }
}
