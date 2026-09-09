use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
    layout::{Alignment, Constraint, Direction, Layout},
    style::Modifier,
    widgets::{Block, Borders, Clear, Paragraph},
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

mod pe_sig {
    pub const SIZE: usize = 4;
}

mod coff {
    pub const HEADER_SIZE: usize = 20;
    pub const NUMBER_OF_SECTIONS: usize = 2;
}

mod opt {
    pub const OFFSET_FROM_PE: usize = super::pe_sig::SIZE + super::coff::HEADER_SIZE; // 24
    pub const SIZE_OF_IMAGE: usize = 56;
    pub const DLL_CHARACTERISTICS: usize = 70;
}

pub fn align_up(value: u64, align: u64) -> u64 {
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
    pe_ptr + opt::OFFSET_FROM_PE + pe.coff_header.size_of_optional_header as usize + section_index * field::ENTRY_SIZE
}

pub fn align_offset_to_va(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return;
    }

    let Some(pe) = app.header_view.pe.as_ref() else {
        let msg = crate::i18n::M::ErrNoPeHeaders.tr(app.config.lang).to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    };

    let sec_count = pe.sections.len();
    if sec_count == 0 {
        let msg = "No sections to align".to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    }

    let updates: Vec<(usize, u32)> = pe
        .sections
        .iter()
        .enumerate()
        .map(|(idx, sec)| {
            (
                section_header_offset(pe, idx) + field::POINTER_TO_RAW_DATA,
                sec.virtual_address,
            )
        })
        .collect();

    for (target_off, new_offset) in updates {
        write_u32(app, target_off, new_offset);
    }

    let msg = crate::i18n::fill(
        crate::i18n::M::DonePointerToRawData.tr(app.config.lang),
        &[&sec_count.to_string()],
    );
    App::log(app, msg.clone());
    app.header_view.tools_last_message = Some(msg);
    app.update_file_headers();
}

