use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Row, Table, TableState},
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

    // 7 Sidebar Categories (PE Header titles updated according to user request)
    let categories = vec![
        format!("1. DOS Header"),
        format!("2. COFF Header"),
        format!("3. Optional Header"),
        format!("4. Data Directories"),
        format!("5. Section ({})", pe_ref.sections.len()),
        format!("6. Import Directory ({})", pe_ref.imports.len()),
        format!("7. Section Tools"),
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
                    let val_style = if is_row_sel {
                        if is_detail_active {
                            highlight_style.add_modifier(Modifier::BOLD)
                        } else {
                            main_style.add_modifier(Modifier::REVERSED)
                        }
                    } else {
                        main_style
                    };

                    Row::new(vec![
                        Cell::new(name.as_str()).style(main_style.add_modifier(Modifier::BOLD)),
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

                    let pick = |col: usize| {
                        if is_row_sel && sel_col == col { cell_style } else { row_style }
                    };

                    Row::new(vec![
                        Cell::new(name.as_str()).style(row_style),
                        Cell::new(rva.as_str()).style(pick(0)),
                        Cell::new(size.as_str()).style(pick(1)),
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
            // Section Table (Matches User Image 1 without underline on unselected cells)
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

            let len = sec_rows.len();
            if app.header_view.detail_index >= len && len > 0 {
                app.header_view.detail_index = len - 1;
            }

            let sel_col = app.header_view.detail_col_index.min(5);
            let visible_rows = detail_capacity(detail_area);
            app.header_view.last_detail_rows = visible_rows;
            let (start_idx, end_idx) = crate::header::formats::pe::fields::visible_window(
                len,
                app.header_view.detail_index,
                visible_rows,
            );

            let rows: Vec<Row> = sec_rows
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(end_idx.saturating_sub(start_idx))
                .map(|(idx, (num, name, vsize, voff, rsize, roff, flags))| {
                    let is_row_sel = idx == app.header_view.detail_index;
                    let (row_style, cell_style) =
                        selection_styles(&app.config.theme, is_row_sel, is_detail_active);

                    let make_cell_style = |c_idx: usize| {
                        if is_row_sel && sel_col == c_idx { cell_style } else { row_style }
                    };

                    Row::new(vec![
                        Cell::new(num.as_str()).style(row_style),
                        Cell::new(name.as_str()).style(make_cell_style(0)),
                        Cell::new(vsize.as_str()).style(make_cell_style(1)),
                        Cell::new(voff.as_str()).style(make_cell_style(2)),
                        Cell::new(rsize.as_str()).style(make_cell_style(3)),
                        Cell::new(roff.as_str()).style(make_cell_style(4)),
                        Cell::new(flags.as_str()).style(make_cell_style(5)),
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

            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_style(detail_border_style)
                .style(main_style);

            let table = Table::new(rows, widths)
                .block(detail_block)
                .header(Row::new(vec!["#", "Name", "Virtual Size", "Virtual Offset", "Raw Size", "Raw Offset", "Characteristics"]).style(main_style.add_modifier(Modifier::BOLD)));

            let mut state = TableState::default();
            if len > 0 {
                state.select(Some(app.header_view.detail_index.saturating_sub(start_idx)));
            }
            frame.render_stateful_widget(table, detail_area, &mut state);
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
        6 => {
            // Section Tools: a short menu of section-editing actions rather
            // than a data table, since there's nothing to browse - just
            // things to trigger.
            let mut action_rows: Vec<(String, String)> = Vec::new();
            match pe_ref.sections.get(app.header_view.tools_section_index) {
                // Names the section it will act on. The tool works on whichever row
                // the Section tab was left on, which is invisible from here - so
                // "Set PointerToRawData = VirtualAddress" left the user guessing
                // which of six sections was about to change.
                Some(section) => action_rows.push((
                    "Align Offset to VA".to_string(),
                    format!(
                        "Set '{}'.PointerToRawData = VirtualAddress (0x{:X})",
                        section.name().unwrap_or("?"),
                        section.virtual_address
                    ),
                )),
                None => action_rows.push((
                    "Align Offset to VA".to_string(),
                    "No sections - select one in the Section tab first".to_string(),
                )),
            }
            action_rows.push((
                "Add New Section".to_string(),
                "Append a new section of a given size (default 0x1000)".to_string(),
            ));

            let len = action_rows.len();
            if app.header_view.detail_index >= len {
                app.header_view.detail_index = len.saturating_sub(1);
            }

            // The result message goes *inside* the box, on its last row.
            //
            // It used to be a strip below the box, which made the box one row
            // shorter than the sidebar beside it and put the text where the bottom
            // border should have been - it read as a rendering fault rather than as
            // confirmation, which is why running the action looked like it had done
            // nothing.
            let has_message = app.header_view.tools_last_message.is_some();
            let footer_height = if has_message { 1 } else { 0 };

            let rows: Vec<Row> = action_rows
                .iter()
                .enumerate()
                .map(|(idx, (name, desc))| {
                    let is_row_sel = idx == app.header_view.detail_index;
                    let style = if is_row_sel {
                        if is_detail_active {
                            highlight_style.add_modifier(Modifier::BOLD)
                        } else {
                            main_style.add_modifier(Modifier::REVERSED)
                        }
                    } else {
                        main_style
                    };
                    Row::new(vec![
                        Cell::new(name.as_str()).style(main_style.add_modifier(Modifier::BOLD)),
                        Cell::new(desc.as_str()).style(style),
                    ])
                    .style(main_style)
                })
                .collect();

            let widths = [Constraint::Length(20), Constraint::Fill(1)];
            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_style(detail_border_style)
                .style(main_style);
            let inner = detail_block.inner(detail_area);
            frame.render_widget(detail_block, detail_area);

            let split = Layout::vertical([Constraint::Fill(1), Constraint::Length(footer_height)]);
            let [table_area, message_area] = inner.layout(&split);

            let table = Table::new(rows, widths)
                .header(Row::new(vec!["Action", "Description"]).style(main_style.add_modifier(Modifier::BOLD)));

            frame.render_widget(table, table_area);

            if let Some(msg) = &app.header_view.tools_last_message {
                use ratatui::widgets::Paragraph;
                let para = Paragraph::new(msg.as_str()).style(main_style.add_modifier(Modifier::BOLD));
                frame.render_widget(para, message_area);
            }
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
}
