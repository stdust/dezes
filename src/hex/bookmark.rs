use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers};
use iced_x86::Formatter;
use serde::Serialize;
use std::io::Result;

use crate::{app::App, editor::UIState};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Default)]
pub struct Bookmark {
    pub offset: usize,
    #[serde(default)]
    pub label: String,
}

impl<'de> serde::Deserialize<'de> for Bookmark {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BookmarkVisitor;

        impl<'de> serde::de::Visitor<'de> for BookmarkVisitor {
            type Value = Bookmark;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an integer offset or a bookmark struct with offset and label")
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Bookmark, E>
            where
                E: serde::de::Error,
            {
                let offset = usize::try_from(v)
                    .map_err(|_| E::custom("bookmark offset cannot be negative"))?;
                Ok(Bookmark {
                    offset,
                    label: String::new(),
                })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Bookmark, E>
            where
                E: serde::de::Error,
            {
                let offset = usize::try_from(v)
                    .map_err(|_| E::custom("bookmark offset is too large"))?;
                Ok(Bookmark {
                    offset,
                    label: String::new(),
                })
            }

            fn visit_map<M>(self, mut access: M) -> std::result::Result<Bookmark, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut offset = None;
                let mut label = String::new();

                while let Some((key, value)) = access.next_entry::<String, toml::Value>()? {
                    match key.as_str() {
                        "offset" => {
                            if let Some(n) = value.as_integer()
                                && let Ok(u) = usize::try_from(n)
                            {
                                offset = Some(u);
                            }
                        }
                        "label" => {
                            if let Some(s) = value.as_str() {
                                label = s.to_string();
                            }
                        }
                        _ => {}
                    }
                }

                let offset = offset.ok_or_else(|| serde::de::Error::missing_field("offset"))?;
                Ok(Bookmark { offset, label })
            }
        }

        deserializer.deserialize_any(BookmarkVisitor)
    }
}

#[derive(Default)]
pub struct BookmarksDialog {
    pub selected_index: usize,
    pub input: tui_input::Input,
    pub target_offset: usize,
    pub is_new: bool,
    pub return_to_list: bool,
}

fn fixed_centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width, height)
}

pub fn open_bookmarks_dialog(app: &mut App) {
    if !app.hex_view.bookmarks.is_empty()
        && app.hex_view.bookmark_dialog.selected_index >= app.hex_view.bookmarks.len()
    {
        app.hex_view.bookmark_dialog.selected_index = app.hex_view.bookmarks.len() - 1;
    }
    app.state = UIState::DialogBookmarks;
    app.dialog_renderer = Some(|app, frame| draw_bookmarks_dialog(app, frame, app.screen));
}

pub fn open_add_bookmark_dialog(app: &mut App, offset: usize) {
    let existing_label = app
        .hex_view
        .bookmarks
        .iter()
        .find(|b| b.offset == offset)
        .map(|b| b.label.clone())
        .unwrap_or_default();

    let cur = existing_label.chars().count();
    app.hex_view.bookmark_dialog.input = tui_input::Input::new(existing_label).with_cursor(cur);
    app.hex_view.bookmark_dialog.target_offset = offset;
    app.hex_view.bookmark_dialog.is_new = true;
    app.hex_view.bookmark_dialog.return_to_list = app.state == UIState::DialogBookmarks;
    app.state = UIState::DialogBookmarkInput;
    app.dialog_renderer = Some(|app, frame| draw_bookmark_input_popup(app, frame, app.screen));
}

pub fn open_edit_bookmark_dialog(app: &mut App, index: usize) {
    if let Some(b) = app.hex_view.bookmarks.get(index) {
        let offset = b.offset;
        let label = b.label.clone();
        let cur = label.chars().count();
        app.hex_view.bookmark_dialog.input = tui_input::Input::new(label).with_cursor(cur);
        app.hex_view.bookmark_dialog.target_offset = offset;
        app.hex_view.bookmark_dialog.is_new = false;
        app.hex_view.bookmark_dialog.return_to_list = true;
        app.state = UIState::DialogBookmarkInput;
        app.dialog_renderer = Some(|app, frame| draw_bookmark_input_popup(app, frame, app.screen));
    }
}