pub fn remove_aslr(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return;
    }

    let Some(pe) = app.header_view.pe.as_ref() else {
        let msg = crate::i18n::M::ErrNoPeHeaders.tr(app.config.lang).to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    };

    if pe.optional_header.is_none() {
        let msg = crate::i18n::M::ErrNoOptionalHeader.tr(app.config.lang).to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    }

    let opt_off = pe.dos_header.pe_pointer as usize + opt::OFFSET_FROM_PE;
    let dll_char_off = opt_off + opt::DLL_CHARACTERISTICS;

    let b0 = crate::hex::edit::displayed_byte(app, dll_char_off);
    let b1 = crate::hex::edit::displayed_byte(app, dll_char_off + 1);
    let current = u16::from_le_bytes([b0, b1]);

    const IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE: u16 = 0x0040;

    if (current & IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE) != 0 {
        let new_val = current & !IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE;
        write_u16(app, dll_char_off, new_val);

        let msg = crate::i18n::fill(
            crate::i18n::M::DoneAslrRemoved.tr(app.config.lang),
            &[&format!("0x{:04X}", current), &format!("0x{:04X}", new_val)],
        );
        App::log(app, msg.clone());
        app.header_view.tools_last_message = Some(msg);
        app.update_file_headers();
    } else {
        let msg = crate::i18n::fill(
            crate::i18n::M::NoteAslrNotSet.tr(app.config.lang),
            &[&format!("0x{:04X}", current)],
        );
        App::log(app, msg.clone());
        app.header_view.tools_last_message = Some(msg);
    }
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
        let opt_off = pe.dos_header.pe_pointer as usize + opt::OFFSET_FROM_PE;

        let nsections = pe.sections.len();
        let new_sec_base = section_header_offset(pe, nsections);

        if (new_sec_base + field::ENTRY_SIZE) as u64 > size_of_headers {
            return Err(crate::i18n::fill(
                crate::i18n::M::ErrNoRoomForSectionHeader.tr(lang),
                &[&format!("{:X}", size_of_headers)],
            ));
        }

        // Verify that the space for the new section header is actually empty (zeroed padding).
        // If non-zero data exists, adding a section header would overwrite metadata or code.
        let buf = app.file_info.get_buffer_ref();
        if new_sec_base + field::ENTRY_SIZE <= buf.len() {
            let has_non_zero = (new_sec_base..new_sec_base + field::ENTRY_SIZE).any(|ofs| {
                app.hex_view.changed_bytes.get(&ofs).copied().unwrap_or(buf[ofs]) != 0
            });
            if has_non_zero {
                return Err(crate::i18n::fill(
                    crate::i18n::M::ErrNoRoomForSectionHeader.tr(lang),
                    &[&format!("{:X}", size_of_headers)],
                ));
            }
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
            coff_off: pe.dos_header.pe_pointer as usize + pe_sig::SIZE,
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

    write_u16(app, plan.coff_off + coff::NUMBER_OF_SECTIONS, plan.number_of_sections + 1);

    write_u32(app, plan.opt_off + opt::SIZE_OF_IMAGE, plan.new_size_of_image as u32);

    let current_len = app.file_info.buffer_len() as u64;
    let target_len = plan.new_raw_offset + plan.raw_size;
    let extension_len = target_len.saturating_sub(current_len);
    if extension_len > 0 {
        app.file_info.stage_extension(&vec![0u8; extension_len as usize]);
    }

    let msg = crate::i18n::fill(
        crate::i18n::M::DoneSectionAdded.tr(app.config.lang),
        &[
            ".new",
            &format!("0x{:X}", plan.new_va),
            &format!("0x{:X}", requested_size),
            &format!("0x{:X}", plan.new_raw_offset),
        ],
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

pub fn delete_last_section(app: &mut App) -> std::result::Result<(), String> {
    let lang = app.config.lang;
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return Err("File is read-only".to_string());
    }

    let (
        sec_hdr_off,
        coff_off,
        old_nsections,
        opt_off,
        new_size_of_image,
        last_name,
        last_raw_offset,
        last_raw_size,
        last_idx,
        nsections,
    ) = {
        let Some(pe) = app.header_view.pe.as_ref() else {
            return Err(crate::i18n::M::ErrNoPeHeaders.tr(lang).to_string());
        };

        let Some(opt) = pe.optional_header.as_ref() else {
            return Err(crate::i18n::M::ErrNoOptionalHeader.tr(lang).to_string());
        };

        let nsections = pe.sections.len();
        if nsections <= 1 {
            let msg = "Cannot delete the only remaining section".to_string();
            app.header_view.tools_last_message = Some(msg.clone());
            app.error(msg.clone());
            return Err(msg);
        }

        let last_idx = nsections - 1;
        let last_sec = &pe.sections[last_idx];
        let last_name = last_sec.name().unwrap_or(".last").to_string();
        let last_raw_offset = last_sec.pointer_to_raw_data as usize;
        let last_raw_size = last_sec.size_of_raw_data as usize;

        let sec_hdr_off = section_header_offset(pe, last_idx);
        let coff_off = pe.dos_header.pe_pointer as usize + pe_sig::SIZE;
        let old_nsections = pe.coff_header.number_of_sections;

        let section_alignment = opt.windows_fields.section_alignment.max(1) as u64;
        let opt_off = pe.dos_header.pe_pointer as usize + opt::OFFSET_FROM_PE;

        let remaining_sections = &pe.sections[..last_idx];
        let max_end_va = remaining_sections
            .iter()
            .map(|s| s.virtual_address as u64 + (s.virtual_size as u64).max(s.size_of_raw_data as u64))
            .max()
            .unwrap_or(opt.windows_fields.size_of_headers as u64);

        let new_size_of_image = align_up(max_end_va, section_alignment);

        (
            sec_hdr_off,
            coff_off,
            old_nsections,
            opt_off,
            new_size_of_image,
            last_name,
            last_raw_offset,
            last_raw_size,
            last_idx,
            nsections,
        )
    };

    // 1. Zero out the 40-byte section header entry in memory
    write_bytes(app, sec_hdr_off, &[0u8; field::ENTRY_SIZE]);

    // 2. Decrement NumberOfSections in COFF header
    write_u16(app, coff_off + coff::NUMBER_OF_SECTIONS, old_nsections.saturating_sub(1));

    // 3. Recalculate SizeOfImage based on remaining sections
    write_u32(app, opt_off + opt::SIZE_OF_IMAGE, new_size_of_image as u32);

    // 4. Truncate file if last section's raw data was at or near the physical EOF
    let buf_len = app.file_info.buffer_len();
    if last_raw_offset > 0 && last_raw_offset + last_raw_size >= buf_len {
        app.file_info.shrink_to(last_raw_offset);
        app.hex_view.changed_bytes.retain(|&k, _| k < last_raw_offset);
    }

    // 5. Adjust selection and refresh headers
    if app.header_view.detail_index >= last_idx {
        app.header_view.detail_index = last_idx - 1;
    }
    if app.header_view.tools_section_index >= last_idx {
        app.header_view.tools_section_index = last_idx - 1;
    }

    app.update_file_headers();

    let msg = format!(
        "Deleted last section '{}' (SizeOfImage -> 0x{:X}, NumberOfSections -> {})",
        last_name, new_size_of_image, nsections - 1
    );
    App::log(app, msg.clone());
    app.header_view.tools_last_message = Some(msg);
    Ok(())
}

pub fn prompt_delete_last_section(app: &mut App) {
    let lang = app.config.lang;
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return;
    }
    let Some(pe) = app.header_view.pe.as_ref() else {
        app.error(crate::i18n::M::ErrNoPeHeaders.tr(lang).to_string());
        return;
    };
    if pe.sections.len() <= 1 {
        let msg = "Cannot delete the only remaining section".to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    }
    app.state = UIState::DialogConfirmDeleteSection;
    app.dialog_renderer = Some(dialog_confirm_delete_section_draw);
}

