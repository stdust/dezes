//! Shared "boxed field row" rendering used by the Edit Data, Wildcard Pattern
//! Replace and Find Pattern dialogs.
//!
//! Every dialog that uses this draws one bordered box containing a fixed
//! number of `"  <label> : [ ... ]  "` rows plus an optional status line,
//! all sharing a single outer border instead of each field being its own
//! nested bordered box. Factored out so the three dialogs can't visually
//! drift apart (same label alignment, same trailing margin, same wide-char
//! safe padding, same cursor placement).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tui_input::Input;
use unicode_width::UnicodeWidthChar;

use crate::app::App;

/// Desired number of visible display-columns in each value field.
pub const CONTENT_WIDTH: usize = 40;

/// Blank columns kept after the closing `]` and before the dialog's right border.
pub const TRAILING_MARGIN: usize = 2;

/// One row's worth of static description: its label and whether it is
/// currently focused.
pub struct FieldRow<'a> {
    pub label: &'a str,
    pub input: &'a Input,
    pub focused: bool,
    /// Selection range (start, end) in char indices, already resolved by the
    /// caller (only relevant when `focused` is true).
    pub selection: Option<(usize, usize)>,
}

/// Width of the longest label among `rows`, used to line up every `:` and `[`.
pub fn label_width(labels: &[&str]) -> usize {
    labels.iter().map(|l| l.chars().count()).max().unwrap_or(0)
}

/// `"  <label> : ["` - the fixed part of a field row, before the value.
pub fn field_prefix(label: &str, label_width: usize) -> String {
    format!("  {:<width$} : [", label, width = label_width)
}

/// Outer box size for `n_fields` rows plus `status_rows` status lines, sized so
/// every field lines up per [`field_prefix`] / [`CONTENT_WIDTH`] /
/// [`TRAILING_MARGIN`].
pub fn box_size_rows(label_width: usize, n_fields: usize, status_rows: usize, avail: Rect) -> (u16, u16) {
    // Computed the same way `field_prefix` builds its string, so the box is
    // always exactly wide enough for it: "  " (2) + label (padded to
    // `label_width`) + " : [" (4).
    let prefix_len = field_prefix("", label_width).chars().count();
    let inner_width = (prefix_len + CONTENT_WIDTH + 1 + TRAILING_MARGIN) as u16; // +1 "]"
    let width = (inner_width + 2).min(avail.width); // +2 outer border

    // Leading blank line + one row per field + trailing blank line (+ status lines).
    //
    // Two status rows is what the Find and Replace dialogs use: a result row and a
    // permanent key hint. They shared one row before, and the counter and the hint
    // together are wider than the box - so finding something wiped the hint off the
    // screen just as the new keys became relevant.
    let mut content_rows = n_fields as u16 + 2;
    if status_rows > 0 {
        content_rows += 1 + status_rows as u16; // blank separator + the rows
    }
    let height = (content_rows + 2).min(avail.height); // +2 outer border

    (width, height)
}

/// Renders the outer bordered box with `title`, sized via [`box_size`], sitting
/// slightly above dead-center of `frame.area()`. Returns the inner content
/// area (i.e. already past the border).
pub fn draw_box(app: &App, frame: &mut Frame, title: String, label_width: usize, n_fields: usize, has_status: bool) -> Rect {
    draw_box_rows(app, frame, title, label_width, n_fields, usize::from(has_status))
}

/// [`draw_box`] with an explicit number of status rows.
///
/// The Find and Replace dialogs use two: a result row and a permanent key hint.
pub fn draw_box_rows(
    app: &App,
    frame: &mut Frame,
    title: String,
    label_width: usize,
    n_fields: usize,
    status_rows: usize,
) -> Rect {
    let dialog_style = app.config.theme.dialog;
    let (width, height) = box_size_rows(label_width, n_fields, status_rows, frame.area());
    let area = centered_rect_above(width, height, frame.area());

    frame.render_widget(Clear, area);

    let outer_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(dialog_style)
        .border_style(dialog_style.add_modifier(Modifier::BOLD));

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);
    inner_area
}

/// Centered rect that sits a bit above dead-center rather than perfectly
/// centered: only ~1/3 of the vertical slack is used as the top margin
/// (instead of half), so the dialog sits slightly higher on screen.
///
/// Public so other dialogs outside this module (e.g. the "Add New Section"
/// size prompt) that want to match the same slightly-above-center placement
/// don't have to reimplement it.
pub fn centered_rect_above(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let slack_y = r.height.saturating_sub(height);
    let y = r.y + slack_y / 3;

    Rect::new(x, y, width, height)
}

