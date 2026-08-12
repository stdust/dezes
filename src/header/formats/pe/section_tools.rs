use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyModifiers},
    layout::Alignment,
    widgets::{Block, Clear, Paragraph},
};
use std::io::Result;

use crate::app::App;
use crate::editor::UIState;
use crate::header::header_view::Pe;

const DEFAULT_SECTION_CHARACTERISTICS: u32 = 0xC000_0040;

mod field {
    pub const NAME: usize = 0;
    pub const VIRTUAL_SIZE: usize = 8;
    pub const VIRTUAL_ADDRESS: usize = 12;
    pub const SIZE_OF_RAW_DATA: usize = 16;
    pub const POINTER_TO_RAW_DATA: usize = 20;
    pub const POINTER_TO_RELOCATIONS: usize = 24;
    pub const POINTER_TO_LINENUMBERS: usize = 28;
    pub const NUMBER_OF_RELOCATIONS: usize = 32;
    pub const NUMBER_OF_LINENUMBERS: usize = 34;
    pub const CHARACTERISTICS: usize = 36;
    pub const ENTRY_SIZE: usize = 40;
}

fn align_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    value.div_ceil(align) * align
}

fn write_u16(app: &mut App, offset: usize, value: u16) {
    for (i, b) in value.to_le_bytes().iter().enumerate() {
        crate::hex::edit::record_edit(app, offset + i, *b);
    }
}

fn write_u32(app: &mut App, offset: usize, value: u32) {
    for (i, b) in value.to_le_bytes().iter().enumerate() {
        crate::hex::edit::record_edit(app, offset + i, *b);
    }
}

fn write_bytes(app: &mut App, offset: usize, bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        crate::hex::edit::record_edit(app, offset + i, *b);
    }
}

fn section_header_offset(pe: &Pe, section_index: usize) -> usize {
    let pe_ptr = pe.dos_header.pe_pointer as usize;
    pe_ptr + 24 + pe.coff_header.size_of_optional_header as usize + section_index * field::ENTRY_SIZE
}

pub fn align_offset_to_va(app: &mut App, section_index: usize) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return;
    }

    let Some((sec_name, new_offset, old_offset, sec_base)) = app.header_view.pe.as_ref().and_then(|pe| {
        let section = pe.sections.get(section_index)?;
        Some((
            section.name().unwrap_or("section").to_string(),
            section.virtual_address,
            section.pointer_to_raw_data,
            section_header_offset(pe, section_index),
        ))
    }) else {
        let message = crate::i18n::fill(
            crate::i18n::M::ErrNoSectionPicked.tr(app.config.lang),
            &[&section_index.to_string()],
        );
        app.header_view.tools_last_message = Some(message.clone());
        app.error(message);
        return;
    };

    write_u32(app, sec_base + field::POINTER_TO_RAW_DATA, new_offset);

    let msg = format!(
        "Done: '{}'.PointerToRawData set to 0x{:X} (was 0x{:X})",
        sec_name, new_offset, old_offset
    );
    App::log(app, msg.clone());
    app.header_view.tools_last_message = Some(msg);
    app.update_file_headers();
}