fn get_preview_text(app: &App, offset: usize) -> String {
    let buf = app.file_info.get_buffer_ref();
    if offset >= buf.len() {
        return String::new();
    }
    let end = (offset + 6).min(buf.len());
    let bytes = &buf[offset..end];
    let hex_part = bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");

    let is_disasm = app.editor_view == crate::editor::AppView::Disasm || app.is_executable();
    if is_disasm && app.is_executable() {
        let bitness = if app.is_64() { 64 } else { 32 };
        let slice = &buf[offset..buf.len().min(offset + 15)];
        let decoder = iced_x86::Decoder::with_ip(bitness, slice, app.get_va(offset), iced_x86::DecoderOptions::NONE);
        if let Some(instr) = decoder.into_iter().next() {
            let mut raw = String::new();
            let mut formatter = iced_x86::IntelFormatter::new();
            formatter.options_mut().set_first_operand_char_index(0);
            formatter.options_mut().set_hex_prefix("0x");
            formatter.options_mut().set_hex_suffix("");
            formatter.options_mut().set_leading_zeroes(false);
            formatter.format(&instr, &mut raw);
            let clean = crate::disasm::draw::clean_instruction_text(app, &instr, &raw);
            if !clean.is_empty() {
                return format!("{:<17}   {}", hex_part, clean);
            }
        }
    }
    hex_part
}

