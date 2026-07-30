use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier},
    widgets::{Block, Borders, Cell, Clear, Row, Table, TableState},
};
use ratatui::crossterm::event::{Event, KeyCode};
use std::io::Result;

use crate::{app::App, editor::UIState};
use super::xref::{find_xrefs, XrefItem};

#[derive(Default)]
pub struct XrefDialog {
    pub target_va: u64,
    pub items: Vec<XrefItem>,
    pub selected_index: usize,
    /// Set when the scan hit `MAX_XREF_ITEMS`, so the title can say the list is
    /// incomplete rather than implying that's all there is.
    pub truncated: bool,
}

impl XrefDialog {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn load_for_va(&mut self, app: &App, va: u64) {
        self.target_va = va;
        self.items = find_xrefs(app, va);
        self.truncated = crate::disasm::xref::is_truncated(&self.items);
        self.selected_index = 0;
    }

    pub fn reset(&mut self) {
        self.target_va = 0;
        self.items.clear();
        self.selected_index = 0;
        self.truncated = false;
    }
}

/// Runs the xref search at the cursor and opens the dialog.
///
/// Every caller has to set five fields *and* install `dialog_renderer`; when
/// that last line was missing (the Ctrl+R handler in `global/events.rs`), the
/// state still switched to `DialogXref`, so keys were routed to the dialog's
/// handler while nothing was drawn - the app looked frozen even though Esc
/// still worked. Keeping the sequence in one place is what stops the three
/// entry points (Ctrl+R globally, `r` in Hex, `r` in Disasm) from drifting
/// apart again.
pub fn open_xref_dialog(app: &mut App) {
    let target_va = app.get_va(app.hex_view.offset);

    // Address 0 is not a real target: it matches every zeroed immediate and
    // memory displacement in the file. On a non-executable that is most of
    // them, so the dialog used to fill with hits that reference nothing. This
    // happens whenever `get_va` has no PE/ELF layout to work from.
    if target_va == 0 {
        App::log(
            app,
            "Xref: no target address at the cursor (address 0)".to_string(),
        );
        crate::beep!();
        return;
    }

    let items = find_xrefs(app, target_va);
    let truncated = crate::disasm::xref::is_truncated(&items);
    App::log(
        app,
        format!(
            "Xref search for 0x{:X}: {} found{}",
            target_va,
            items.len(),
            if truncated { " (limit reached)" } else { "" }
        ),
    );
    app.disasm_xref_dialog.target_va = target_va;
    app.disasm_xref_dialog.items = items;
    app.disasm_xref_dialog.truncated = truncated;
    app.disasm_xref_dialog.selected_index = 0;
    app.state = UIState::DialogXref;
    app.dialog_renderer = Some(|app, frame| draw_xref_dialog(app, frame, app.screen));
}

/// Half-open range of result indices to build rows for, given the selected index
/// and how many rows fit.
///
/// Keeps the selection inside the returned range - that is the property the
/// renderer depends on, and it is checked directly in the tests rather than
/// inferred from what ends up on screen.
fn visible_window(total: usize, selected: usize, visible_rows: usize) -> (usize, usize) {
    if total == 0 || visible_rows == 0 {
        return (0, 0);
    }
    let selected = selected.min(total - 1);
    let half = visible_rows / 2;
    let start = if selected > half {
        (selected - half).min(total.saturating_sub(visible_rows))
    } else {
        0
    };
    // A few rows beyond the visible height so the table has something to scroll
    // into rather than ending abruptly at the last drawn line.
    let end = (start + visible_rows + 5).min(total);
    (start, end)
}

fn fixed_centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width, height)
}

