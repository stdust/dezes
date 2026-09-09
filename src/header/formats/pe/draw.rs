use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, TableState},
};

use crate::app::App;
use crate::header::header_view::HeaderPane;


/// Styles for a row of a cell-based table: the whole row, and the one cell the
/// cursor is on.
///
/// A single highlighted cell was all there was to go on, and on a dark theme that
/// is a sliver of light in one column of seven - easy to lose and hard to tell
/// apart from a value that happens to be bright. The row now carries a band so it
/// is obvious *where* the cursor is, and the focused cell is inverted and bold
/// inside that band so it is obvious *which column* Enter will edit. No new
/// colours: both come from `theme.highlight`, so every theme keeps its own look.
fn selection_styles(
    theme: &crate::themes::Theme,
    is_row_sel: bool,
    is_detail_active: bool,
) -> (ratatui::style::Style, ratatui::style::Style) {
    if !is_row_sel {
        return (theme.main, theme.main);
    }
    if is_detail_active {
        (
            theme.highlight,
            theme
                .highlight
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    } else {
        // The pane does not have focus: show where the cursor would come back to,
        // without competing with the pane that does.
        (
            theme.main.add_modifier(Modifier::REVERSED),
            theme.main.add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    }
}

/// Rows the detail table can show, given the area it was handed.
///
/// Two borders and the column-header row are not data.
fn detail_capacity(detail_area: Rect) -> usize {
    detail_area.height.saturating_sub(3) as usize
}

pub fn pe_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let main_style = app.config.theme.main;
    let highlight_style = app.config.theme.highlight;
    let active_border_style = app.config.theme.highlight.add_modifier(Modifier::BOLD);
    let inactive_border_style = app.config.theme.dimmed;

    let pe_ref = match &app.header_view.pe {
        Some(pe) => pe,
        None => return,
    };

    // 2-Pane Horizontal Split: Sidebar (25%) + Inspector Panel (75%)
    let layout = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]);
    let [sidebar_area, detail_area] = area.layout(&layout);

    // Clear entire area and apply theme style to avoid background bleed
    frame.render_widget(Clear, area);
    let bg_block = Block::default().style(main_style);
    frame.render_widget(bg_block, area);

    // 6 Sidebar Categories
    let categories = [
        "1. DOS Header".to_string(),
        "2. COFF Header".to_string(),
        "3. Optional Header".to_string(),
        "4. Data Directories".to_string(),
        format!("5. Section ({})", pe_ref.sections.len()),
        format!("6. Import Directory ({})", pe_ref.imports.len()),
    ];

    let max_categories = categories.len();
    if app.header_view.sidebar_index >= max_categories {
        app.header_view.sidebar_index = 0;
    }

    let is_sidebar_active = app.header_view.active_pane == HeaderPane::Sidebar;
    let is_detail_active = app.header_view.active_pane == HeaderPane::Detail;

    let sidebar_border_style = if is_sidebar_active { active_border_style } else { inactive_border_style };
    let detail_border_style = if is_detail_active { active_border_style } else { inactive_border_style };

    // Render Left Sidebar List
    let sidebar_items: Vec<ListItem> = categories
        .iter()
        .enumerate()
        .map(|(idx, cat)| {
            let item_style = if idx == app.header_view.sidebar_index {
                if is_sidebar_active {
                    highlight_style.add_modifier(Modifier::BOLD)
                } else {
                    main_style.add_modifier(Modifier::REVERSED)
                }
            } else {
                main_style
            };
            ListItem::new(cat.as_str()).style(item_style)
        })
        .collect();

    let sidebar_block = Block::default()
        .borders(Borders::ALL)
        .border_style(sidebar_border_style)
        .style(main_style)
        .title(" PE ");

    let sidebar_list = List::new(sidebar_items).block(sidebar_block);
    frame.render_widget(sidebar_list, sidebar_area);

    // Render Right Panel based on selected Sidebar Category
    match app.header_view.sidebar_index {
        0..=2 => {
            // DOS / COFF / Optional Header: one key-value row per field.
            //
            // The rows come from `fields::kv_fields`, which is also what the Enter
            // and `g`/`f` handlers resolve `detail_index` through, so the row that
            // is highlighted is the field that gets edited. This used to be a
            // third private copy of the list.
            let kv_rows = crate::header::formats::pe::fields::kv_fields(
                pe_ref,
                app.header_view.sidebar_index,
            );

            let len = kv_rows.len();
            if app.header_view.detail_index >= len && len > 0 {
                app.header_view.detail_index = len - 1;
            }

            let visible_rows = detail_capacity(detail_area);
            app.header_view.last_detail_rows = visible_rows;
            let (start_idx, end_idx) = crate::header::formats::pe::fields::visible_window(
                len,
                app.header_view.detail_index,
                visible_rows,
            );

            let rows: Vec<Row> = kv_rows
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
                .map(|(idx, field)| {
                    let (name, val) = (&field.name, &field.value);
                    let is_row_sel = idx == app.header_view.detail_index;
                    let is_dirty = (field.offset..field.offset + field.size.max(1))
                        .any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));

                    let display_name = if is_dirty {
                        format!("* {}", name)
                    } else {
                        name.clone()
                    };

                    let name_style = if is_dirty {
                        app.config.theme.changed_bytes.add_modifier(Modifier::BOLD)
                    } else {
                        main_style.add_modifier(Modifier::BOLD)
                    };

                    let val_style = if is_row_sel {
                        if is_detail_active {
                            highlight_style.add_modifier(Modifier::BOLD)
                        } else {
                            main_style.add_modifier(Modifier::REVERSED)
                        }
                    } else if is_dirty {
                        app.config.theme.changed_bytes
                    } else {
                        main_style
                    };

                    Row::new(vec![
                        Cell::new(display_name).style(name_style),
                        Cell::new(val.as_str()).style(val_style),
                    ])
                    .style(main_style)
                })
                .collect();

            let widths = [Constraint::Length(30), Constraint::Fill(1)];
            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_style(detail_border_style)
                .style(main_style);

            let table = Table::new(rows, widths)
                .block(detail_block)
                .header(Row::new(vec!["Field Name", "Decoded Value"]).style(main_style.add_modifier(Modifier::BOLD)));

            // Selection is relative to the window, not the whole list.
            let mut state = TableState::default();
            if len > 0 {
                state.select(Some(app.header_view.detail_index.saturating_sub(start_idx)));
            }
            frame.render_stateful_widget(table, detail_area, &mut state);
        }
        3 => {
            // Data Directories Table (Show Real RVA and Size with Cell-level Selection)
            let dir_names = crate::header::formats::pe::fields::DATA_DIRECTORY_NAMES;

            let mut dir_rows = Vec::new();
            if let Some(opt) = &pe_ref.optional_header {
                let data_dirs = &opt.data_directories.data_directories;
                for (idx, name) in dir_names.iter().enumerate() {
                    let (rva, size) = if let Some(Some((_, dd))) = data_dirs.get(idx) {
                        (dd.virtual_address, dd.size)
                    } else {
                        (0, 0)
                    };

                    dir_rows.push((
                        format!("{}: {}", idx, name),
                        format!("{:08X}", rva),
                        format!("{:08X}", size),
                    ));
                }
            }

            let len = dir_rows.len();
            if app.header_view.detail_index >= len && len > 0 {
                app.header_view.detail_index = len - 1;
            }

            let sel_col = app.header_view.detail_col_index.min(1);
            let visible_rows = detail_capacity(detail_area);
            app.header_view.last_detail_rows = visible_rows;
            let (start_idx, end_idx) = crate::header::formats::pe::fields::visible_window(
                len,
                app.header_view.detail_index,
                visible_rows,
            );

            let rows: Vec<Row> = dir_rows
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
                .map(|(idx, (name, rva, size))| {
                    let is_row_sel = idx == app.header_view.detail_index;
                    let (row_style, cell_style) =
                        selection_styles(&app.config.theme, is_row_sel, is_detail_active);

                    let dd_base = crate::header::formats::pe::OptionalHeaderLayout::from_pe(pe_ref).data_directory(idx);
                    let rva_dirty = (dd_base..dd_base + 4).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let size_dirty = (dd_base + 4..dd_base + 8).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));

                    let display_name = if rva_dirty || size_dirty {
                        format!("* {}", name)
                    } else {
                        name.clone()
                    };

                    let pick = |col: usize, dirty: bool| {
                        if is_row_sel && sel_col == col {
                            cell_style
                        } else if dirty {
                            app.config.theme.changed_bytes.add_modifier(Modifier::BOLD)
                        } else {
                            row_style
                        }
                    };

                    let name_style = if rva_dirty || size_dirty {
                        app.config.theme.changed_bytes.add_modifier(Modifier::BOLD)
                    } else {
                        row_style
                    };

                    Row::new(vec![
                        Cell::new(display_name).style(name_style),
                        Cell::new(rva.as_str()).style(pick(0, rva_dirty)),
                        Cell::new(size.as_str()).style(pick(1, size_dirty)),
                    ])
                    .style(row_style)
                })
                .collect();

            let widths = [Constraint::Length(28), Constraint::Length(16), Constraint::Length(16)];
            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_style(detail_border_style)
                .style(main_style);

            let table = Table::new(rows, widths)
                .block(detail_block)
                .header(Row::new(vec!["Directory Name", "Virtual Address (RVA)", "Virtual Size"]).style(main_style.add_modifier(Modifier::BOLD)));

            let mut state = TableState::default();
            if len > 0 {
                state.select(Some(app.header_view.detail_index.saturating_sub(start_idx)));
            }
            frame.render_stateful_widget(table, detail_area, &mut state);
        }
        4 => {
            let sec_count = pe_ref.sections.len();
            // 6 tool rows + 2 borders + 1 footer/message row
            let tools_needed = 6 + 2 + 1;
            let tools_height = (tools_needed as u16)
                .min(detail_area.height.saturating_sub(6))
                .max(4);

            let vert_split = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(tools_height),
            ]);
            let [table_area, tools_box_area] = detail_area.layout(&vert_split);

            let is_in_table = is_detail_active && app.header_view.detail_index < sec_count;
            let is_in_tools = is_detail_active && app.header_view.detail_index >= sec_count;

            let table_border_style = if is_in_table { active_border_style } else { inactive_border_style };
            let tools_border_style = if is_in_tools { active_border_style } else { inactive_border_style };

            // --- 1. Section Table ---
            let mut sec_rows = Vec::new();
            for (sec_idx, sec) in pe_ref.sections.iter().enumerate() {
                let sec_name = sec.name().unwrap_or("???");
                sec_rows.push((
                    format!("{}", sec_idx + 1),
                    sec_name.to_string(),
                    format!("{:08X}", sec.virtual_size),
                    format!("{:08X}", sec.virtual_address),
                    format!("{:08X}", sec.size_of_raw_data),
                    format!("{:08X}", sec.pointer_to_raw_data),
                    format!("{:08X}", sec.characteristics),
                ));
            }

            let sel_table_idx = if app.header_view.detail_index < sec_count {
                app.header_view.detail_index
            } else {
                app.header_view.tools_section_index.min(sec_count.saturating_sub(1))
            };

            let sel_col = app.header_view.detail_col_index.min(5);
            let visible_rows = detail_capacity(table_area);
            app.header_view.last_detail_rows = visible_rows;
            let (start_idx, end_idx) = crate::header::formats::pe::fields::visible_window(
                sec_count,
                sel_table_idx,
                visible_rows,
            );

            let rows: Vec<Row> = sec_rows
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
                .map(|(idx, (num, name, vsize, voff, rsize, roff, flags))| {
                    let is_row_sel = idx == sel_table_idx;
                    let (row_style, cell_style) =
                        selection_styles(&app.config.theme, is_row_sel, is_in_table);

                    let size_of_opt_hdr = pe_ref.coff_header.size_of_optional_header as usize;
                    let sec_base = pe_ref.dos_header.pe_pointer as usize + 24 + size_of_opt_hdr + idx * 40;

                    let d_name = (sec_base..sec_base + 8).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let d_vsize = (sec_base + 8..sec_base + 12).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let d_voff = (sec_base + 12..sec_base + 16).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let d_rsize = (sec_base + 16..sec_base + 20).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let d_roff = (sec_base + 20..sec_base + 24).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let d_flags = (sec_base + 36..sec_base + 40).any(|ofs| app.hex_view.changed_bytes.contains_key(&ofs));
                    let any_dirty = d_name || d_vsize || d_voff || d_rsize || d_roff || d_flags;

                    let display_num = if any_dirty {
                        format!("* {}", num)
                    } else {
                        num.clone()
                    };

                    let make_cell_style = |c_idx: usize, dirty: bool| {
                        if is_row_sel && is_in_table && sel_col == c_idx {
                            cell_style
                        } else if dirty {
                            app.config.theme.changed_bytes.add_modifier(Modifier::BOLD)
                        } else {
                            row_style
                        }
                    };

                    Row::new(vec![
                        Cell::new(display_num).style(if any_dirty { app.config.theme.changed_bytes.add_modifier(Modifier::BOLD) } else { row_style }),
                        Cell::new(name.as_str()).style(make_cell_style(0, d_name)),
                        Cell::new(vsize.as_str()).style(make_cell_style(1, d_vsize)),
                        Cell::new(voff.as_str()).style(make_cell_style(2, d_voff)),
                        Cell::new(rsize.as_str()).style(make_cell_style(3, d_rsize)),
                        Cell::new(roff.as_str()).style(make_cell_style(4, d_roff)),
                        Cell::new(flags.as_str()).style(make_cell_style(5, d_flags)),
                    ])
                    .style(row_style)
                })
                .collect();

            let widths = [
                Constraint::Length(4),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
            ];

            let table_block = Block::default()
                .borders(Borders::ALL)
                .border_style(table_border_style)
                .style(main_style)
                .title(format!(" Sections ({}) ", sec_count));

            let table = Table::new(rows, widths)
                .block(table_block)
                .header(Row::new(vec!["#", "Name", "Virtual Size", "Virtual Offset", "Raw Size", "Raw Offset", "Characteristics"]).style(main_style.add_modifier(Modifier::BOLD)));

            let mut state = TableState::default();
            if sec_count > 0 {
                state.select(Some(sel_table_idx.saturating_sub(start_idx)));
            }
            frame.render_stateful_widget(table, table_area, &mut state);

            // --- 2. Section Tools Box ---
            let selected_sec_idx = app.header_view.tools_section_index.min(sec_count.saturating_sub(1));
            let selected_sec = pe_ref.sections.get(selected_sec_idx);
            let selected_sec_name = selected_sec.and_then(|s| s.name().ok()).unwrap_or("?");
            let last_sec = pe_ref.sections.last();
            let last_sec_name = last_sec.and_then(|s| s.name().ok()).unwrap_or("?");

            let stem = std::path::Path::new(&app.file_info.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            let clean_name = selected_sec_name.trim_start_matches('.').replace('/', "_");

            let section_alignment = pe_ref
                .optional_header
                .as_ref()
                .map(|o| o.windows_fields.section_alignment.max(1) as u64)
                .unwrap_or(0x1000);
            let max_end_va = pe_ref
                .sections
                .iter()
                .map(|s| s.virtual_address as u64 + (s.virtual_size as u64).max(s.size_of_raw_data as u64))
                .max()
                .unwrap_or(0);
            let expected_size_of_image = crate::header::formats::pe::section_tools::align_up(max_end_va, section_alignment);

            let titles = [
                crate::i18n::M::SecToolAlignOffsetsTitle.tr(app.config.lang),
                crate::i18n::M::SecToolAddSectionTitle.tr(app.config.lang),
                crate::i18n::M::SecToolDumpSectionTitle.tr(app.config.lang),
                crate::i18n::M::SecToolDeleteLastSectionTitle.tr(app.config.lang),
                crate::i18n::M::SecToolFixSizeOfImageTitle.tr(app.config.lang),
                crate::i18n::M::SecToolRemoveAslrTitle.tr(app.config.lang),
            ];

            let expected_size_str = format!("{:X}", expected_size_of_image);
            let descs = [
                crate::i18n::M::SecToolAlignOffsetsDesc.tr(app.config.lang).to_string(),
                crate::i18n::M::SecToolAddSectionDesc.tr(app.config.lang).to_string(),
                crate::i18n::fill(
                    crate::i18n::M::SecToolDumpSectionDesc.tr(app.config.lang),
                    &[selected_sec_name, stem, &clean_name],
                ),
                crate::i18n::fill(
                    crate::i18n::M::SecToolDeleteLastSectionDesc.tr(app.config.lang),
                    &[last_sec_name],
                ),
                crate::i18n::fill(
                    crate::i18n::M::SecToolFixSizeOfImageDesc.tr(app.config.lang),
                    &[&expected_size_str],
                ),
                crate::i18n::M::SecToolRemoveAslrDesc.tr(app.config.lang).to_string(),
            ];

            use unicode_width::UnicodeWidthStr;
            let max_title_w = titles.iter().map(|t| UnicodeWidthStr::width(*t)).max().unwrap_or(25);

            let tool_items: Vec<String> = titles
                .iter()
                .zip(descs.iter())
                .map(|(title, desc)| {
                    let tw = UnicodeWidthStr::width(*title);
                    let pad_len = max_title_w.saturating_sub(tw) + 2;
                    let pad = " ".repeat(pad_len);
                    format!("{}{}{}", title, pad, desc)
                })
                .collect();

            let tools_title = format!(" {} ", crate::i18n::M::SecToolsTitle.tr(app.config.lang));
            let tools_block = Block::default()
                .borders(Borders::ALL)
                .border_style(tools_border_style)
                .style(main_style)
                .title(tools_title);
            let tools_inner = tools_block.inner(tools_box_area);
            frame.render_widget(tools_block, tools_box_area);

            let tools_split = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]);
            let [items_area, message_area] = tools_inner.layout(&tools_split);

            let active_tool_idx = if is_in_tools {
                Some(app.header_view.detail_index.saturating_sub(sec_count))
            } else {
                None
            };

            let list_items: Vec<ListItem> = tool_items
                .iter()
                .enumerate()
                .map(|(idx, text)| {
                    let is_sel = active_tool_idx == Some(idx);
                    let item_style = if is_sel {
                        highlight_style.add_modifier(Modifier::BOLD)
                    } else {
                        main_style
                    };
                    let prefix = if is_sel { "> " } else { "  " };
                    ListItem::new(format!("{}{}", prefix, text)).style(item_style)
                })
                .collect();

            let tools_list = List::new(list_items);
            frame.render_widget(tools_list, items_area);

            if let Some(msg) = &app.header_view.tools_last_message {
                let para = Paragraph::new(format!("  * {}", msg)).style(app.config.theme.highlight.add_modifier(Modifier::BOLD));
                frame.render_widget(para, message_area);
            }
        }
        5 => {
            // Import Directory Table
            //
            // Only the visible slice is turned into rows. This built one for every
            // import - cloning two Strings each - on every frame, and none of them
            // past the bottom border could ever be seen: with 289 imports and room
            // for 24, the list stopped at 24 and the selection carried on moving
            // out of sight. Measured at 3.9 ms a frame against 0.3 ms for the other
            // tabs.
            let len = pe_ref.imports.len();
            if app.header_view.detail_index >= len && len > 0 {
                app.header_view.detail_index = len - 1;
            }

            let visible_rows = detail_capacity(detail_area);
            app.header_view.last_detail_rows = visible_rows;
            let (start_idx, end_idx) = crate::header::formats::pe::fields::visible_window(
                len,
                app.header_view.detail_index,
                visible_rows,
            );

            let mut imp_rows = Vec::with_capacity(end_idx.saturating_sub(start_idx));
            for (imp_idx, imp) in pe_ref
                .imports
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
            {
                imp_rows.push((
                    imp_idx,
                    format!("{}", imp_idx + 1),
                    imp.dll.clone(),
                    imp.name.clone(),
                    format!("{:08X}", imp.rva),
                    format!("{:08X}", imp.offset),
                ));
            }

            let rows: Vec<Row> = imp_rows
                .iter()
                .map(|(idx, num, dll, name, rva, roff)| {
                    let row_style = if *idx == app.header_view.detail_index {
                        if is_detail_active {
                            highlight_style.add_modifier(Modifier::BOLD)
                        } else {
                            main_style.add_modifier(Modifier::REVERSED)
                        }
                    } else {
                        main_style
                    };
                    Row::new(vec![
                        Cell::new(num.as_str()),
                        Cell::new(dll.as_str()),
                        Cell::new(name.as_str()),
                        Cell::new(rva.as_str()),
                        Cell::new(roff.as_str()),
                    ])
                    .style(row_style)
                })
                .collect();

            let widths = [
                Constraint::Length(5),
                Constraint::Length(18),
                Constraint::Length(30),
                Constraint::Length(14),
                Constraint::Length(14),
            ];

            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_style(detail_border_style)
                .style(main_style);

            let table = Table::new(rows, widths)
                .block(detail_block)
                .header(Row::new(vec!["#", "DLL Name", "Function Name", "RVA", "Raw Offset"]).style(main_style.add_modifier(Modifier::BOLD)));

            let mut state = TableState::default();
            if len > 0 {
                state.select(Some(app.header_view.detail_index.saturating_sub(start_idx)));
            }
            frame.render_stateful_widget(table, detail_area, &mut state);
        }
        _ => {}
    }
}