pub fn draw_bookmarks_dialog(app: &mut App, frame: &mut Frame, area: Rect) {
    let popup_area = fixed_centered_rect(96, 20, area);
    frame.render_widget(Clear, popup_area);

    let dialog_style = app.config.theme.dialog;
    let total_count = app.hex_view.bookmarks.len();

    let title = format!(
        " {} ({} {}) ",
        crate::i18n::M::BookmarksTitle.tr(app.config.lang),
        total_count,
        crate::i18n::M::FoundCount.tr(app.config.lang)
    );

    let outer_block = Block::default()
        .title(title)
        .title_bottom(crate::i18n::M::BookmarksFooterKeys.tr(app.config.lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = outer_block.inner(popup_area);
    frame.render_widget(outer_block, popup_area);

    if total_count == 0 {
        let empty_msg = crate::i18n::M::NoBookmarks.tr(app.config.lang);
        let para = Paragraph::new(empty_msg)
            .alignment(ratatui::layout::Alignment::Center)
            .style(dialog_style);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(inner_area.height / 2),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner_area);
        frame.render_widget(para, chunks[1]);
        return;
    }

    let is_64 = app.is_64();
    let is_disasm = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr_col_len = if is_64 && is_disasm {
        let max_len = app.hex_view.bookmarks.iter()
            .map(|b| {
                let va = app.get_va(b.offset);
                if va >= 0x1_0000_0000 { format!("{:X}", va).len() } else { 8 }
            })
            .max()
            .unwrap_or(9);
        max_len.max(crate::i18n::M::LblAddress.tr(app.config.lang).chars().count())
    } else {
        8
    };
    let label_col_len = 18usize; // reduced by 2 spaces from 20

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1)])
        .split(inner_area);

    let sep_style = dialog_style
        .fg(app.config.theme.dimmed.fg.unwrap_or(Color::DarkGray))
        .remove_modifier(Modifier::BOLD);
    let bold_header = dialog_style.add_modifier(Modifier::BOLD);

    let header_cells = [
        Cell::from(vec![
            Line::from(vec![
                Span::styled(" No.", bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled("────┼", sep_style)),
        ]),
        Cell::from(vec![
            Line::from(vec![
                Span::styled(format!(" {:<width$} ", crate::i18n::M::LblAddress.tr(app.config.lang), width = addr_col_len), bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled(format!("{:─>width$}┼", "", width = addr_col_len + 2), sep_style)),
        ]),
        Cell::from(vec![
            Line::from(vec![
                Span::styled(format!(" {:<width$} ", crate::i18n::M::LblLabel.tr(app.config.lang), width = label_col_len), bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled(format!("{:─>width$}┼", "", width = label_col_len + 2), sep_style)),
        ]),
        Cell::from(vec![
            Line::from(Span::styled(format!(" {}", crate::i18n::M::LblPreview.tr(app.config.lang)), bold_header)),
            Line::from(Span::styled("────────────────────────────────────────────────────────────────────────", sep_style)),
        ]),
    ];
    let header = Row::new(header_cells).style(dialog_style).height(2);

    let visible_rows = chunks[0].height.saturating_sub(3) as usize; // 2 for header + 1 padding
    let sel = app.hex_view.bookmark_dialog.selected_index.min(total_count.saturating_sub(1));
    let half = visible_rows / 2;
    let start_idx = if sel > half {
        (sel - half).min(total_count.saturating_sub(visible_rows))
    } else {
        0
    };
    let end_idx = (start_idx + visible_rows).min(total_count);
    let select_offset = Some(sel.saturating_sub(start_idx));

    let mut rows = Vec::with_capacity(end_idx - start_idx);
    for idx in start_idx..end_idx {
        let item = &app.hex_view.bookmarks[idx];
        let is_selected = idx == app.hex_view.bookmark_dialog.selected_index;
        let row_style = if is_selected {
            app.config.theme.highlight.add_modifier(Modifier::BOLD)
        } else {
            dialog_style
        };

        let addr = if is_disasm {
            app.get_va(item.offset)
        } else {
            item.offset as u64
        };
        let addr_str = if is_64 && is_disasm && addr >= 0x1_0000_0000 {
            format!("{:X}", addr)
        } else {
            format!("{:08X}", addr)
        };

        let preview_str = get_preview_text(app, item.offset);
        let truncated_label: String = item.label.chars().take(label_col_len).collect();

        let cells = if is_selected {
            vec![
                Cell::new(format!(" {:>2} │", idx + 1)).style(row_style),
                Cell::new(format!(" {:<width$} │", addr_str, width = addr_col_len)).style(row_style),
                Cell::new(format!(" {:<width$} │", truncated_label, width = label_col_len)).style(row_style),
                Cell::new(format!(" {}", preview_str)).style(row_style),
            ]
        } else {
            vec![
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {:>2} ", idx + 1), row_style),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {:<width$} ", addr_str, width = addr_col_len), row_style),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {:<width$} ", truncated_label, width = label_col_len), row_style),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {}", preview_str), row_style),
                ])).style(row_style),
            ]
        };
        rows.push(Row::new(cells).style(row_style));
    }

    let widths = [
        Constraint::Length(5),
        Constraint::Length((addr_col_len + 3) as u16),
        Constraint::Length((label_col_len + 3) as u16),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .style(dialog_style);

    let mut table_state = TableState::default();
    table_state.select(select_offset);

    frame.render_stateful_widget(table, chunks[0], &mut table_state);
}

