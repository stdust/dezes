use ratatui::{
    Frame,
    crossterm::event::KeyModifiers,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Padding, Paragraph, Row, Table, TableState},
};

use ratatui::crossterm::event::{Event, KeyCode};
use std::io::Result;

use crate::{app::App, editor::UIState, util::center_widget};

/// Checks if `haystack` contains `needle` ignoring ASCII case, without allocating.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let needle_len = needle.len();
    if needle_len == 0 {
        return true;
    }
    if haystack.len() < needle_len {
        return false;
    }
    haystack.as_bytes().windows(needle_len).any(|window| {
        window.eq_ignore_ascii_case(needle.as_bytes())
    })
}

/// The comments the list is currently showing, in list order.
///
/// The draw code, Enter, Delete and F2 all have to agree on which entry row *n*
/// is - the regex filter means the list is not the raw `comment_name_list` - so
/// the filtering lives here instead of being spelled out at each call site.
fn filtered_comments(app: &App) -> Vec<crate::hex::comment::Comment> {
    let pattern = app.hex_view.names_regex.trim();
    if pattern.is_empty() {
        return app.hex_view.comment_name_list.clone();
    }

    let re = crate::hex::strings::build_safe_regex(pattern);
    app.hex_view
        .comment_name_list
        .iter()
        .filter(|cmt| {
            if let Some(r) = &re {
                crate::util::has_nonempty_match(r, &cmt.comment)
            } else {
                contains_ignore_ascii_case(&cmt.comment, pattern)
            }
        })
        .cloned()
        .collect()
}

/// Offset of the highlighted row, or `None` when the list is empty or the
/// selection is stale.
fn selected_offset(app: &App) -> Option<usize> {
    let choice = app.hex_view.names_list_state.selected()?;
    filtered_comments(app).get(choice).map(|cmt| cmt.offset)
}

pub fn dialog_names_draw(app: &mut App, frame: &mut Frame) {
    let shown = filtered_comments(app);
    let count = shown.len();
    let is_disasm = app.editor_view == crate::editor::AppView::Disasm;
    let is_64 = app.is_64();
    let use_va = app.hex_view.show_va || is_disasm;
    let lang = app.config.lang;
    let dialog_style = app.config.theme.dialog;

    let width = 84u16.min(frame.area().width.saturating_sub(4)).max(55);
    let height = 20u16.min(frame.area().height.saturating_sub(2)).max(10);
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);

    let title = format!(
        " {} ({} {}) ",
        crate::i18n::M::NamesTitle.tr(lang),
        count,
        crate::i18n::M::FoundCount.tr(lang)
    );

    let block = Block::bordered()
        .title(title)
        .title_bottom(crate::i18n::M::NamesFooter.tr(lang))
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    if count == 0 {
        let empty_msg = crate::i18n::M::NoNames.tr(lang);
        let para = Paragraph::new(empty_msg)
            .alignment(Alignment::Center)
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

    let addr_col_len = if use_va {
        if is_64 {
            let max_len = shown.iter()
                .map(|cmt| {
                    let va = app.get_va(cmt.offset);
                    if va >= 0x1_0000_0000 { format!("{:X}", va).len() } else { 8 }
                })
                .max()
                .unwrap_or(9);
            max_len.max(crate::i18n::M::LblAddress.tr(lang).chars().count())
        } else {
            8.max(crate::i18n::M::LblAddress.tr(lang).chars().count())
        }
    } else {
        8.max(crate::i18n::M::LblAddress.tr(lang).chars().count())
    };

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
                Span::styled(format!(" {:<width$} ", crate::i18n::M::LblAddress.tr(lang), width = addr_col_len), bold_header),
                Span::styled("│", sep_style),
            ]),
            Line::from(Span::styled(format!("{:─>width$}┼", "", width = addr_col_len + 2), sep_style)),
        ]),
        Cell::from(vec![
            Line::from(Span::styled(format!(" {}", crate::i18n::M::LblLabel.tr(lang)), bold_header)),
            Line::from(Span::styled("────────────────────────────────────────────────────────────────────────", sep_style)),
        ]),
    ];
    let header = Row::new(header_cells).style(dialog_style).height(2);

    let visible_rows = inner_area.height.saturating_sub(2) as usize;
    let sel = app.hex_view.names_list_state.selected().unwrap_or(0).min(count.saturating_sub(1));
    let half = visible_rows / 2;
    let start_idx = if sel > half {
        (sel - half).min(count.saturating_sub(visible_rows))
    } else {
        0
    };
    let end_idx = (start_idx + visible_rows).min(count);

    let mut rows = Vec::with_capacity(end_idx.saturating_sub(start_idx));
    for (idx, cmt) in shown.iter().enumerate().take(end_idx).skip(start_idx) {
        let is_selected = Some(idx) == app.hex_view.names_list_state.selected();
        let row_style = if is_selected {
            app.config.theme.highlight.add_modifier(Modifier::BOLD)
        } else {
            dialog_style
        };

        let addr_str = if use_va {
            let va = app.get_va(cmt.offset);
            if is_64 && va >= 0x1_0000_0000 {
                format!("{:X}", va)
            } else {
                format!("{:08X}", va)
            }
        } else {
            format!("{:08X}", cmt.offset)
        };

        let cells = if is_selected {
            vec![
                Cell::new(format!(" {:>2} │", idx + 1)).style(row_style),
                Cell::new(format!(" {:<width$} │", addr_str, width = addr_col_len)).style(row_style),
                Cell::new(format!(" {}", cmt.comment)).style(row_style),
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
                Cell::new(format!(" {}", cmt.comment)).style(row_style),
            ]
        };

        rows.push(Row::new(cells).style(row_style));
    }

    let widths = [
        Constraint::Length(5),
        Constraint::Length((addr_col_len + 3) as u16),
        Constraint::Min(0),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .style(dialog_style);

    let mut table_state = TableState::default();
    if count > 0 {
        table_state.select(Some(sel.saturating_sub(start_idx)));
    }

    frame.render_stateful_widget(table, inner_area, &mut table_state);
}

