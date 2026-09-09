use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers};
use std::io::Result;

use crate::{app::App, editor::UIState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchItem {
    pub offset: usize,
    pub size: usize,
    pub orig_bytes: Vec<u8>,
    pub patched_bytes: Vec<u8>,
    pub enabled: bool,
}

#[derive(Default)]
pub struct PatchesDialog {
    pub items: Vec<PatchItem>,
    pub selected_index: usize,
}

impl PatchesDialog {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.items.clear();
        self.selected_index = 0;
    }
}

pub fn collect_patches(app: &App) -> Vec<PatchItem> {
    if app.hex_view.changed_bytes.is_empty() && app.hex_view.disabled_bytes.is_empty() {
        return Vec::new();
    }
    let mut offsets: Vec<usize> = app.hex_view.changed_bytes.keys()
        .chain(app.hex_view.disabled_bytes.keys())
        .copied()
        .collect();
    offsets.sort_unstable();
    offsets.dedup();

    let buffer = app.file_info.get_buffer_ref();
    let buf_len = buffer.len();

    let mut items = Vec::new();
    let mut i = 0;
    while i < offsets.len() {
        let start = offsets[i];
        let mut end = start + 1;
        let is_enabled = app.hex_view.changed_bytes.contains_key(&start);
        let mut orig = Vec::new();
        let mut patched = Vec::new();

        let orig_b = if start < buf_len { buffer[start] } else { 0 };
        let patch_b = if is_enabled {
            app.hex_view.changed_bytes.get(&start).copied().unwrap_or(orig_b)
        } else {
            app.hex_view.disabled_bytes.get(&start).copied().unwrap_or(orig_b)
        };
        orig.push(orig_b);
        patched.push(patch_b);

        i += 1;
        while i < offsets.len()
            && offsets[i] == end
            && app.hex_view.changed_bytes.contains_key(&offsets[i]) == is_enabled
        {
            let cur = offsets[i];
            let orig_b = if cur < buf_len { buffer[cur] } else { 0 };
            let patch_b = if is_enabled {
                app.hex_view.changed_bytes.get(&cur).copied().unwrap_or(orig_b)
            } else {
                app.hex_view.disabled_bytes.get(&cur).copied().unwrap_or(orig_b)
            };
            orig.push(orig_b);
            patched.push(patch_b);
            end += 1;
            i += 1;
        }

        items.push(PatchItem {
            offset: start,
            size: end - start,
            orig_bytes: orig,
            patched_bytes: patched,
            enabled: is_enabled,
        });
    }

    items
}

fn format_hex_bytes(bytes: &[u8], max_len: usize) -> String {
    if bytes.len() <= max_len {
        bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
    } else {
        let first: Vec<String> = bytes[..max_len].iter().map(|b| format!("{:02X}", b)).collect();
        format!("{}..", first.join(" "))
    }
}

fn fixed_centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width, height)
}

pub fn open_patches_dialog(app: &mut App) {
    let items = collect_patches(app);
    let count = items.len();
    let total_bytes: usize = items.iter().map(|p| p.size).sum();
    app.hex_view.patches_dialog.items = items;
    app.hex_view.patches_dialog.selected_index = 0;
    app.state = UIState::DialogPatches;
    app.dialog_renderer = Some(|app, frame| draw_patches_dialog(app, frame, app.screen));
    App::log(
        app,
        format!("Patches: {} chunk(s), {} byte(s)", count, total_bytes),
    );
}