pub fn draw_xref_dialog(app: &mut App, frame: &mut Frame, area: Rect) {
    let popup_area = fixed_centered_rect(82, 18, area);

    // Clear background
    frame.render_widget(Clear, popup_area);

    let dialog_style = app.config.theme.dialog;
    let dialog = &app.disasm_xref_dialog;

    let lang = app.config.lang;
    let title = format!(
        " {} 0x{:X} ({} {}{}) ",
        crate::i18n::M::XrefTitle.tr(lang),
        dialog.target_va,
        dialog.items.len(),
        crate::i18n::M::FoundCount.tr(lang),
        if dialog.truncated {
            crate::i18n::M::XrefLimitReached.tr(lang)
        } else {
            ""
        }
    );

    let outer_block = Block::default()
        .title(title)
        .title_bottom(crate::i18n::M::XrefFooterKeys.tr(lang))
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = outer_block.inner(popup_area);
    frame.render_widget(outer_block, popup_area);

    let is_64 = app.is_64();
    let addr_col_len = if is_64 { 16 } else { 10 };

    let header_cells = [
        Cell::new(crate::i18n::M::LblType.tr(lang)).style(dialog_style.add_modifier(Modifier::BOLD)),
        Cell::new(crate::i18n::M::LblAddress.tr(lang)).style(dialog_style.add_modifier(Modifier::BOLD)),
        Cell::new(crate::i18n::M::LblInstruction.tr(lang)).style(dialog_style.add_modifier(Modifier::BOLD)),
    ];
    let header = Row::new(header_cells).style(dialog_style).bottom_margin(1);

    // Only the visible slice is turned into rows.
    //
    // This used to build a `Row` for all of them - up to `MAX_XREF_ITEMS` (5000),
    // each cloning its instruction text - on every rendered frame, for a box 18
    // rows tall. Measured at 11.5 ms per frame in a release build with a full
    // result list, which is felt on every keystroke while the popup is open.
    // `string_ref_dialog` already windowed; the two had diverged.
    let visible_rows = inner_area.height.saturating_sub(2) as usize;
    let total = dialog.items.len();
    let (start_idx, end_idx) = visible_window(total, dialog.selected_index, visible_rows);

    let mut rows = Vec::with_capacity(end_idx.saturating_sub(start_idx));
    for idx in start_idx..end_idx {
        let item = &dialog.items[idx];
        let is_selected = idx == dialog.selected_index;
        let row_style = if is_selected {
            app.config.theme.highlight
        } else {
            dialog_style
        };

        let va_str = if is_64 {
            format!("0x{:016X}", item.va)
        } else {
            format!("0x{:08X}", item.va)
        };

        let type_style = row_style.fg(Color::Black).add_modifier(Modifier::BOLD);

        let cells = vec![
            Cell::new(item.ref_type.as_str()).style(type_style),
            Cell::new(va_str).style(row_style),
            Cell::new(item.instr_text.clone()).style(row_style),
        ];

        rows.push(Row::new(cells).style(row_style));
    }

    let widths = [
        Constraint::Length(6),
        Constraint::Length(addr_col_len),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(2)
        .style(dialog_style);

    let mut table_state = TableState::default();
    if !dialog.items.is_empty() {
        // Relative to the windowed slice, not the whole list.
        table_state.select(Some(dialog.selected_index.saturating_sub(start_idx)));
    }

    frame.render_stateful_widget(table, inner_area, &mut table_state);
}

/// One row as `type<TAB>address<TAB>instruction`, the three columns on screen.
fn item_as_tsv(item: &XrefItem, is_64: bool) -> String {
    let va = if is_64 {
        format!("{:016X}", item.va)
    } else {
        format!("{:08X}", item.va)
    };
    format!("{}\t{}\t{}", item.ref_type.as_str(), va, item.instr_text)
}

/// The whole result list, and how many rows it is.
///
/// A function rather than inline in the key handler so the result can be asserted
/// without the real clipboard, which parallel tests contend over.
fn all_rows_as_tsv(app: &App) -> (String, usize) {
    let is_64 = app.is_64();
    let rows: Vec<String> = app
        .disasm_xref_dialog
        .items
        .iter()
        .map(|item| item_as_tsv(item, is_64))
        .collect();
    // CRLF: a Windows clipboard, and a spreadsheet pasting LF-only text puts every
    // row in one cell.
    (rows.join("\r\n"), rows.len())
}

pub fn dialog_xref_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != ratatui::crossterm::event::KeyEventKind::Press {
            return Ok(false);
        }

        let item_cnt = app.disasm_xref_dialog.items.len();

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.disasm_xref_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            KeyCode::Up => {
                if item_cnt > 0 && app.disasm_xref_dialog.selected_index > 0 {
                    app.disasm_xref_dialog.selected_index -= 1;
                }
                return Ok(false);
            }
            KeyCode::Down => {
                if item_cnt > 0 && app.disasm_xref_dialog.selected_index + 1 < item_cnt {
                    app.disasm_xref_dialog.selected_index += 1;
                }
                return Ok(false);
            }
            KeyCode::PageUp => {
                if item_cnt > 0 {
                    app.disasm_xref_dialog.selected_index =
                        app.disasm_xref_dialog.selected_index.saturating_sub(10);
                }
                return Ok(false);
            }
            // `y` the selected row, `Y` the whole list, tab-separated in the same
            // three columns the table shows. A cross-reference list is something
            // people work through outside the editor - it was the one panel with no
            // way of getting the addresses out.
            KeyCode::Char('y') => {
                let is_64 = app.is_64();
                let text = app
                    .disasm_xref_dialog
                    .items
                    .get(app.disasm_xref_dialog.selected_index)
                    .map(|item| item_as_tsv(item, is_64))
                    .unwrap_or_default();
                app.copy_to_clipboard(text, "1 cross-reference".to_string());
                return Ok(false);
            }
            KeyCode::Char('Y') => {
                let (text, count) = all_rows_as_tsv(app);
                app.copy_to_clipboard(text, format!("{} cross-reference(s)", count));
                return Ok(false);
            }
            KeyCode::PageDown => {
                if item_cnt > 0 {
                    app.disasm_xref_dialog.selected_index =
                        (app.disasm_xref_dialog.selected_index + 10).min(item_cnt - 1);
                }
                return Ok(false);
            }
            KeyCode::Enter => {
                if item_cnt > 0 {
                    let sel = app.disasm_xref_dialog.selected_index;
                    let target_offset = app.disasm_xref_dialog.items[sel].offset;
                    let target_va = app.disasm_xref_dialog.items[sel].va;
                    // A PTR or RVA hit is a *value* sitting in a data section, not an
                    // instruction. Landing on it in the Disassembly view would decode
                    // a pointer table as code, so those go to the Hex view, where
                    // eight bytes of address read as what they are.
                    let is_value = matches!(
                        app.disasm_xref_dialog.items[sel].ref_type,
                        super::xref::XrefType::Ptr | super::xref::XrefType::Rva
                    );
                    if is_value && app.editor_view == crate::editor::AppView::Disasm {
                        app.editor_view = crate::editor::AppView::Hex;
                        app.last_primary_view = crate::editor::AppView::Hex;
                        app.prev_editor_view = crate::editor::AppView::Hex;
                    }

                    app.reader.page_start = target_offset;
                    app.goto(target_offset);
                    app.align_page_for_view();
                    App::log(app, format!("Jumped to Xref at 0x{:X}", target_va));
                }
                app.disasm_xref_dialog.reset();
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                return Ok(false);
            }
            _ => {}
        }
    }
    Ok(false)
}