pub fn add_new_section(app: &mut App, requested_size: u32) -> std::result::Result<(), String> {
    let lang = app.config.lang;
    if requested_size == 0 {
        return Err(crate::i18n::M::ErrSectionSizeZero.tr(lang).to_string());
    }

    struct Plan {
        new_sec_base: usize,
        coff_off: usize,
        opt_off: usize,
        number_of_sections: u16,
        new_va: u64,
        raw_size: u64,
        new_raw_offset: u64,
        new_size_of_image: u64,
    }

    let plan = {
        let Some(pe) = &app.header_view.pe else {
            return Err(crate::i18n::M::ErrNoPeHeaders.tr(lang).to_string());
        };
        let Some(opt) = &pe.optional_header else {
            return Err(crate::i18n::M::ErrNoOptionalHeader.tr(lang).to_string());
        };

        let section_alignment = opt.windows_fields.section_alignment.max(1) as u64;
        let file_alignment = opt.windows_fields.file_alignment.max(1) as u64;
        let size_of_headers = opt.windows_fields.size_of_headers as u64;
        let size_of_image = opt.windows_fields.size_of_image as u64;
        let opt_off = pe.dos_header.pe_pointer as usize + 24;

        let nsections = pe.sections.len();
        let new_sec_base = section_header_offset(pe, nsections);

        if (new_sec_base + field::ENTRY_SIZE) as u64 > size_of_headers {
            return Err(crate::i18n::fill(
                crate::i18n::M::ErrNoRoomForSectionHeader.tr(lang),
                &[&format!("{:X}", size_of_headers)],
            ));
        }

        let (prev_va_end, prev_raw_end) = match pe.sections.last() {
            Some(last) => (
                last.virtual_address as u64 + (last.virtual_size as u64).max(last.size_of_raw_data as u64),
                last.pointer_to_raw_data as u64 + last.size_of_raw_data as u64,
            ),
            None => (size_of_headers, size_of_headers),
        };

        let new_va = align_up(prev_va_end, section_alignment);
        let raw_size = align_up(requested_size as u64, file_alignment);

        let current_len = app.file_info.buffer_len() as u64;
        let new_raw_offset = align_up(prev_raw_end.max(current_len), file_alignment);

        if new_va > u32::MAX as u64 || new_raw_offset > u32::MAX as u64 || raw_size > u32::MAX as u64 {
            return Err(crate::i18n::M::ErrSectionTooBig.tr(lang).to_string());
        }

        let new_size_of_image =
            align_up((new_va + raw_size.max(requested_size as u64)).max(size_of_image), section_alignment);

        Plan {
            new_sec_base,
            coff_off: pe.dos_header.pe_pointer as usize,
            opt_off,
            number_of_sections: pe.coff_header.number_of_sections,
            new_va,
            raw_size,
            new_raw_offset,
            new_size_of_image,
        }
    };

    let mut name_bytes = [0u8; 8];
    name_bytes[..4].copy_from_slice(b".new");
    write_bytes(app, plan.new_sec_base + field::NAME, &name_bytes);
    write_u32(app, plan.new_sec_base + field::VIRTUAL_SIZE, requested_size);
    write_u32(app, plan.new_sec_base + field::VIRTUAL_ADDRESS, plan.new_va as u32);
    write_u32(app, plan.new_sec_base + field::SIZE_OF_RAW_DATA, plan.raw_size as u32);
    write_u32(app, plan.new_sec_base + field::POINTER_TO_RAW_DATA, plan.new_raw_offset as u32);
    write_u32(app, plan.new_sec_base + field::POINTER_TO_RELOCATIONS, 0);
    write_u32(app, plan.new_sec_base + field::POINTER_TO_LINENUMBERS, 0);
    write_u16(app, plan.new_sec_base + field::NUMBER_OF_RELOCATIONS, 0);
    write_u16(app, plan.new_sec_base + field::NUMBER_OF_LINENUMBERS, 0);
    write_u32(app, plan.new_sec_base + field::CHARACTERISTICS, DEFAULT_SECTION_CHARACTERISTICS);

    write_u16(app, plan.coff_off + 6, plan.number_of_sections + 1);

    write_u32(app, plan.opt_off + 56, plan.new_size_of_image as u32);

    let current_len = app.file_info.buffer_len() as u64;
    let target_len = plan.new_raw_offset + plan.raw_size;
    let extension_len = target_len.saturating_sub(current_len);
    if extension_len > 0 {
        app.file_info.stage_extension(&vec![0u8; extension_len as usize]);
    }

    let msg = format!(
        "Done: added section '.new' (VA=0x{:X}, Size=0x{:X}, RawOffset=0x{:X})",
        plan.new_va, requested_size, plan.new_raw_offset
    );
    App::log(app, msg.clone());
    app.header_view.tools_last_message = Some(msg);

    app.update_file_headers();
    Ok(())
}

fn parse_size(input: &str) -> Option<u32> {
    let clean = input.trim();
    let hex_digits = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")).unwrap_or(clean);
    u32::from_str_radix(hex_digits, 16).ok().filter(|&v| v > 0)
}