pub fn dialog_confirm_delete_section_draw(app: &mut App, frame: &mut Frame) {
    let Some(pe) = app.header_view.pe.as_ref() else { return };
    let last_sec_name = pe
        .sections
        .last()
        .and_then(|s| s.name().ok())
        .unwrap_or(".last")
        .to_string();

    let title = " Delete Section ";
    let prompt = format!("Delete last section '{}' and truncate file data?", last_sec_name);
    let options = "[y / Enter] Confirm   [n / Esc] Cancel";

    let area = crate::hex::field_box::centered_rect_above(64, 7, frame.area());
    frame.render_widget(Clear, area);

    let dialog_style = app.config.theme.dialog;
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // blank
            Constraint::Length(1), // prompt
            Constraint::Length(1), // blank
            Constraint::Length(1), // options
            Constraint::Length(1), // blank
        ])
        .split(inner);

    let prompt_para = Paragraph::new(prompt)
        .alignment(Alignment::Center)
        .style(dialog_style);
    frame.render_widget(prompt_para, chunks[1]);

    let options_para = Paragraph::new(options)
        .alignment(Alignment::Center)
        .style(app.config.theme.highlight);
    frame.render_widget(options_para, chunks[3]);
}

pub fn dialog_confirm_delete_section_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            let _ = delete_last_section(app);
            Ok(true)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub fn fix_size_of_image(app: &mut App) {
    if app.file_info.is_read_only {
        app.read_only_error(crate::i18n::M::RoSectionTools);
        return;
    }

    let Some((opt_off, current_size_of_image, expected_size_of_image)) = (|| {
        let pe = app.header_view.pe.as_ref()?;
        let opt = pe.optional_header.as_ref()?;

        let section_alignment = opt.windows_fields.section_alignment.max(1) as u64;
        let current_size_of_image = opt.windows_fields.size_of_image as u64;

        let max_end_va = pe
            .sections
            .iter()
            .map(|sec| sec.virtual_address as u64 + (sec.virtual_size as u64).max(sec.size_of_raw_data as u64))
            .max()
            .unwrap_or(opt.windows_fields.size_of_headers as u64);

        let expected_size_of_image = align_up(max_end_va, section_alignment);
        let opt_off = pe.dos_header.pe_pointer as usize + opt::OFFSET_FROM_PE;
        Some((opt_off, current_size_of_image, expected_size_of_image))
    })() else {
        let msg = crate::i18n::M::ErrNoPeHeaders.tr(app.config.lang).to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    };

    if current_size_of_image == expected_size_of_image {
        let msg = format!("SizeOfImage is already correct (0x{:X})", current_size_of_image);
        App::log(app, msg.clone());
        app.header_view.tools_last_message = Some(msg);
        return;
    }

    write_u32(app, opt_off + opt::SIZE_OF_IMAGE, expected_size_of_image as u32);
    app.update_file_headers();

    let msg = format!("Fixed SizeOfImage: 0x{:X} -> 0x{:X}", current_size_of_image, expected_size_of_image);
    App::log(app, msg.clone());
    app.header_view.tools_last_message = Some(msg);
}