pub fn dialog_names_events(app: &mut App, event: &Event) -> Result<bool> {
    let shown = filtered_comments(app);
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
            KeyCode::Char(';') => {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
                crate::hex::comment::open_comment_dialog(app);
            }
            KeyCode::Down => {
                app.hex_view.names_list_state.select_next();
            }
            KeyCode::Up => {
                app.hex_view.names_list_state.select_previous();
            }
            KeyCode::PageDown => {
                app.hex_view.names_list_state.scroll_down_by(30);
            }
            KeyCode::PageUp => {
                app.hex_view.names_list_state.scroll_up_by(30);
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.hex_view.names_list_state.select_first();
                } else if let Some(n) = app.hex_view.names_list_state.selected() {
                    // we show 30 strings at a time, so this will select
                    // the string at the top of the list
                    let new_index = n.saturating_sub(29);
                    app.hex_view.names_list_state.select(Some(new_index));
                }
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.hex_view.names_list_state.select_last();
                } else if let Some(n) = app.hex_view.names_list_state.selected() {
                    // Clamped to the last entry. An index past the end leaves the
                    // list with nothing selected, so Enter then does nothing and
                    // the dialog looks stuck.
                    let last = app.hex_view.comment_name_list.len().saturating_sub(1);
                    app.hex_view
                        .names_list_state
                        .select(Some((n + 29).min(last)));
                }
            }
            KeyCode::Enter => {
                if let Some(offset) = selected_offset(app) {
                    app.goto(offset);
                }
                app.state = UIState::Normal;
                app.dialog_renderer = None;
            }
            // Delete the highlighted comment, staying in the list so several can be
            // cleaned up in a row.
            //
            // This replaces a bare 'D' that wiped *every* comment in the file with
            // no confirmation and no undo - one keystroke away from the arrow keys.
            KeyCode::Delete => {
                match selected_offset(app) {
                    Some(offset) => {
                        app.hex_view.comments.remove(&offset);
                        app.hex_view
                            .comment_name_list
                            .retain(|entry| entry.offset != offset);
                        App::log(app, format!("Deleted the comment at 0x{:X}", offset));
                        app.view_generation = app.view_generation.wrapping_add(1);
                        app.persist_annotations();

                        // Keep a valid selection: removing the last row leaves the
                        // index past the end, and then Enter and Delete both do
                        // nothing while the dialog looks alive.
                        let remaining = filtered_comments(app).len();
                        if remaining == 0 {
                            app.hex_view.names_list_state.select(None);
                        } else if let Some(sel) = app.hex_view.names_list_state.selected()
                            && sel >= remaining
                        {
                            app.hex_view.names_list_state.select(Some(remaining - 1));
                        }
                    }
                    None => crate::beep!(),
                }
            }
            // Edit the highlighted comment (F2, the same key that starts editing in
            // the hex view). The cursor moves to that offset first, so the comment
            // box names the address it is about to change.
            KeyCode::F(2) => {
                match selected_offset(app) {
                    Some(offset) => {
                        app.dialog_2nd_renderer = None;
                        app.goto(offset);
                        // The comment box reads and writes `hex_view.offset`, and
                        // `goto` refuses an offset outside the mapping - so without
                        // this check an entry pointing past EOF would silently open
                        // the box for wherever the cursor happened to be, and Enter
                        // would move the comment there.
                        if app.hex_view.offset == offset {
                            crate::hex::comment::open_comment_dialog(app);
                        } else {
                            let message = crate::i18n::fill(
                                crate::i18n::M::ErrCommentOutside.tr(app.config.lang),
                                &[&format!("{:X}", offset)],
                            );
                            app.error(message);
                        }
                    }
                    None => crate::beep!(),
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('c') | KeyCode::Char('C')
                if key.code == KeyCode::Char('y')
                    || key.code == KeyCode::Char('Y')
                    || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let is_disasm = app.editor_view == crate::editor::AppView::Disasm;
                let is_64 = app.is_64();
                let use_va = app.hex_view.show_va || is_disasm;
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    let text = shown
                        .iter()
                        .map(|cmt| {
                            let addr_str = if use_va {
                                let va = app.get_va(cmt.offset);
                                if is_64 && va >= 0x1_0000_0000 {
                                    format!("{:X}", va)
                                } else {
                                    format!("{:08X}", va)
                                }
                            } else {
                                format!("{:08X}", cmt.offset)
                            };
                            format!("{}\t{}", addr_str, cmt.comment)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let cnt = shown.len();
                    app.copy_to_clipboard(text, format!("{} name(s)", cnt));
                } else if let Some(choice) = app.hex_view.names_list_state.selected()
                    && let Some(cmt) = shown.get(choice)
                {
                    let addr_str = if use_va {
                        let va = app.get_va(cmt.offset);
                        if is_64 && va >= 0x1_0000_0000 {
                            format!("{:X}", va)
                        } else {
                            format!("{:08X}", va)
                        }
                    } else {
                        format!("{:08X}", cmt.offset)
                    };
                    app.copy_to_clipboard(format!("{}\t{}", addr_str, cmt.comment), "1 name".to_string());
                }
            }
            KeyCode::Char('f') | KeyCode::Char('/') => {
                app.state = UIState::DialogNamesRegex;
                app.dialog_2nd_renderer = Some(dialog_names_regex_draw);
            }
            KeyCode::Char('o') => {
                app.hex_view.comment_name_list.sort_by_key(|x| x.offset);
            }
            KeyCode::Char('n') if !key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.hex_view
                    .comment_name_list
                    .sort_by_key(|x| x.comment.clone());
            }
            _ => {}
        }
    }
    Ok(false)
}