pub fn draw_section_size_dialog(app: &mut App, frame: &mut Frame) {
    let width = 40.min(frame.area().width.saturating_sub(4)).max(28);
    let height = if app.header_view.section_size_dialog.error_message.is_some() { 4 } else { 3 };
    let dialog_area = crate::hex::field_box::centered_rect_above(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);

    let dialog = &app.header_view.section_size_dialog;
    let input_text = dialog.input.value();

    let mut body = String::new();
    if let Some(err) = &dialog.error_message {
        body.push_str(err);
        body.push('\n');
    }

    let block = Block::bordered()
        .title(crate::i18n::M::AddSectionTitle.tr(app.config.lang))
        .title_alignment(Alignment::Center);

    let paragraph = if dialog.selection_all && !input_text.is_empty() {
        use ratatui::text::{Line, Span};
        let line = Line::from(vec![Span::styled(input_text.to_string(), app.config.theme.highlight)]);
        Paragraph::new(vec![Line::raw(body.clone()), line])
            .style(app.config.theme.dialog)
            .block(block)
    } else if dialog.selection_anchor.is_some() {
        use ratatui::text::Line;
        let line = crate::text_field::render_line(
            &dialog.input,
            dialog.selection_anchor,
            app.config.theme.dialog,
            app.config.theme.highlight,
        );
        Paragraph::new(vec![Line::raw(body.clone()), line])
            .style(app.config.theme.dialog)
            .block(block)
    } else {
        Paragraph::new(format!("{}{}", body, input_text))
            .style(app.config.theme.dialog)
            .block(block)
    };

    frame.render_widget(paragraph, dialog_area);

    let text_row = if app.header_view.section_size_dialog.error_message.is_some() { 2 } else { 1 };
    let cursor_x = dialog_area.x + 1 + app.header_view.section_size_dialog.input.cursor() as u16;
    let cursor_y = dialog_area.y + text_row;
    if cursor_x < dialog_area.x + dialog_area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn section_size_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    let dialog = &mut app.header_view.section_size_dialog;
    (&mut dialog.input, &mut dialog.selection_anchor)
}