#[cfg(test)]
mod detail_scroll_tests {
    use crate::app::App;
    use crate::editor::AppView;
    use crate::header::header_view::HeaderPane;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use ratatui::layout::Rect;
    use ratatui::{Terminal, backend::TestBackend};

    /// A real PE with more imports than a short table can show.
    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        app.editor_view = AppView::Header;
        app.header_view.active_pane = HeaderPane::Detail;
        Some(app)
    }

    const WIDTH: u16 = 120;
    const HEIGHT: u16 = 24;

    fn render(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
        app.screen = Rect::new(0, 0, WIDTH, HEIGHT);
        terminal
            .draw(|f| crate::draw::draw(f, app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .chunks(WIDTH as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = super::super::events::view_header_pe_events(app, key);
    }

    /// The selected import has to be on screen wherever it is in the list.
    ///
    /// The table rendered from row 0 and stopped at the bottom border, so with 289
    /// imports and room for about 24 the list simply ended at 24: Down went on
    /// moving the selection, invisibly, and nothing on screen changed.
    #[test]
    fn a_late_import_row_is_scrolled_into_view() {
        let Some(mut app) = loaded_app() else { return };
        let imports = app.header_view.pe.as_ref().map(|pe| pe.imports.len()).unwrap_or(0);
        if imports <= HEIGHT as usize {
            return; // not enough rows for this to be a scrolling question
        }

        app.header_view.sidebar_index = 5; // Import Directory
        app.header_view.detail_index = imports - 1;

        let screen = render(&mut app);
        let last_row_number = format!("{}", imports);
        assert!(
            screen.contains(&last_row_number),
            "the last import (#{}) never made it onto the screen:\n{}",
            last_row_number,
            screen
        );
    }

    /// PageDown moves by a screenful, and End lands on the last entry.
    #[test]
    fn the_page_keys_walk_the_import_list() {
        let Some(mut app) = loaded_app() else { return };
        let imports = app.header_view.pe.as_ref().map(|pe| pe.imports.len()).unwrap_or(0);
        if imports <= HEIGHT as usize {
            return;
        }

        app.header_view.sidebar_index = 5;
        app.header_view.detail_index = 0;
        // One frame first, so the page step knows how tall the table is.
        let _ = render(&mut app);
        assert!(app.header_view.last_detail_rows > 0, "the draw has to report its height");

        press(&mut app, KeyCode::PageDown);
        let after_page = app.header_view.detail_index;
        assert!(
            after_page > 1,
            "PageDown moved {} row(s); it should move a screenful",
            after_page
        );

        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.header_view.detail_index, 0, "PageUp has to come back");

        press(&mut app, KeyCode::End);
        assert_eq!(app.header_view.detail_index, imports - 1);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.header_view.detail_index, 0);
    }

    /// The same for the Optional Header, which is 25 rows and does not fit a short
    /// terminal either.
    #[test]
    fn a_late_optional_header_field_is_scrolled_into_view() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.sidebar_index = 2;
        press(&mut app, KeyCode::End);

        let last_name = {
            let pe = app.header_view.pe.as_ref().unwrap();
            let rows = super::super::fields::kv_fields(pe, 2);
            rows.last().unwrap().name.clone()
        };

        let screen = render(&mut app);
        assert!(
            screen.contains(&last_name),
            "'{}' is the last Optional Header field and was off screen:\n{}",
            last_name,
            screen
        );
    }
}
#[cfg(test)]
mod selection_visibility_tests {
    use crate::app::App;
    use crate::editor::AppView;
    use crate::header::header_view::HeaderPane;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;
    use ratatui::{Terminal, backend::TestBackend};

