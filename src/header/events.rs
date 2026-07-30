use std::io::Result;

use ratatui::crossterm::event::KeyEvent;

use crate::{app::App, header::formats};

pub fn header_view_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    if app.file_info.r#type.starts_with("ELF") || app.header_view.elf.is_some() {
        formats::elf::events::view_header_elf_events(app, key)
    } else if app.file_info.r#type.starts_with("PE") || app.header_view.pe.is_some() {
        formats::pe::events::view_header_pe_events(app, key)
    } else {
        Ok(false)
    }
}