/// The Names filter box and its selection anchor.
fn names_filter_field(app: &mut App) -> (&mut tui_input::Input, &mut Option<usize>) {
    (
        &mut app.hex_view.names_regex_input,
        &mut app.hex_view.names_filter_anchor,
    )
}

pub fn dialog_names_regex_draw(app: &mut App, frame: &mut Frame) {
    let para = Paragraph::new(crate::text_field::render_line(
        &app.hex_view.names_regex_input,
        app.hex_view.names_filter_anchor,
        app.config.theme.main,
        app.config.theme.highlight,
    ));

    let dialog_area = center_widget(frame.area().width / 3, 3, frame.area());

    let block = Block::new()
        .title(crate::i18n::M::RegexTitle.tr(app.config.lang))
        .borders(Borders::ALL)
        .border_set(symbols::border::PLAIN)
        .style(app.config.theme.main)
        .padding(Padding::horizontal(1));

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(para.block(block), dialog_area);
    let x = app.hex_view.names_regex_input.visual_cursor();
    frame.set_cursor_position((dialog_area.x + 2 + x as u16, dialog_area.y + 1));
}

pub fn dialog_names_regex_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.dialog_2nd_renderer = None;
                app.state = UIState::DialogNames;
            }
            KeyCode::Enter => {
                app.hex_view.names_regex = String::from(app.hex_view.names_regex_input.value());
                app.dialog_2nd_renderer = None;
                app.state = UIState::DialogNames;
                app.hex_view.names_list_state.select(Some(0));
            }
            // Shift+arrows, Shift+Home/End, and Ctrl+C/X/V over the block.
            _ => {
                crate::text_field::handle_key(app, names_filter_field, event);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod names_key_tests {
    use super::*;
    use crate::commands::Commands;
    use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    /// A real file is mapped because F2 moves the cursor to the entry's offset, and
    /// `goto` will not leave the mapping.
    fn app_with_comments() -> App {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);

        let dir = std::env::temp_dir().join(format!("dz6_names_keys_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("n_{}.bin", SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0x90u8; 0x400]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        Commands::comment(&mut app, 0x100, "first".to_string());
        Commands::comment(&mut app, 0x200, "second".to_string());
        Commands::comment(&mut app, 0x300, "third".to_string());
        app.state = UIState::DialogNames;
        app.hex_view.names_list_state.select(Some(1));
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        let event = Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_names_events(app, &event);
    }

    /// Delete removes the highlighted comment and nothing else.
    ///
    /// The key it replaces was a bare 'D' that cleared every comment in the file,
    /// with no confirmation, one key away from the arrows.
    #[test]
    fn delete_removes_only_the_selected_comment() {
        let mut app = app_with_comments();

        press(&mut app, KeyCode::Delete);

        assert!(!app.hex_view.comments.contains_key(&0x200), "the selected one goes");
        assert!(app.hex_view.comments.contains_key(&0x100), "the others stay");
        assert!(app.hex_view.comments.contains_key(&0x300));
        assert_eq!(app.hex_view.comment_name_list.len(), 2, "the list entry goes too");
        assert!(
            app.state == UIState::DialogNames,
            "the dialog stays open so several can be deleted in a row"
        );
    }

    /// Deleting the last row must leave a selection that still works.
    #[test]
    fn the_selection_stays_valid_after_deleting_the_last_row() {
        let mut app = app_with_comments();
        app.hex_view.names_list_state.select(Some(2));

        press(&mut app, KeyCode::Delete);
        assert_eq!(app.hex_view.names_list_state.selected(), Some(1));

        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::Delete);
        assert_eq!(
            app.hex_view.names_list_state.selected(),
            None,
            "an empty list must not keep an index"
        );
        assert!(app.hex_view.comments.is_empty());
    }

    /// F2 opens the comment box for the highlighted entry, pre-filled.
    #[test]
    fn f2_edits_the_selected_comment() {
        let mut app = app_with_comments();

        press(&mut app, KeyCode::F(2));

        assert!(app.state == UIState::DialogComment);
        assert_eq!(app.hex_view.comment_input.value(), "second");
        assert_eq!(
            app.hex_view.offset, 0x200,
            "the cursor follows, so the box names the offset it will change"
        );
    }

    /// The regex filter decides what row *n* is, so Delete and F2 have to see the
    /// filtered list - not the raw one.
    #[test]
    fn the_filter_decides_which_entry_a_row_is() {
        let mut app = app_with_comments();
        app.hex_view.names_regex = "third".to_string();
        app.hex_view.names_list_state.select(Some(0));

        assert_eq!(selected_offset(&app), Some(0x300));

        press(&mut app, KeyCode::Delete);
        assert!(!app.hex_view.comments.contains_key(&0x300));
        assert_eq!(app.hex_view.comments.len(), 2);
    }

    #[test]
    fn y_and_shift_y_copy_names() {
        let mut app = app_with_comments();
        app.hex_view.names_list_state.select(Some(0));

        let event_y = Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_names_events(&mut app, &event_y);
        assert!(app.logs.last().unwrap().contains("1 name"));

        let event_shift_y = Event::Key(KeyEvent {
            code: KeyCode::Char('Y'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = dialog_names_events(&mut app, &event_shift_y);
        assert!(app.logs.last().unwrap().contains("3 name(s)"));
    }

    #[test]
    fn names_dialog_uses_va_when_show_va_is_true() {
        let mut app = app_with_comments();
        app.hex_view.show_va = true;
        app.image_base_override = Some(0x140000000);

        // When show_va is true, get_va is used
        let va = app.get_va(0x100);
        assert_eq!(va, 0x140000100);
    }

    #[test]
    fn names_dialog_64bit_renders_without_leading_zeros() {
        let mut app = app_with_comments();
        app.config.bitness_override = Some(64);
        app.hex_view.show_va = true;
        app.image_base_override = Some(0x140000000);

        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            dialog_names_draw(&mut app, f);
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

        assert!(rendered.contains("140000100"));
        assert!(!rendered.contains("0000000140000100"));
        assert!(rendered.contains("│"));
        assert!(rendered.contains("────┼"));
    }

    #[test]
    fn names_dialog_empty_shows_no_names_message() {
        let mut app = App::new();
        app.state = UIState::DialogNames;

        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("terminal");
        terminal.draw(|f| {
            dialog_names_draw(&mut app, f);
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

        assert!(rendered.contains("No names or comments"));
        assert!(!rendered.contains("────┼"));
    }
}
