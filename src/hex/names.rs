use ratatui::{
    Frame,
    crossterm::event::KeyModifiers,
    layout::Alignment,
    symbols,
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
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
    let re = if !pattern.is_empty() {
        regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .ok()
    } else {
        None
    };

    app.hex_view
        .comment_name_list
        .iter()
        .filter(|cmt| {
            if pattern.is_empty() {
                true
            } else if let Some(r) = &re {
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
    let items: Vec<ListItem> = shown
        .iter()
        .map(|cmt| ListItem::from(format!("{:08X}  {}", cmt.offset, cmt.comment)))
        .collect();

    let list = List::new(items)
        .style(app.config.theme.dialog)
        .block(
            Block::bordered()
                .title(format!(
                    " {} ({}) ",
                    crate::i18n::M::NamesTitle.tr(app.config.lang),
                    count
                ))
                .title_bottom(crate::i18n::M::NamesFooter.tr(app.config.lang))
                .title_alignment(Alignment::Center)
                .padding(Padding::horizontal(1)),
        )
        .highlight_style(app.config.theme.highlight)
        .repeat_highlight_symbol(true);

    let width = frame.area().width / 2;
    let height = frame.area().height / 2 + 4;
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);
    frame.render_stateful_widget(list, dialog_area, &mut app.hex_view.names_list_state);
}

pub fn dialog_names_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
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
            KeyCode::Char('f') | KeyCode::Char('/') => {
                app.state = UIState::DialogNamesRegex;
                app.dialog_2nd_renderer = Some(dialog_names_regex_draw);
            }
            KeyCode::Char('o') => {
                app.hex_view.comment_name_list.sort_by_key(|x| x.offset);
            }
            KeyCode::Char('n') => {
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
}