#[cfg(test)]
mod xref_dialog_tests {
    use crate::editor::UIState;

    /// `open_xref_dialog` must leave the dialog drawable.
    ///
    /// Regression test for Ctrl+R: it used to set `UIState::DialogXref` without
    /// installing `dialog_renderer`, which routed keys to the dialog handler
    /// while drawing nothing - indistinguishable from a hang.
    #[test]
    fn opening_installs_a_renderer() {
        let mut app = crate::app::App::new();
        // The test binary itself is an executable image, so `get_va` has a real
        // layout to work from without depending on any fixture file.
        let exe = std::env::current_exe().expect("test exe path");
        app.load_file(exe.to_str().expect("utf-8 path"), 0, true)
            .expect("load the test binary");
        assert_ne!(
            app.get_va(app.hex_view.offset),
            0,
            "precondition: a real image base is needed for the search to run"
        );

        super::open_xref_dialog(&mut app);
        // `assert!` rather than `assert_eq!`: UIState doesn't derive Debug.
        assert!(app.state == UIState::DialogXref);
        assert!(
            app.dialog_renderer.is_some(),
            "state switched to DialogXref with no renderer: the dialog would \
             never be drawn and the app would appear frozen"
        );
    }

    /// The window must always contain the selection, and never exceed what is
    /// needed to fill the box.
    ///
    /// This replaced building a `Row` for every result on every frame - 11.5 ms
    /// per frame in a release build with a full 5000-item list, felt on each
    /// keystroke while the popup is open.
    #[test]
    fn the_window_always_contains_the_selection() {
        for &visible in &[1usize, 5, 16, 30] {
            for &total in &[0usize, 1, 3, 17, 100, 5000] {
                for &selected in &[0usize, 1, 8, 50, 99, 4999, 100_000] {
                    let (start, end) = super::visible_window(total, selected, visible);

                    assert!(start <= end, "inverted window");
                    assert!(end <= total, "window past the end of the list");

                    if total == 0 {
                        assert_eq!((start, end), (0, 0));
                        continue;
                    }

                    let clamped = selected.min(total - 1);
                    assert!(
                        clamped >= start && clamped < end,
                        "selection {} outside window {}..{} (total {}, visible {})",
                        clamped,
                        start,
                        end,
                        total,
                        visible
                    );

                    // Bounded work per frame: never more than a screenful plus a
                    // small scroll margin.
                    assert!(
                        end - start <= visible + 5,
                        "window {}..{} is larger than the box needs (visible {})",
                        start,
                        end,
                        visible
                    );
                }
            }
        }
    }