/// Screen rect the dialog occupies, without drawing it.
///
/// The Find and Replace dialogs move the cursor in the view *behind* them, so they
/// need to know which rows they are covering: a match scrolled to a row under the
/// box is found but invisible, which reads as "only the match counter changes".
pub fn dialog_rect(app: &App, label_width: usize, n_fields: usize, status_rows: usize) -> Rect {
    let (width, height) = box_size_rows(label_width, n_fields, status_rows, app.screen);
    centered_rect_above(width, height, app.screen)
}

/// Scrolls the hex view so `offset` is on a row the dialog is not covering, and
/// leaves the cursor there.
///
/// `goto` alone is not enough: it scrolls only when the target is outside the
/// current page, so a match a few rows down landed behind the box.
pub fn reveal_behind_dialog(app: &mut App, offset: usize, dialog: Rect) {
    app.goto_with_history(offset, false);

    let bpl = app.config.hex_mode_bytes_per_line.max(1);
    // Row 0 of the content area is one below the ruler.
    let content_top = 1u16;
    let row = content_top + app.hex_view.cursor.y as u16;
    let dialog_bottom = dialog.y.saturating_add(dialog.height);

    if row < dialog.y || row >= dialog_bottom {
        return; // already visible
    }

    // One row of clearance under the box, reached by starting the page earlier -
    // which moves everything, including the match, further down the screen.
    let wanted = dialog_bottom.saturating_add(1);
    let shift_rows = wanted.saturating_sub(row) as usize;
    let new_start = app.reader.page_start.saturating_sub(shift_rows * bpl);
    if new_start == app.reader.page_start {
        return; // already at the top of the file, nothing to give
    }
    app.reader.page_start = new_start;
    app.reader.page_end = new_start
        .saturating_add(app.reader.page_current_size)
        .saturating_sub(1);
    // Recompute the cursor's screen position against the page that is now shown.
    app.goto_with_history(offset, false);
}