pub fn open_dump_section_dialog(app: &mut App) {
    let Some(pe) = app.header_view.pe.as_ref() else {
        let msg = crate::i18n::M::ErrNoPeHeaders.tr(app.config.lang).to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    };

    let sec_idx = app.header_view.tools_section_index.min(pe.sections.len().saturating_sub(1));
    let Some(sec) = pe.sections.get(sec_idx) else {
        let msg = "No section selected to dump".to_string();
        app.header_view.tools_last_message = Some(msg.clone());
        app.error(msg);
        return;
    };

    let raw_name = sec.name().unwrap_or("sec");
    let clean_name = raw_name.trim_start_matches('.').replace('/', "_");
    let stem = std::path::Path::new(&app.file_info.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");

    let default_filename = format!("{}_{}.bin", stem, clean_name);
    app.header_view.dump_dialog.open(&default_filename);
    app.state = UIState::DialogDumpSection;
    app.dialog_renderer = Some(draw_dump_section_dialog);
}

pub fn draw_dump_section_dialog(app: &mut App, frame: &mut Frame) {
    let width = 50.min(frame.area().width.saturating_sub(4)).max(32);
    let height = if app.header_view.dump_dialog.error_message.is_some() { 4 } else { 3 };
    let dialog_area = crate::hex::field_box::centered_rect_above(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);

    let dialog = &app.header_view.dump_dialog;
    let input_text = dialog.input.value();

    let mut body = String::new();
    if let Some(err) = &dialog.error_message {
        body.push_str(err);
        body.push('\n');
    }

    let block = Block::bordered()
        .title(" Dump Section to File ")
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

    let text_row = if app.header_view.dump_dialog.error_message.is_some() { 2 } else { 1 };
    let cursor_x = dialog_area.x + 1 + app.header_view.dump_dialog.input.cursor() as u16;
    let cursor_y = dialog_area.y + text_row;
    if cursor_x < dialog_area.x + dialog_area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn dump_section_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    let dialog = &mut app.header_view.dump_dialog;
    (&mut dialog.input, &mut dialog.selection_anchor)
}

pub fn dialog_dump_section_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        if key.modifiers.contains(KeyModifiers::SHIFT) {
            app.header_view.dump_dialog.selection_all = false;
        }

        match key.code {
            KeyCode::Esc => {
                app.header_view.dump_dialog = Default::default();
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            KeyCode::Enter => {
                let filename = app.header_view.dump_dialog.input.value().trim().to_string();
                if filename.is_empty() {
                    app.header_view.dump_dialog.error_message = Some("Filename cannot be empty".to_string());
                    return Ok(false);
                }

                let Some(pe) = app.header_view.pe.as_ref() else {
                    app.state = UIState::Normal;
                    app.dialog_renderer = None;
                    return Ok(false);
                };

                let sec_idx = app.header_view.tools_section_index.min(pe.sections.len().saturating_sub(1));
                let Some(sec) = pe.sections.get(sec_idx) else {
                    app.state = UIState::Normal;
                    app.dialog_renderer = None;
                    return Ok(false);
                };

                let sec_name = sec.name().unwrap_or("section").to_string();
                let raw_offset = sec.pointer_to_raw_data as usize;
                let raw_size = sec.size_of_raw_data as usize;
                let buf = app.file_info.get_buffer_ref();
                let end_offset = (raw_offset + raw_size).min(buf.len());

                let mut data = if raw_offset < buf.len() {
                    buf[raw_offset..end_offset].to_vec()
                } else {
                    Vec::new()
                };

                // Overlay any changed bytes
                for (i, byte) in data.iter_mut().enumerate() {
                    let ofs = raw_offset + i;
                    if let Some(&ch) = app.hex_view.changed_bytes.get(&ofs) {
                        *byte = ch;
                    }
                }

                let target_path = if std::path::Path::new(&filename).is_absolute() {
                    std::path::PathBuf::from(&filename)
                } else if let Some(parent) = std::path::Path::new(&app.file_info.path).parent() {
                    parent.join(&filename)
                } else {
                    std::path::PathBuf::from(&filename)
                };

                match std::fs::write(&target_path, &data) {
                    Ok(()) => {
                        let msg = format!(
                            "Dumped section '{}' (0x{:X} bytes) to '{}'",
                            sec_name,
                            data.len(),
                            target_path.display()
                        );
                        App::log(app, msg.clone());
                        app.header_view.tools_last_message = Some(msg);
                        app.header_view.dump_dialog = Default::default();
                        app.state = UIState::Normal;
                        app.dialog_renderer = None;
                    }
                    Err(e) => {
                        app.header_view.dump_dialog.error_message = Some(format!("Error: {}", e));
                    }
                }
            }
            KeyCode::Char(c) if app.header_view.dump_dialog.selection_all => {
                app.header_view.dump_dialog.selection_all = false;
                app.header_view.dump_dialog.selection_anchor = None;
                app.header_view.dump_dialog.input = tui_input::Input::new(c.to_string());
            }
            KeyCode::Backspace | KeyCode::Delete if app.header_view.dump_dialog.selection_all => {
                app.header_view.dump_dialog.selection_all = false;
                app.header_view.dump_dialog.selection_anchor = None;
                app.header_view.dump_dialog.input = tui_input::Input::default();
            }
            _ => {
                app.header_view.dump_dialog.selection_all = false;
                crate::text_field::handle_key(app, dump_section_field, event);
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
        app.header_view.sidebar_index = 4;
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
        let sec_count = app.header_view.pe.as_ref().map(|p| p.sections.len()).unwrap_or(0);
        app.header_view.detail_index = sec_count;

        enter(&mut app);
        assert!(app.header_view.tools_last_message.is_some(), "no message was set");
        assert!(!app.hex_view.changed_bytes.is_empty(), "nothing was staged");

        let rows = screen(&mut app);
        let message_row = rows
            .iter()
            .position(|r| r.contains("Aligned PointerToRawData to VirtualAddress"))
            .expect("the message is not on screen");
        assert!(
            rows[message_row + 1].contains('\u{2518}') || rows[message_row + 1].contains('\u{2500}'),
            "the message is on the border row itself:\n{}\n{}",
            rows[message_row],
            rows[message_row + 1]
        );

        for sec in &app.header_view.pe.as_ref().unwrap().sections {
            assert_eq!(sec.pointer_to_raw_data, sec.virtual_address);
        }
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
        let sec_count = app.header_view.pe.as_ref().map(|p| p.sections.len()).unwrap_or(0);

        app.header_view.detail_index = sec_count;
        enter(&mut app);
        assert!(app.hex_view.changed_bytes.is_empty(), "a read-only file was edited");
        assert!(app.status_error.is_some(), "the refusal was not reported");

        app.status_error = None;
        app.header_view.detail_index = sec_count + 1;
        enter(&mut app);
        assert!(app.state != UIState::DialogSectionSize, "the size prompt opened anyway");
        assert!(app.status_error.is_some());
    }

    #[test]
    fn a_click_runs_the_tool_it_lands_on() {
        let Some(mut app) = loaded(false) else { return };
        let _ = screen(&mut app);
        let sec_count = app.header_view.pe.as_ref().map(|p| p.sections.len()).unwrap_or(0);

        let total_h = H.saturating_sub(2);
        let tools_h = (9u16).min(total_h.saturating_sub(6)).max(4);
        let tools_top_y = total_h.saturating_sub(tools_h);

        click(&mut app, W / 2, tools_top_y + 1);
        assert_eq!(app.header_view.detail_index, sec_count);
        assert!(
            app.header_view.tools_last_message.is_some(),
            "clicking the first action did not run it"
        );

        click(&mut app, W / 2, tools_top_y + 2);
        assert_eq!(app.header_view.detail_index, sec_count + 1);
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

    #[test]
    fn remove_aslr_clears_dynamic_base() {
        let Some(mut app) = loaded(false) else { return };
        let opt_off = app.header_view.pe.as_ref().unwrap().dos_header.pe_pointer as usize + 24;
        let dll_char_off = opt_off + 70;

        // Stage a DllCharacteristics with ASLR set (e.g. 0x8140)
        super::write_u16(&mut app, dll_char_off, 0x8140);
        app.update_file_headers();

        super::remove_aslr(&mut app);

        let b0 = crate::hex::edit::displayed_byte(&app, dll_char_off);
        let b1 = crate::hex::edit::displayed_byte(&app, dll_char_off + 1);
        let after = u16::from_le_bytes([b0, b1]);

        assert_eq!(after, 0x8100, "ASLR flag 0x0040 should be removed (0x8140 -> 0x8100)");
        assert!(app.header_view.tools_last_message.as_ref().unwrap().contains("ASLR removed"));
    }

    #[test]
    fn delete_last_section_removes_header_and_decrements_count() {
        let Some(mut app) = loaded(false) else { return };
        let initial_count = app.header_view.pe.as_ref().unwrap().sections.len();
        if initial_count <= 1 {
            return;
        }

        let res = super::delete_last_section(&mut app);
        assert!(res.is_ok(), "delete_last_section failed: {:?}", res);

        let new_count = app.header_view.pe.as_ref().unwrap().sections.len();
        assert_eq!(new_count, initial_count - 1);
        assert!(
            app.header_view.tools_last_message.as_ref().unwrap().contains("Deleted last section"),
            "success message missing"
        );
    }

    #[test]
    fn fix_size_of_image_updates_when_mismatched() {
        let Some(mut app) = loaded(false) else { return };
        let opt_off = app.header_view.pe.as_ref().unwrap().dos_header.pe_pointer as usize + super::opt::OFFSET_FROM_PE;
        let size_of_image_off = opt_off + super::opt::SIZE_OF_IMAGE;

        // Stage mismatched SizeOfImage
        super::write_u32(&mut app, size_of_image_off, 0x1000);
        app.update_file_headers();

        super::fix_size_of_image(&mut app);

        assert!(
            app.header_view.tools_last_message.as_ref().unwrap().contains("Fixed SizeOfImage"),
            "fix message missing"
        );
    }

    #[test]
    fn open_dump_section_dialog_sets_state_and_filename() {
        let Some(mut app) = loaded(false) else { return };
        super::open_dump_section_dialog(&mut app);

        assert_eq!(app.state, UIState::DialogDumpSection);
        assert!(app.header_view.dump_dialog.input.value().ends_with(".bin"));
    }

    #[test]
    fn prompt_delete_last_section_requires_confirmation() {
        let Some(mut app) = loaded(false) else { return };
        let initial_count = app.header_view.pe.as_ref().unwrap().sections.len();
        if initial_count <= 1 {
            return;
        }

        super::prompt_delete_last_section(&mut app);
        assert_eq!(app.state, UIState::DialogConfirmDeleteSection);

        // Press 'n' or Esc to cancel
        let key_cancel = KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = super::dialog_confirm_delete_section_events(&mut app, key_cancel);
        assert_eq!(app.state, UIState::Normal);
        assert_eq!(app.header_view.pe.as_ref().unwrap().sections.len(), initial_count);

        // Now prompt again and confirm with 'y'
        super::prompt_delete_last_section(&mut app);
        assert_eq!(app.state, UIState::DialogConfirmDeleteSection);
        let key_confirm = KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = super::dialog_confirm_delete_section_events(&mut app, key_confirm);
        assert_eq!(app.state, UIState::Normal);
        assert_eq!(app.header_view.pe.as_ref().unwrap().sections.len(), initial_count - 1);
        assert!(app.header_view.tools_last_message.as_ref().unwrap().contains("Deleted last section"));
    }

    #[test]
    fn section_tools_keyboard_shortcuts_trigger_actions() {
        let Some(mut app) = loaded(false) else { return };

        let press = |app: &mut App, code: KeyCode| {
            let key = KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = super::super::events::view_header_pe_events(app, key);
        };

        // 1. 'a' triggers align_offset_to_va
        press(&mut app, KeyCode::Char('a'));
        assert!(app.header_view.tools_last_message.as_ref().unwrap().contains("Aligned PointerToRawData"));

        // 2. 'n' triggers add_new_section prompt
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.state, UIState::DialogSectionSize);
        app.state = UIState::Normal;

        // 3. 'd' triggers dump section dialog
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.state, UIState::DialogDumpSection);
        app.state = UIState::Normal;

        // 4. Delete triggers delete last section confirm dialog
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.state, UIState::DialogConfirmDeleteSection);
        app.state = UIState::Normal;

        // 5. 'f' triggers fix_size_of_image
        press(&mut app, KeyCode::Char('f'));
        assert!(app.header_view.tools_last_message.is_some());

        // 6. 'r' triggers remove_aslr
        press(&mut app, KeyCode::Char('r'));
        assert!(app.header_view.tools_last_message.is_some());
    }
}