    /// A zero-height box must not produce rows at all.
    #[test]
    fn no_rows_when_nothing_is_visible() {
        assert_eq!(super::visible_window(500, 10, 0), (0, 0));
    }

    /// The popup must show the selected row wherever it is in a long list.
    ///
    /// Rows are now windowed to the visible slice - previously every result got a
    /// `Row` on every frame, 11.5 ms per frame with a full list - so the table's
    /// selected index has to be translated into that slice. Getting it wrong shows
    /// the highlight on the wrong line or none at all.
    #[test]
    fn the_selected_row_is_visible_at_any_position() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = crate::app::App::new();
        app.disasm_xref_dialog.target_va = 0x1_4000_1000;
        app.disasm_xref_dialog.items = (0..2000)
            .map(|i| crate::disasm::xref::XrefItem {
                offset: i,
                va: 0x1_4000_0000 + i as u64,
                ref_type: crate::disasm::xref::XrefType::Call,
                // A distinctive marker per row so it can be found on screen.
                instr_text: format!("marker{:04}", i),
            })
            .collect();

        for selected in [0usize, 1, 17, 500, 1998, 1999] {
            app.disasm_xref_dialog.selected_index = selected;

            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
            terminal
                .draw(|f| {
                    app.screen = f.area();
                    super::draw_xref_dialog(&mut app, f, f.area());
                })
                .expect("draw");

            let buf = terminal.backend().buffer().clone();
            let marker = format!("marker{:04}", selected);
            let lines: Vec<String> = (0..30)
                .map(|y| {
                    (0..100)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect();

            let row_y = lines
                .iter()
                .position(|line| line.contains(&marker))
                .unwrap_or_else(|| panic!("selected row {} is not on screen", selected));

            // And the highlight has to be on *that* row. Checking only that the
            // text is on screen passes even with an untranslated select index,
            // because the windowing puts the row on screen either way - the
            // highlight is what actually proves the index was mapped into the
            // visible slice.
            let highlight_bg = app
                .config
                .theme
                .highlight
                .bg
                .expect("the theme's highlight style needs a background");
            let highlighted_rows: Vec<usize> = (0..30)
                .filter(|&y| {
                    lines[y].contains("marker")
                        && (0..100).any(|x| buf[(x, y as u16)].bg == highlight_bg)
                })
                .collect();
            assert_eq!(
                highlighted_rows,
                vec![row_y],
                "exactly the row holding the selected item must be highlighted (selected {})",
                selected
            );
        }
    }

    /// Closing must clear both halves of the state, so a later frame can't draw
    /// a dialog the user already dismissed.
    #[test]
    fn reset_clears_items() {
        let mut d = super::XrefDialog::new();
        d.target_va = 0x140001000;
        d.selected_index = 7;
        d.truncated = true;
        d.reset();
        assert_eq!(d.target_va, 0);
        assert_eq!(d.selected_index, 0);
        assert!(d.items.is_empty());
        assert!(!d.truncated);
    }

    /// With no file loaded `get_va` yields 0, which is not a searchable target:
    /// it matches every zeroed immediate and displacement. The dialog must not
    /// open at all in that case.
    #[test]
    fn zero_target_does_not_open_the_dialog() {
        let mut app = crate::app::App::new();
        assert_eq!(app.get_va(app.hex_view.offset), 0, "precondition");
        super::open_xref_dialog(&mut app);
        assert!(app.state == UIState::Normal);
        assert!(app.dialog_renderer.is_none());
        assert!(app.disasm_xref_dialog.items.is_empty());
    }
}

#[cfg(test)]
mod xref_copy_tests {
    use super::*;
    use crate::disasm::xref::{XrefItem, XrefType};
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn item(va: u64) -> XrefItem {
        XrefItem {
            offset: 0x400,
            va,
            ref_type: XrefType::Call,
            instr_text: "call 0x140001000".to_string(),
        }
    }