/// Draws one field row (`"  Label : [ value  ]  "`) at `row` inside
/// `inner_area` (row 0 = the line right after the leading blank line).
///
/// Returns the on-screen cursor position for this row when `field.focused` is
/// true, so the caller can call `frame.set_cursor_position` once after all
/// rows are drawn.
pub fn draw_field_row(
    app: &App,
    frame: &mut Frame,
    inner_area: Rect,
    row: u16,
    label_width: usize,
    field: &FieldRow,
) -> Option<(u16, u16)> {
    let dialog_style = app.config.theme.dialog;
    let y = inner_area.y + 1 + row; // +1 for the leading blank line
    if y >= inner_area.y + inner_area.height {
        return None;
    }
    let line_area = Rect::new(inner_area.x, y, inner_area.width, 1);

    let label_style = if field.focused {
        dialog_style.add_modifier(Modifier::BOLD)
    } else {
        dialog_style.patch(Style::default().fg(Color::DarkGray))
    };

    // `visual_scroll` answers in terminal *columns*; the code below needs a
    // *character* index to skip by and to compare against the selection, which is
    // also character-based. The two are only the same while every character is one
    // column wide - with CP949 or UTF-16 text in the field they diverge, and the
    // row then scrolled to the wrong character and highlighted the wrong span.
    let scroll_cols = field.input.visual_scroll(CONTENT_WIDTH);
    let chars: Vec<char> = field.input.value().chars().collect();

    let scroll = char_index_at_column(&chars, scroll_cols);

    // Select visible chars by *display width*, not by character count: some
    // fields (e.g. CP949/UTF-16 text) can contain wide characters that occupy
    // two terminal columns each. Counting characters instead of columns would
    // let a wide char push the row past `CONTENT_WIDTH`, shoving the closing
    // `]` off the edge of the box.
    let mut visible_chars: Vec<char> = Vec::new();
    let mut shown_width = 0usize;
    for &ch in chars.iter().skip(scroll) {
        let w = ch.width().unwrap_or(0);
        if shown_width + w > CONTENT_WIDTH {
            break;
        }
        visible_chars.push(ch);
        shown_width += w;
    }

    let prefix = field_prefix(field.label, label_width);
    let prefix_len = prefix.chars().count();
    let mut spans = vec![Span::styled(prefix, label_style)];

    if let Some((sel_start, sel_end)) = field.selection {
        for (i, &ch) in visible_chars.iter().enumerate() {
            let idx = scroll + i;
            let style = if idx >= sel_start && idx < sel_end {
                app.config.theme.highlight
            } else {
                dialog_style
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
    } else {
        let visible_val: String = visible_chars.iter().collect();
        spans.push(Span::styled(visible_val, dialog_style));
    }

    // Pad by display width, not char count, so the closing bracket stays in
    // the same column on every row regardless of wide characters.
    if shown_width < CONTENT_WIDTH {
        spans.push(Span::styled(" ".repeat(CONTENT_WIDTH - shown_width), dialog_style));
    }
    spans.push(Span::styled("]", label_style));
    // Blank margin between "]" and the dialog's right border.
    spans.push(Span::styled(" ".repeat(TRAILING_MARGIN), dialog_style));

    frame.render_widget(Paragraph::new(Line::from(spans)).style(dialog_style), line_area);

    if field.focused {
        // Both terms are columns here: `visual_cursor` and `scroll_cols`. Using
        // the character-based `scroll` would put the cursor in the wrong column
        // once a wide character is scrolled past.
        let cursor_x = line_area.x
            + prefix_len as u16
            + (field.input.visual_cursor().saturating_sub(scroll_cols)) as u16;
        Some((cursor_x, line_area.y))
    } else {
        None
    }
}

/// Index of the first character at or after terminal column `column`.
///
/// `tui_input::Input::visual_scroll` answers in columns, but the render loop needs
/// a character index - to skip by, and to compare against the character-based
/// selection range. The two coincide only while every character is one column
/// wide; with CP949 or UTF-16 text in the field they diverge, and the row then
/// scrolled to the wrong character and highlighted the wrong span.
fn char_index_at_column(chars: &[char], column: usize) -> usize {
    let mut cols = 0usize;
    for (i, ch) in chars.iter().enumerate() {
        if cols >= column {
            return i;
        }
        cols += ch.width().unwrap_or(0);
    }
    chars.len()
}

/// Draws the status line, right after the blank separator below the fields.
pub fn draw_status_row(app: &App, frame: &mut Frame, inner_area: Rect, n_fields: u16, text: &str) {
    draw_status_row_at(app, frame, inner_area, n_fields, 0, text);
}

/// Draws status row `row` (0-based) for dialogs that reserved more than one.
pub fn draw_status_row_at(app: &App, frame: &mut Frame, inner_area: Rect, n_fields: u16, row: u16, text: &str) {
    let status_y = inner_area.y + 1 + n_fields + 1 + row;
    if status_y < inner_area.y + inner_area.height {
        let status_area = Rect::new(inner_area.x, status_y, inner_area.width, 1);
        frame.render_widget(
            Paragraph::new(text.to_string()).style(app.config.theme.dialog),
            status_area,
        );
    }
}

#[cfg(test)]
mod column_index_tests {
    use super::char_index_at_column;

    /// With single-width text, columns and character indices agree - which is why
    /// the confusion went unnoticed.
    #[test]
    fn ascii_columns_equal_char_indices() {
        let chars: Vec<char> = "abcdef".chars().collect();
        for col in 0..=6 {
            assert_eq!(char_index_at_column(&chars, col), col);
        }
    }

    /// With wide characters they do not. Each Hangul syllable is two columns, so
    /// column 4 is character 2 - treating the column as an index skipped twice as
    /// far into the text.
    #[test]
    fn wide_characters_shift_the_mapping() {
        let chars: Vec<char> = "가나다라".chars().collect();
        assert_eq!(char_index_at_column(&chars, 0), 0);
        assert_eq!(char_index_at_column(&chars, 2), 1);
        assert_eq!(char_index_at_column(&chars, 4), 2);
        assert_eq!(char_index_at_column(&chars, 6), 3);
    }

    /// A column that falls inside a wide character resolves to that character,
    /// never past it.
    #[test]
    fn a_column_inside_a_wide_char_resolves_to_it() {
        let chars: Vec<char> = "가나".chars().collect();
        assert_eq!(char_index_at_column(&chars, 1), 1);
        assert_eq!(char_index_at_column(&chars, 3), 2);
    }

    /// Mixed text is the realistic case for these dialogs.
    #[test]
    fn mixed_width_text() {
        let chars: Vec<char> = "ab가cd".chars().collect();
        // columns: a=0, b=1, 가=2..3, c=4, d=5
        assert_eq!(char_index_at_column(&chars, 0), 0);
        assert_eq!(char_index_at_column(&chars, 1), 1);
        assert_eq!(char_index_at_column(&chars, 2), 2);
        assert_eq!(char_index_at_column(&chars, 4), 3);
        assert_eq!(char_index_at_column(&chars, 5), 4);
    }

    /// Past the end, and empty input, must clamp rather than index out of range.
    #[test]
    fn out_of_range_columns_clamp() {
        let chars: Vec<char> = "abc".chars().collect();
        assert_eq!(char_index_at_column(&chars, 99), 3);
        assert_eq!(char_index_at_column(&[], 0), 0);
        assert_eq!(char_index_at_column(&[], 5), 0);
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::*;
    use crate::app::App;
    use ratatui::layout::Rect;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A 64 KiB file, 16 bytes per line, 30-row screen.
    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dezes_reveal");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("r_{}.bin", SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0u8; 0x10000]).expect("fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.config.hex_mode_bytes_per_line = 16;
        app.screen = Rect::new(0, 0, 100, 30);
        app.reader.page_current_size = 16 * 27;
        app
    }

    /// Sets the visible page the way the draw loop does.
    ///
    /// `page_end` matters: `goto` treats an offset past it as off-page and scrolls,
    /// so leaving it stale makes every call look like a jump.
    fn show_page(app: &mut App, start: usize) {
        app.reader.page_start = start;
        app.reader.page_end = start + app.reader.page_current_size - 1;
    }

    /// The page follows the match, which is the bug behind "only the counter
    /// changes": the old code set the cursor coordinates by hand and never touched
    /// `page_start`, so a match outside the visible page was never scrolled to.
    #[test]
    fn the_page_follows_the_match() {
        let mut app = app_with_file();
        show_page(&mut app, 0);
        let dialog = Rect::new(20, 7, 60, 8);

        reveal_behind_dialog(&mut app, 0x2289, dialog);

        assert_eq!(app.hex_view.offset, 0x2289);
        let page_start = app.reader.page_start;
        assert!(
            page_start <= 0x2289 && 0x2289 < page_start + app.reader.page_current_size,
            "0x2289 is not on the page starting at 0x{:X}",
            page_start
        );
    }

    /// A match that would land on a row the dialog covers is pushed below it.
    #[test]
    fn a_hidden_row_is_scrolled_clear_of_the_dialog() {
        let mut app = app_with_file();
        let dialog = Rect::new(20, 7, 60, 8); // covers screen rows 7..15

        // Row 8 of the content area = screen row 9, inside the dialog.
        show_page(&mut app, 0x1000);
        let target = 0x1000 + 8 * 16;
        reveal_behind_dialog(&mut app, target, dialog);

        let row = 1 + app.hex_view.cursor.y as u16;
        assert!(
            row >= dialog.y + dialog.height,
            "the match is still under the dialog: row {} vs dialog {}..{}",
            row,
            dialog.y,
            dialog.y + dialog.height
        );
        assert_eq!(app.hex_view.offset, target, "the cursor itself must not move");
    }

    /// A row already clear of the dialog is left where it is - no jumping about.
    #[test]
    fn a_visible_row_is_left_alone() {
        let mut app = app_with_file();
        let dialog = Rect::new(20, 7, 60, 8);

        show_page(&mut app, 0x1000);
        let target = 0x1000 + 20 * 16; // screen row 21, below the box
        let before = app.reader.page_start;
        reveal_behind_dialog(&mut app, target, dialog);

        assert_eq!(app.reader.page_start, before, "the page must not move");
    }

    /// Near the start of the file there is nothing to scroll, and that must not
    /// underflow or loop.
    #[test]
    fn the_top_of_the_file_is_handled() {
        let mut app = app_with_file();
        let dialog = Rect::new(20, 0, 60, 8);

        show_page(&mut app, 0);
        reveal_behind_dialog(&mut app, 0x20, dialog);

        assert_eq!(app.reader.page_start, 0);
        assert_eq!(app.hex_view.offset, 0x20);
    }
}