pub fn draw_patches_dialog(app: &mut App, frame: &mut Frame, area: Rect) {
    let popup_area = fixed_centered_rect(96, 20, area);
    frame.render_widget(Clear, popup_area);

    let dialog_style = app.config.theme.dialog;
    let dialog = &app.hex_view.patches_dialog;

    let total_chunks = dialog.items.len();
    let total_bytes: usize = dialog.items.iter().map(|p| p.size).sum();

    let title = format!(
        " {} ({} {}, {} B) ",
        crate::i18n::M::PatchesTitle.tr(app.config.lang),
        total_chunks,
        crate::i18n::M::FoundCount.tr(app.config.lang),
        total_bytes
    );

    let outer_block = Block::default()
        .title(title)
        .title_bottom(crate::i18n::M::PatchesFooterKeys.tr(app.config.lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = outer_block.inner(popup_area);
    frame.render_widget(outer_block, popup_area);

    if total_chunks == 0 {
        let empty_msg = crate::i18n::M::NoPatches.tr(app.config.lang);
        let para = Paragraph::new(empty_msg)
            .alignment(ratatui::layout::Alignment::Center)
            .style(dialog_style);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
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
        let max_len = dialog.items.iter()
            .map(|item| {
                let va = app.get_va(item.offset);
                if va >= 0x1_0000_0000 { format!("{:X}", va).len() } else { 8 }
            })
            .max()
            .unwrap_or(9);
        max_len.max(crate::i18n::M::LblAddress.tr(app.config.lang).chars().count())
    } else {
        8
    };

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
                Span::styled(" St", bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled("───┼", sep_style)),
        ]),
        Cell::from(vec![
            Line::from(vec![
                Span::styled(format!(" {:<width$} ", crate::i18n::M::LblAddress.tr(app.config.lang), width = addr_col_len), bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled(format!("{:─>width$}┼", "", width = addr_col_len + 2), sep_style)),
        ]),
        Cell::from(vec![
            Line::from(Span::styled(format!(" {}", crate::i18n::M::LblOriginalToPatched.tr(app.config.lang)), bold_header)),
            Line::from(Span::styled("────────────────────────────────────────────────────────────────────────", sep_style)),
        ]),
        Cell::from(vec![
            Line::from(vec![
                Span::styled("│ ", sep_style),
                Span::styled(crate::i18n::M::LblSize.tr(app.config.lang), bold_header),
            ]),
            Line::from(Span::styled("┼──────────", sep_style)),
        ]),
    ];
    let header = Row::new(header_cells).style(dialog_style).height(2);

    let visible_rows = chunks[0].height.saturating_sub(3) as usize; // 2 for header + 1 padding
    let sel = dialog.selected_index.min(total_chunks.saturating_sub(1));
    let half = visible_rows / 2;
    let start_idx = if sel > half {
        (sel - half).min(total_chunks.saturating_sub(visible_rows))
    } else {
        0
    };
    let end_idx = (start_idx + visible_rows).min(total_chunks);
    let select_offset = Some(sel.saturating_sub(start_idx));

    let disabled_fg = app.config.theme.dimmed.fg.unwrap_or(Color::DarkGray);

    let mut rows = Vec::with_capacity(end_idx - start_idx);
    for idx in start_idx..end_idx {
        let item = &dialog.items[idx];
        let is_selected = idx == dialog.selected_index;
        let is_enabled = item.enabled;

        let base_row_style = if is_selected {
            app.config.theme.highlight.add_modifier(Modifier::BOLD)
        } else {
            dialog_style
        };

        let row_style = if is_enabled {
            base_row_style
        } else {
            base_row_style.fg(disabled_fg)
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

        let orig_hex = format_hex_bytes(&item.orig_bytes, 8);
        let patch_hex = format_hex_bytes(&item.patched_bytes, 8);
        let size_str = format!("{} B", item.size);

        let dot_char = if is_enabled { "●" } else { "○" };
        let dot_fg = if is_enabled { Color::Green } else { disabled_fg };

        let cells = if is_selected {
            let dot_style = if is_enabled {
                row_style.fg(Color::Green)
            } else {
                row_style.fg(disabled_fg)
            };
            vec![
                Cell::new(format!(" {:>2} │", idx + 1)).style(row_style),
                Cell::new(format!(" {} │", dot_char)).style(dot_style),
                Cell::new(format!(" {:<width$} │", addr_str, width = addr_col_len)).style(row_style),
                Cell::new(format!(" {} -> {}", orig_hex, patch_hex)).style(row_style),
                Cell::new(format!("│ {:>5}   ", size_str)).style(row_style),
            ]
        } else {
            let patched_fg = if is_enabled {
                app.config.theme.changed_bytes.fg.unwrap_or(Color::Rgb(0xDB, 0xBC, 0x7F))
            } else {
                disabled_fg
            };
            let patched_style = Style::default().fg(patched_fg).add_modifier(if is_enabled { Modifier::BOLD } else { Modifier::empty() });

            vec![
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {:>2} ", idx + 1), row_style),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {} ", dot_char), Style::default().fg(dot_fg)),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {:<width$} ", addr_str, width = addr_col_len), row_style),
                    Span::styled("│", sep_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(orig_hex, row_style),
                    Span::styled(" -> ", row_style),
                    Span::styled(patch_hex, patched_style),
                ])).style(row_style),
                Cell::from(Line::from(vec![
                    Span::styled("│ ", sep_style),
                    Span::styled(format!("{:>5}   ", size_str), row_style),
                ])).style(row_style),
            ]
        };
        rows.push(Row::new(cells).style(row_style));
    }

    let widths = [
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Length((addr_col_len + 3) as u16),
        Constraint::Min(32),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .style(dialog_style);

    let mut table_state = TableState::default();
    table_state.select(select_offset);

    frame.render_stateful_widget(table, chunks[0], &mut table_state);
}