    /// Three columns, the same ones the table shows.
    #[test]
    fn a_row_matches_the_table_columns() {
        let it = item(0x140001234);
        assert_eq!(
            item_as_tsv(&it, true),
            "CALL\t0000000140001234\tcall 0x140001000"
        );
        assert_eq!(item_as_tsv(&item(0x40001234), false), "CALL\t40001234\tcall 0x140001000");
    }

    /// `y` copies one, `Y` copies the list, and neither closes the dialog.
    #[test]
    fn y_and_shift_y_report_what_they_copied() {
        let mut app = crate::app::App::new();
        app.disasm_xref_dialog.target_va = 0x140001000;
        app.disasm_xref_dialog.items = (0..4).map(|i| item(0x140002000 + i)).collect();
        app.state = UIState::DialogXref;

        let press = |app: &mut crate::app::App, code: KeyCode| {
            let key = KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = dialog_xref_events(app, &Event::Key(key));
        };

        // What `Y` hands to the clipboard, asserted without the clipboard itself:
        // it is a shared OS resource and parallel tests fought over it.
        let (text, count) = all_rows_as_tsv(&app);
        assert_eq!(count, 4);
        assert_eq!(text.lines().count(), 4);
        for line in text.lines() {
            assert_eq!(line.split('\t').count(), 3, "row: {:?}", line);
        }

        // Both keys report, and neither closes the dialog. A machine with no
        // clipboard is a legitimate case, so only the fact that it was reported is
        // asserted here.
        app.logs.clear();
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Char('Y'));
        assert_eq!(app.logs.len(), 2, "logs: {:?}", app.logs);
        assert!(app.logs.iter().all(|l| l.contains("clipboard")), "logs: {:?}", app.logs);
        assert!(app.state == UIState::DialogXref, "copying closed the dialog");
        assert_eq!(app.disasm_xref_dialog.items.len(), 4, "and it kept the results");
    }

    /// `q` still closes: it is a letter key like the copy keys, and the two must not
    /// have swapped.
    #[test]
    fn q_still_closes() {
        let mut app = crate::app::App::new();
        app.disasm_xref_dialog.items = vec![item(0x140002000)];
        app.state = UIState::DialogXref;

        let key = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = dialog_xref_events(&mut app, &Event::Key(key));

        assert!(app.state == UIState::Normal);
        assert!(app.disasm_xref_dialog.items.is_empty());
    }
}