pub fn draw_bookmark_input_popup(app: &mut App, frame: &mut Frame, area: Rect) {
    let popup_area = fixed_centered_rect(60, 3, area);
    frame.render_widget(Clear, popup_area);

    let dialog_style = app.config.theme.dialog;
    let is_disasm = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if is_disasm {
        app.get_va(app.hex_view.bookmark_dialog.target_offset)
    } else {
        app.hex_view.bookmark_dialog.target_offset as u64
    };

    let title_prefix = if app.hex_view.bookmark_dialog.is_new {
        crate::i18n::M::BookmarkAddTitle.tr(app.config.lang)
    } else {
        crate::i18n::M::BookmarkEditTitle.tr(app.config.lang)
    };
    let title = format!(" {} (0x{:X}) ", title_prefix, addr);

    let block = Block::default()
        .title(title)
        .title_bottom(crate::i18n::M::BookmarkInputHint.tr(app.config.lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let val = app.hex_view.bookmark_dialog.input.value();
    let cur = app.hex_view.bookmark_dialog.input.visual_cursor();

    let (before, after) = if cur < val.len() {
        let (b, rest) = val.split_at(cur);
        let mut chars = rest.chars();
        let c = chars.next().unwrap_or(' ');
        let a = chars.as_str();
        (b, Some((c, a)))
    } else {
        (val, None)
    };

    let mut spans = vec![Span::raw(" "), Span::raw(before)];
    if let Some((c, a)) = after {
        spans.push(Span::styled(c.to_string(), app.config.theme.highlight.add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(a));
    } else {
        spans.push(Span::styled(" ", app.config.theme.highlight.add_modifier(Modifier::BOLD)));
    }

    let para = Paragraph::new(Line::from(spans)).style(dialog_style);
    frame.render_widget(para, inner);
}

pub fn dialog_bookmarks_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Mouse(mouse) = event {
        if let ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) = mouse.kind {
            let total = app.hex_view.bookmarks.len();
            if total > 0 {
                let popup_height = 20u16.min(app.screen.height);
                let dialog_y = app.screen.y + (app.screen.height.saturating_sub(popup_height)) / 2;
                let table_start_y = dialog_y + 4;
                let visible_rows = (popup_height.saturating_sub(6)) as usize;

                let sel = app.hex_view.bookmark_dialog.selected_index.min(total - 1);
                let half = visible_rows / 2;
                let start = if sel > half {
                    (sel - half).min(total.saturating_sub(visible_rows))
                } else {
                    0
                };

                if mouse.row >= table_start_y && (mouse.row - table_start_y) < visible_rows as u16 {
                    let clicked_row = (mouse.row - table_start_y) as usize;
                    let clicked_index = start + clicked_row;
                    if clicked_index < total {
                        app.hex_view.bookmark_dialog.selected_index = clicked_index;
                    }
                }
            }
        }
        match mouse.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                if app.hex_view.bookmark_dialog.selected_index > 0 {
                    app.hex_view.bookmark_dialog.selected_index -= 1;
                }
            }
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                let cnt = app.hex_view.bookmarks.len();
                if cnt > 0 && app.hex_view.bookmark_dialog.selected_index + 1 < cnt {
                    app.hex_view.bookmark_dialog.selected_index += 1;
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    let Event::Key(key) = event else {
        return Ok(false);
    };

    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    let total = app.hex_view.bookmarks.len();

    match key.code {
        KeyCode::Esc => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
        }
        KeyCode::Enter => {
            if let Some(item) = app.hex_view.bookmarks.get(app.hex_view.bookmark_dialog.selected_index) {
                let target_ofs = item.offset;
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                app.goto_with_history(target_ofs, false);
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_add_bookmark_dialog(app, app.hex_view.offset);
        }
        KeyCode::F(2) => {
            if total > 0 {
                open_edit_bookmark_dialog(app, app.hex_view.bookmark_dialog.selected_index);
            }
        }
        KeyCode::Delete | KeyCode::Backspace if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if total > 0 {
                let idx = app.hex_view.bookmark_dialog.selected_index;
                let removed = app.hex_view.bookmarks.remove(idx);
                let new_len = app.hex_view.bookmarks.len();
                if app.hex_view.bookmark_dialog.selected_index >= new_len {
                    app.hex_view.bookmark_dialog.selected_index = new_len.saturating_sub(1);
                }
                app.persist_annotations();
                App::log(app, format!("Deleted bookmark at 0x{:X} ({})", removed.offset, removed.label));
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(item) = app.hex_view.bookmarks.get(app.hex_view.bookmark_dialog.selected_index) {
                let text = format!("0x{:X}\t{}", item.offset, item.label);
                app.copy_to_clipboard(text, "bookmark".to_string());
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.hex_view.bookmark_dialog.selected_index > 0 {
                app.hex_view.bookmark_dialog.selected_index -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if total > 0 && app.hex_view.bookmark_dialog.selected_index + 1 < total {
                app.hex_view.bookmark_dialog.selected_index += 1;
            }
        }
        KeyCode::PageUp => {
            app.hex_view.bookmark_dialog.selected_index =
                app.hex_view.bookmark_dialog.selected_index.saturating_sub(10);
        }
        KeyCode::PageDown => {
            if total > 0 {
                app.hex_view.bookmark_dialog.selected_index =
                    (app.hex_view.bookmark_dialog.selected_index + 10).min(total - 1);
            }
        }
        KeyCode::Home => {
            app.hex_view.bookmark_dialog.selected_index = 0;
        }
        KeyCode::End if total > 0 => {
            app.hex_view.bookmark_dialog.selected_index = total - 1;
        }
        _ => {}
    }
    Ok(false)
}

pub fn dialog_bookmark_input_events(app: &mut App, event: &Event) -> Result<bool> {
    let Event::Key(key) = event else {
        return Ok(false);
    };

    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            if app.hex_view.bookmark_dialog.return_to_list {
                open_bookmarks_dialog(app);
            } else {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
        }
        KeyCode::Enter => {
            let label = app.hex_view.bookmark_dialog.input.value().trim().to_string();
            let target_ofs = app.hex_view.bookmark_dialog.target_offset;

            // Check if bookmark at target_offset already exists
            if let Some(pos) = app.hex_view.bookmarks.iter().position(|b| b.offset == target_ofs) {
                app.hex_view.bookmarks[pos].label = label.clone();
                app.hex_view.bookmark_dialog.selected_index = pos;
            } else {
                app.hex_view.bookmarks.push(Bookmark {
                    offset: target_ofs,
                    label: label.clone(),
                });
                app.hex_view.bookmarks.sort_by_key(|b| b.offset);
                if let Some(pos) = app.hex_view.bookmarks.iter().position(|b| b.offset == target_ofs) {
                    app.hex_view.bookmark_dialog.selected_index = pos;
                }
            }
            app.persist_annotations();
            App::log(app, format!("Bookmark saved at 0x{:X}: '{}'", target_ofs, label));

            if app.hex_view.bookmark_dialog.return_to_list {
                open_bookmarks_dialog(app);
            } else {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
        }
        _ => {
            tui_input::backend::crossterm::EventHandler::handle_event(
                &mut app.hex_view.bookmark_dialog.input,
                event,
            );
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn make_test_app(bytes: &[u8]) -> (std::path::PathBuf, App) {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("dz6_bm_{}_{}", std::process::id(), id));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.bin");
        std::fs::write(&path, bytes).unwrap();
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, false).unwrap();
        (dir, app)
    }

    #[test]
    fn bookmark_deserializes_both_int_and_struct() {
        let toml_ints = "bookmarks = [16, 32]\n";
        #[derive(serde::Deserialize)]
        struct TestDoc {
            bookmarks: Vec<Bookmark>,
        }
        let doc: TestDoc = toml::from_str(toml_ints).unwrap();
        assert_eq!(doc.bookmarks.len(), 2);
        assert_eq!(doc.bookmarks[0].offset, 16);
        assert_eq!(doc.bookmarks[0].label, "");
        assert_eq!(doc.bookmarks[1].offset, 32);

        let toml_structs = r#"
            bookmarks = [
                { offset = 100, label = "OEP" },
                { offset = 200, label = "Main" }
            ]
        "#;
        let doc2: TestDoc = toml::from_str(toml_structs).unwrap();
        assert_eq!(doc2.bookmarks.len(), 2);
        assert_eq!(doc2.bookmarks[0].offset, 100);
        assert_eq!(doc2.bookmarks[0].label, "OEP");
        assert_eq!(doc2.bookmarks[1].offset, 200);
        assert_eq!(doc2.bookmarks[1].label, "Main");
    }

    #[test]
    fn bookmark_add_edit_delete_and_jump() {
        let bytes = vec![0x90u8; 0x100];
        let (dir, mut app) = make_test_app(&bytes);

        // Add bookmark at 0x20
        open_add_bookmark_dialog(&mut app, 0x20);
        assert_eq!(app.state, UIState::DialogBookmarkInput);
        app.hex_view.bookmark_dialog.input = tui_input::Input::new("OEP".to_string());
        let enter = Event::Key(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ));
        let _ = dialog_bookmark_input_events(&mut app, &enter);

        assert_eq!(app.hex_view.bookmarks.len(), 1);
        assert_eq!(app.hex_view.bookmarks[0].offset, 0x20);
        assert_eq!(app.hex_view.bookmarks[0].label, "OEP");

        // Add bookmark at 0x10 (should sort before 0x20)
        open_add_bookmark_dialog(&mut app, 0x10);
        app.hex_view.bookmark_dialog.input = tui_input::Input::new("Header".to_string());
        let _ = dialog_bookmark_input_events(&mut app, &enter);

        assert_eq!(app.hex_view.bookmarks.len(), 2);
        assert_eq!(app.hex_view.bookmarks[0].offset, 0x10);
        assert_eq!(app.hex_view.bookmarks[1].offset, 0x20);

        // Open bookmarks dialog and jump to 0x20
        open_bookmarks_dialog(&mut app);
        app.hex_view.bookmark_dialog.selected_index = 1;
        let _ = dialog_bookmarks_events(&mut app, &enter);

        assert_eq!(app.state, UIState::Normal);
        assert_eq!(app.hex_view.offset, 0x20);

        // Open bookmarks dialog and delete 0x20
        open_bookmarks_dialog(&mut app);
        app.hex_view.bookmark_dialog.selected_index = 1;
        let del = Event::Key(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Delete,
            KeyModifiers::NONE,
        ));
        let _ = dialog_bookmarks_events(&mut app, &del);

        assert_eq!(app.hex_view.bookmarks.len(), 1);
        assert_eq!(app.hex_view.bookmarks[0].offset, 0x10);

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_draw_bookmarks_dialog_renders_correctly() {
        use ratatui::{Terminal, backend::TestBackend};
        let bytes = vec![0x90u8; 0x100];
        let (dir, mut app) = make_test_app(&bytes);
        app.hex_view.bookmarks.push(Bookmark { offset: 0x10, label: "TestEntry".to_string() });
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");

        terminal.draw(|f| {
            let area = f.area();
            draw_bookmarks_dialog(&mut app, f, area);
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Bookmarks"));
        assert!(rendered.contains("TestEntry"));
        assert!(rendered.contains("Address"));

        let dialog_bg = app.config.theme.dialog.bg.unwrap_or(Color::Reset);
        let highlight_bg = app.config.theme.highlight.bg.unwrap_or(Color::Reset);
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                if cell.symbol() == "│" || cell.symbol() == "┼" || cell.symbol() == "─" {
                    assert!(
                        cell.bg == dialog_bg || cell.bg == highlight_bg,
                        "Cell at ({}, {}) has mismatching bg: {:?}, expected {:?} or {:?}",
                        x, y, cell.bg, dialog_bg, highlight_bg
                    );
                }
            }
        }

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_64bit_bookmark_trims_leading_zeros() {
        use ratatui::{Terminal, backend::TestBackend};
        let bytes = vec![0x90u8; 0x100000];
        let (dir, mut app) = make_test_app(&bytes);
        app.hex_view.show_va = true;
        app.config.bitness_override = Some(64);
        app.image_base_override = Some(0x140000000);
        app.hex_view.bookmarks.push(Bookmark { offset: 0x5EB18, label: "TestEntry".to_string() });
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");

        terminal.draw(|f| {
            let area = f.area();
            draw_bookmarks_dialog(&mut app, f, area);
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("14005EB18"));
        assert!(!rendered.contains("000000014005EB18"));

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