fn patch_row_as_text(app: &App, item: &PatchItem) -> String {
    let is_disasm = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let addr = if is_disasm {
        app.get_va(item.offset)
    } else {
        item.offset as u64
    };
    let addr_str = format!("{:X}", addr);
    let orig_hex = item.orig_bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
    let patch_hex = item.patched_bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
    let state_str = if item.enabled { "●" } else { "○" };
    format!("[{}] 0x{}: {} -> {} ({} bytes)", state_str, addr_str, orig_hex, patch_hex, item.size)
}

pub fn dialog_patches_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Mouse(mouse) = event {
        if let ratatui::crossterm::event::MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) = mouse.kind {
            let total = app.hex_view.patches_dialog.items.len();
            if total > 0 {
                let popup_height = 20u16.min(app.screen.height);
                let dialog_y = app.screen.y + (app.screen.height.saturating_sub(popup_height)) / 2;
                let table_start_y = dialog_y + 4; // border (1) + margin (1) + header (2)
                let visible_rows = (popup_height.saturating_sub(6)) as usize;

                let sel = app.hex_view.patches_dialog.selected_index.min(total - 1);
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
                        app.hex_view.patches_dialog.selected_index = clicked_index;
                    }
                }
            }
        }
        match mouse.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollUp => {
                if app.hex_view.patches_dialog.selected_index > 0 {
                    app.hex_view.patches_dialog.selected_index -= 1;
                }
            }
            ratatui::crossterm::event::MouseEventKind::ScrollDown => {
                let cnt = app.hex_view.patches_dialog.items.len();
                if cnt > 0 && app.hex_view.patches_dialog.selected_index + 1 < cnt {
                    app.hex_view.patches_dialog.selected_index += 1;
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    let Event::Key(key) = event else { return Ok(false) };
    if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
        return Ok(false);
    }

    let total = app.hex_view.patches_dialog.items.len();

    match key.code {
        KeyCode::Esc => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
        }
        KeyCode::Enter => {
            if let Some(item) = app.hex_view.patches_dialog.items.get(app.hex_view.patches_dialog.selected_index) {
                let target_ofs = item.offset;
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                app.goto_with_history(target_ofs, false);
            }
        }
        KeyCode::Char(' ') => {
            if let Some(item) = app.hex_view.patches_dialog.items.get(app.hex_view.patches_dialog.selected_index) {
                let offset = item.offset;
                let size = item.size;
                let was_enabled = item.enabled;
                if was_enabled {
                    for ofs in offset..offset.saturating_add(size) {
                        if let Some(b) = app.hex_view.changed_bytes.remove(&ofs) {
                            app.hex_view.disabled_bytes.insert(ofs, b);
                        }
                    }
                    App::log(app, format!("Disabled patch at 0x{:X} ({} byte(s))", offset, size));
                } else {
                    for ofs in offset..offset.saturating_add(size) {
                        if let Some(b) = app.hex_view.disabled_bytes.remove(&ofs) {
                            app.hex_view.changed_bytes.insert(ofs, b);
                        }
                    }
                    App::log(app, format!("Enabled patch at 0x{:X} ({} byte(s))", offset, size));
                }
                app.view_generation = app.view_generation.wrapping_add(1);
                app.hex_view.patches_dialog.items = collect_patches(app);
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let any_enabled = app.hex_view.patches_dialog.items.iter().any(|p| p.enabled);
            if any_enabled {
                for (ofs, b) in app.hex_view.changed_bytes.drain() {
                    app.hex_view.disabled_bytes.insert(ofs, b);
                }
                App::log(app, "Disabled all patches".to_string());
            } else {
                for (ofs, b) in app.hex_view.disabled_bytes.drain() {
                    app.hex_view.changed_bytes.insert(ofs, b);
                }
                App::log(app, "Enabled all patches".to_string());
            }
            app.view_generation = app.view_generation.wrapping_add(1);
            app.hex_view.patches_dialog.items = collect_patches(app);
        }
        KeyCode::F(4) => {
            if let Some(item) = app.hex_view.patches_dialog.items.get(app.hex_view.patches_dialog.selected_index) {
                let target_ofs = item.offset;
                app.dialog_renderer = None;
                app.goto_with_history(target_ofs, false);
                crate::hex::edit_dialog::open_edit_dialog(app);
            }
        }
        KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('r') | KeyCode::Char('R') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(item) = app.hex_view.patches_dialog.items.get(app.hex_view.patches_dialog.selected_index) {
                let offset = item.offset;
                let size = item.size;
                for ofs in offset..offset.saturating_add(size) {
                    app.hex_view.changed_bytes.remove(&ofs);
                    app.hex_view.disabled_bytes.remove(&ofs);
                }
                app.view_generation = app.view_generation.wrapping_add(1);
                app.hex_view.patches_dialog.items = collect_patches(app);
                let new_len = app.hex_view.patches_dialog.items.len();
                if app.hex_view.patches_dialog.selected_index >= new_len {
                    app.hex_view.patches_dialog.selected_index = new_len.saturating_sub(1);
                }
                App::log(app, format!("Reverted patch at 0x{:X} ({} byte(s))", offset, size));
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT) => {
            if total > 0 {
                let rows: Vec<String> = app.hex_view.patches_dialog.items
                    .iter()
                    .map(|item| patch_row_as_text(app, item))
                    .collect();
                let text = rows.join("\r\n");
                app.copy_to_clipboard(text, format!("{} patch(es)", total));
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(item) = app.hex_view.patches_dialog.items.get(app.hex_view.patches_dialog.selected_index) {
                let text = patch_row_as_text(app, item);
                app.copy_to_clipboard(text, "1 patch".to_string());
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.hex_view.patches_dialog.selected_index > 0 {
                app.hex_view.patches_dialog.selected_index -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if total > 0 && app.hex_view.patches_dialog.selected_index + 1 < total {
                app.hex_view.patches_dialog.selected_index += 1;
            }
        }
        KeyCode::PageUp => {
            app.hex_view.patches_dialog.selected_index =
                app.hex_view.patches_dialog.selected_index.saturating_sub(10);
        }
        KeyCode::PageDown => {
            if total > 0 {
                app.hex_view.patches_dialog.selected_index =
                    (app.hex_view.patches_dialog.selected_index + 10).min(total - 1);
            }
        }
        KeyCode::Home => {
            app.hex_view.patches_dialog.selected_index = 0;
        }
        KeyCode::End if total > 0 => {
            app.hex_view.patches_dialog.selected_index = total - 1;
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn make_test_app(bytes: &[u8]) -> (std::path::PathBuf, App) {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("dz6_patches_{}_{}", std::process::id(), id));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.bin");
        std::fs::write(&path, bytes).unwrap();
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, false).unwrap();
        (dir, app)
    }

    #[test]
    fn collect_patches_groups_contiguous_modifications() {
        let mut bytes = vec![0x90u8; 0x100];
        bytes[0x10] = 0x74;
        bytes[0x11] = 0x05;
        let (dir, mut app) = make_test_app(&bytes);

        // Make two patches: 0x10..0x12 (2 bytes) and 0x50 (1 byte)
        app.hex_view.changed_bytes.insert(0x10, 0x90);
        app.hex_view.changed_bytes.insert(0x11, 0x90);
        app.hex_view.changed_bytes.insert(0x50, 0xCC);

        let items = collect_patches(&app);
        assert_eq!(items.len(), 2);

        assert_eq!(items[0].offset, 0x10);
        assert_eq!(items[0].size, 2);
        assert_eq!(items[0].orig_bytes, vec![0x74, 0x05]);
        assert_eq!(items[0].patched_bytes, vec![0x90, 0x90]);

        assert_eq!(items[1].offset, 0x50);
        assert_eq!(items[1].size, 1);
        assert_eq!(items[1].orig_bytes, vec![0x90]);
        assert_eq!(items[1].patched_bytes, vec![0xCC]);

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patches_dialog_navigation_and_revert() {
        let bytes = vec![0x00u8; 0x100];
        let (dir, mut app) = make_test_app(&bytes);

        app.hex_view.changed_bytes.insert(0x20, 0xAA);
        app.hex_view.changed_bytes.insert(0x40, 0xBB);

        open_patches_dialog(&mut app);
        assert_eq!(app.state, UIState::DialogPatches);
        assert_eq!(app.hex_view.patches_dialog.items.len(), 2);
        assert_eq!(app.hex_view.patches_dialog.selected_index, 0);

        // Move down
        let event_down = Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_patches_events(&mut app, &event_down);
        assert_eq!(app.hex_view.patches_dialog.selected_index, 1);

        // Enter jumps to 0x40
        let event_enter = Event::Key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_patches_events(&mut app, &event_enter);
        assert_eq!(app.state, UIState::Normal);
        assert_eq!(app.hex_view.offset, 0x40);

        // Open dialog again and revert 0x40
        open_patches_dialog(&mut app);
        app.hex_view.patches_dialog.selected_index = 1;
        let event_del = Event::Key(KeyEvent {
            code: KeyCode::Delete,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_patches_events(&mut app, &event_del);
        assert_eq!(app.hex_view.patches_dialog.items.len(), 1);
        assert_eq!(app.hex_view.patches_dialog.items[0].offset, 0x20);
        assert_eq!(app.hex_view.changed_bytes.len(), 1);
        assert!(!app.hex_view.changed_bytes.contains_key(&0x40));

        // F4 on 0x20 opens edit data dialog
        let event_f4 = Event::Key(KeyEvent {
            code: KeyCode::F(4),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_patches_events(&mut app, &event_f4);
        assert_eq!(app.state, UIState::DialogEditData);
        assert_eq!(app.hex_view.offset, 0x20);

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_draw_patches_dialog_renders_correctly() {
        use ratatui::{Terminal, backend::TestBackend};
        let bytes = vec![0x00u8; 0x100];
        let (dir, mut app) = make_test_app(&bytes);

        app.hex_view.changed_bytes.insert(0x20, 0xAA);
        open_patches_dialog(&mut app);

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            let area = f.area();
            draw_patches_dialog(&mut app, f, area);
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

        assert!(rendered.contains("Patches"));
        assert!(rendered.contains("00000020"));
        assert!(rendered.contains("00 -> AA"));
        assert!(rendered.contains("1 B"));

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
    fn test_64bit_patches_trims_leading_zeros() {
        use ratatui::{Terminal, backend::TestBackend};
        let bytes = vec![0x00u8; 0x100000];
        let (dir, mut app) = make_test_app(&bytes);
        app.hex_view.show_va = true;
        app.config.bitness_override = Some(64);
        app.image_base_override = Some(0x140000000);

        app.hex_view.changed_bytes.insert(0x5EB18, 0x90);
        open_patches_dialog(&mut app);

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            let area = f.area();
            draw_patches_dialog(&mut app, f, area);
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

    #[test]
    fn test_patches_dialog_enable_disable_toggle() {
        let bytes = vec![0x00u8; 0x100];
        let (dir, mut app) = make_test_app(&bytes);

        app.hex_view.changed_bytes.insert(0x20, 0xAA);
        open_patches_dialog(&mut app);

        assert_eq!(app.hex_view.patches_dialog.items.len(), 1);
        assert_eq!(app.hex_view.patches_dialog.items[0].enabled, true);

        // Press Space to toggle to Disable
        let event_space = Event::Key(KeyEvent {
            code: KeyCode::Char(' '),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_patches_events(&mut app, &event_space);

        // Should be disabled now
        assert_eq!(app.hex_view.patches_dialog.items.len(), 1);
        assert_eq!(app.hex_view.patches_dialog.items[0].enabled, false);
        assert!(!app.hex_view.changed_bytes.contains_key(&0x20));
        assert!(app.hex_view.disabled_bytes.contains_key(&0x20));

        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            let area = f.area();
            draw_patches_dialog(&mut app, f, area);
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

        assert!(rendered.contains("○"));
        assert!(!rendered.contains("Esc: Close"));
        assert!(rendered.contains("Space: Toggle") || rendered.contains("Space: 활성/비활성"));

        // Press Space again to re-enable
        let _ = dialog_patches_events(&mut app, &event_space);
        assert_eq!(app.hex_view.patches_dialog.items[0].enabled, true);
        assert!(app.hex_view.changed_bytes.contains_key(&0x20));
        assert!(!app.hex_view.disabled_bytes.contains_key(&0x20));

        terminal.draw(|f| {
            let area = f.area();
            draw_patches_dialog(&mut app, f, area);
        }).unwrap();

        let buffer2 = terminal.backend().buffer();
        let rendered2: String = (0..buffer2.area.height)
            .map(|y| {
                (0..buffer2.area.width)
                    .map(|x| buffer2[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered2.contains("●"));

        app.file_info.mmap = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