pub fn dialog_section_size_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        if key.modifiers.contains(KeyModifiers::SHIFT) {
            app.header_view.section_size_dialog.selection_all = false;
        }

        match key.code {
            KeyCode::Esc => {
                app.header_view.section_size_dialog = Default::default();
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            KeyCode::Enter => {
                let raw = app.header_view.section_size_dialog.input.value().to_string();
                match parse_size(&raw) {
                    Some(size) => match add_new_section(app, size) {
                        Ok(()) => {
                            app.header_view.section_size_dialog = Default::default();
                            app.state = UIState::Normal;
                            app.dialog_renderer = None;
                        }
                        Err(e) => {
                            app.header_view.section_size_dialog.error_message = Some(e);
                        }
                    },
                    None => {
                        app.header_view.section_size_dialog.error_message =
                            Some(crate::i18n::M::SizeHexHint.tr(app.config.lang).to_string());
                    }
                }
            }
            KeyCode::Char(c) if app.header_view.section_size_dialog.selection_all => {
                app.header_view.section_size_dialog.selection_all = false;
                app.header_view.section_size_dialog.selection_anchor = None;
                app.header_view.section_size_dialog.input = tui_input::Input::new(c.to_string());
            }

            KeyCode::Backspace | KeyCode::Delete if app.header_view.section_size_dialog.selection_all => {
                app.header_view.section_size_dialog.selection_all = false;
                app.header_view.section_size_dialog.selection_anchor = None;
                app.header_view.section_size_dialog.input = tui_input::Input::default();
            }

            _ => {
                app.header_view.section_size_dialog.selection_all = false;
                crate::text_field::handle_key(app, section_size_field, event);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tools_feedback_tests {
    use crate::app::App;
    use crate::editor::{AppView, UIState};
    use crate::header::header_view::HeaderPane;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::layout::Rect;
    use ratatui::{Terminal, backend::TestBackend};

    const W: u16 = 120;
    const H: u16 = 30;

    fn loaded(read_only: bool) -> Option<App> {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dezes_tools");
        std::fs::create_dir_all(&dir).ok()?;
        let copy = dir.join(format!("pe_{}_{}.exe", std::process::id(), seq));
        std::fs::copy(std::env::current_exe().ok()?, &copy).ok()?;

        let mut app = App::new();
        app.config.database = false;
        let exe = copy.to_str()?.to_string();
        app.load_file(&exe, 0, read_only).ok()?;
        if !read_only && app.file_info.is_read_only {
            return None;
        }
        app.header_view.pe.as_ref()?;
        app.editor_view = AppView::Header;
        app.header_view.active_pane = HeaderPane::Detail;
        app.header_view.sidebar_index = 6;
        app.screen = Rect::new(0, 0, W, H);
        Some(app)
    }

    fn screen(app: &mut App) -> Vec<String> {
        let mut t = Terminal::new(TestBackend::new(W, H)).expect("terminal");
        t.draw(|f| crate::draw::draw(f, app)).expect("draw");
        let b = t.backend().buffer().clone();
        (0..H)
            .map(|y| (0..W).map(|x| b[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    fn enter(app: &mut App) {
        let key = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = super::super::events::view_header_pe_events(app, key);
    }

    fn click(app: &mut App, column: u16, row: u16) {
        let event = ratatui::crossterm::event::Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        let _ = crate::events::dispatch_event(app, event);
    }

    #[test]
    fn the_alignment_reports_inside_the_box() {
        let Some(mut app) = loaded(false) else { return };
        app.header_view.detail_index = 0;

        enter(&mut app);
        assert!(app.header_view.tools_last_message.is_some(), "no message was set");
        assert!(!app.hex_view.changed_bytes.is_empty(), "nothing was staged");

        let rows = screen(&mut app);
        let message_row = rows
            .iter()
            .position(|r| r.contains("PointerToRawData set to"))
            .expect("the message is not on screen");
        assert!(
            rows[message_row + 1].contains('\u{2518}') || rows[message_row + 1].contains('\u{2500}'),
            "the message is on the border row itself:\n{}\n{}",
            rows[message_row],
            rows[message_row + 1]
        );
    }

    #[test]
    fn the_row_names_its_section() {
        let Some(mut app) = loaded(false) else { return };
        let name = app
            .header_view
            .pe
            .as_ref()
            .and_then(|pe| pe.sections.first().and_then(|s| s.name().ok().map(String::from)))
            .expect("a first section");

        let rows = screen(&mut app);
        assert!(
            rows.iter().any(|r| r.contains(&name)),
            "the action row does not say which section it targets"
        );
    }

    #[test]
    fn read_only_refuses_the_tools() {
        let Some(mut app) = loaded(true) else { return };

        app.header_view.detail_index = 0;
        enter(&mut app);
        assert!(app.hex_view.changed_bytes.is_empty(), "a read-only file was edited");
        assert!(app.status_error.is_some(), "the refusal was not reported");

        app.status_error = None;
        app.header_view.detail_index = 1;
        enter(&mut app);
        assert!(app.state != UIState::DialogSectionSize, "the size prompt opened anyway");
        assert!(app.status_error.is_some());
    }

    #[test]
    fn a_click_runs_the_tool_it_lands_on() {
        let Some(mut app) = loaded(false) else { return };
        let _ = screen(&mut app);

        click(&mut app, W / 2, 2);
        assert_eq!(app.header_view.detail_index, 0);
        assert!(
            app.header_view.tools_last_message.is_some(),
            "clicking the first action did not run it"
        );

        click(&mut app, W / 2, 3);
        assert_eq!(app.header_view.detail_index, 1);
        assert!(app.state == UIState::DialogSectionSize, "the size prompt did not open");
    }

    #[test]
    fn a_click_in_the_sidebar_switches_tab() {
        let Some(mut app) = loaded(false) else { return };
        let _ = screen(&mut app);

        click(&mut app, 2, 3);
        assert_eq!(app.header_view.sidebar_index, 2);
        assert!(app.header_view.active_pane == HeaderPane::Sidebar);
        assert_eq!(app.header_view.detail_index, 0, "the row index has to reset with the tab");
    }
}