    const WIDTH: u16 = 120;
    const HEIGHT: u16 = 24;

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        app.config.database = false;
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?;
        app.editor_view = AppView::Header;
        app.header_view.active_pane = HeaderPane::Detail;
        app.header_view.sidebar_index = 4; // Section table
        app.header_view.detail_index = 1;
        app.header_view.detail_col_index = 2; // VirtualAddress column
        Some(app)
    }

    /// The selected row has to be a band across the table, not one highlighted
    /// cell.
    ///
    /// A single cell of `highlight` in a seven-column table is a sliver, and on a
    /// dark theme it is easy to mistake for a bright value. The band says which
    /// row; the inverted cell inside it says which column Enter will edit.
    #[test]
    fn the_selected_section_row_is_a_visible_band() {
        let Some(mut app) = loaded_app() else { return };
        if app.header_view.pe.as_ref().map(|pe| pe.sections.len()).unwrap_or(0) < 2 {
            return;
        }

        let highlight_bg = app.config.theme.highlight.bg;
        let section_name = app
            .header_view
            .pe
            .as_ref()
            .and_then(|pe| pe.sections.get(1).and_then(|s| s.name().ok().map(String::from)))
            .expect("a second section with a name");

        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
        app.screen = Rect::new(0, 0, WIDTH, HEIGHT);
        terminal.draw(|f| crate::draw::draw(f, &mut app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();

        // Built from a column range, not by slicing a String: the sidebar can hold
        // multi-byte characters and a byte index into it is not a char boundary.
        let row_text = |y: u16, from: u16| {
            (from..WIDTH)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };

        // The detail pane starts a quarter of the way across; the sidebar has a
        // highlighted row of its own and a border drawn in the same colour.
        let detail_x0 = WIDTH / 4;
        let row = (0..HEIGHT)
            .find(|y| row_text(*y, detail_x0).contains(section_name.as_str()))
            .unwrap_or_else(|| {
                let screen = (0..HEIGHT)
                    .map(|y| row_text(y, 0))
                    .collect::<Vec<_>>()
                    .join("\n");
                panic!("'{}' is not on screen:\n{}", section_name, screen)
            });

        let band = (detail_x0..WIDTH)
            .filter(|x| buffer[(*x, row)].style().bg == highlight_bg)
            .count();
        assert!(
            band > 40,
            "the selected row is only {} cell(s) of band; it has to read as a row, not a sliver",
            band
        );

        // And inside that band, the focused cell is inverted, so the column Enter
        // will edit is distinguishable from the rest of the row.
        let inverted = (detail_x0..WIDTH)
            .filter(|x| {
                buffer[(*x, row)]
                    .style()
                    .add_modifier
                    .contains(Modifier::REVERSED)
            })
            .count();
        assert!(
            inverted > 0,
            "no cell in the selected row marks which column is focused"
        );
        assert!(
            inverted < band,
            "the whole row is inverted, so the focused column is not distinguishable"
        );
    }

    #[test]
    fn section_tools_i18n_korean_and_chinese() {
        use crate::i18n::{Lang, M};

        // Korean verification
        assert_eq!(M::SecToolsTitle.tr(Lang::Ko), "섹션 도구");
        assert_eq!(M::SecToolAlignOffsetsTitle.tr(Lang::Ko), "모든 섹션 VA 정렬 [a]");
        assert_eq!(M::SecToolAddSectionTitle.tr(Lang::Ko), "새 섹션 추가 [n]");
        assert_eq!(M::SecToolDumpSectionTitle.tr(Lang::Ko), "섹션 파일로 덤프 [d]");
        assert_eq!(M::SecToolDeleteLastSectionTitle.tr(Lang::Ko), "마지막 섹션 삭제 [Del]");
        assert_eq!(M::SecToolFixSizeOfImageTitle.tr(Lang::Ko), "SizeOfImage 보정 [f]");
        assert_eq!(M::SecToolRemoveAslrTitle.tr(Lang::Ko), "ASLR 제거 [r]");

        // Chinese verification
        assert_eq!(M::SecToolsTitle.tr(Lang::Zh), "节工具");
        assert_eq!(M::SecToolAlignOffsetsTitle.tr(Lang::Zh), "对齐所有节到 VA [a]");
        assert_eq!(M::SecToolAddSectionTitle.tr(Lang::Zh), "添加新节 [n]");
        assert_eq!(M::SecToolDumpSectionTitle.tr(Lang::Zh), "转储节到文件 [d]");
        assert_eq!(M::SecToolDeleteLastSectionTitle.tr(Lang::Zh), "删除最后一个节 [Del]");
        assert_eq!(M::SecToolFixSizeOfImageTitle.tr(Lang::Zh), "修复 SizeOfImage [f]");
        assert_eq!(M::SecToolRemoveAslrTitle.tr(Lang::Zh), "移除 ASLR [r]");
    }
}
