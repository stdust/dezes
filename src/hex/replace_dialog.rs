use ratatui::{Frame, layout::Rect};
use tui_input::Input;

use crate::hex::field_box::{self, FieldRow};
use crate::app::App;

#[derive(Default)]
pub struct ReplaceDialog {
    pub search_input: Input,
    pub replace_input: Input,
    /// Character a Shift-selection started from, or `None`. One per dialog: only
    /// the focused field can hold a block, and switching fields drops it.
    pub anchor: Option<usize>,
    /// 0: Search pattern input, 1: Replace pattern input
    pub active_field: usize,
    pub error_message: Option<String>,
    pub status_message: Option<String>,
}

impl ReplaceDialog {
    pub fn new() -> Self {
        Self {
            search_input: Input::default(),
            replace_input: Input::default(),
            anchor: None,
            active_field: 0,
            error_message: None,
            status_message: None,
        }
    }

    pub fn reset(&mut self) {
        self.search_input = Input::default();
        self.replace_input = Input::default();
        self.anchor = None;
        self.active_field = 0;
        self.error_message = None;
        self.status_message = None;
    }
}

pub fn draw_replace_dialog(app: &mut App, frame: &mut Frame, _area: Rect) {
    use crate::i18n::M;
    let lang = app.config.lang;
    let field_labels = [M::LblSearch.tr(lang), M::LblReplace.tr(lang)];
    let label_width = field_box::label_width(&field_labels);
    // Two status rows, as in the Find dialog: the result of the last search or
    // replace, and a hint that does not get overwritten by it.
    let inner_area = field_box::draw_box_rows(
        app,
        frame,
        format!(" {} (Ctrl+H) ", M::ReplacePatternTitle.tr(lang)),
        label_width,
        field_labels.len(),
        2,
    );

    let active_field = app.hex_view.replace_dialog.active_field;
    let inputs = [&app.hex_view.replace_dialog.search_input, &app.hex_view.replace_dialog.replace_input];

    let anchor = app.hex_view.replace_dialog.anchor;
    let mut cursor_pos: Option<(u16, u16)> = None;
    for (row, label) in field_labels.iter().enumerate() {
        let focused = active_field == row;
        let field = FieldRow {
            label,
            input: inputs[row],
            focused,
            selection: if focused {
                crate::text_field::selection(inputs[row], anchor)
            } else {
                None
            },
        };
        if let Some(pos) = field_box::draw_field_row(app, frame, inner_area, row as u16, label_width, &field) {
            cursor_pos = Some(pos);
        }
    }

    let dialog = &app.hex_view.replace_dialog;
    let result_text = if let Some(ref err) = dialog.error_message {
        format!("  {}: {}", M::LblError.tr(lang), err)
    } else if let Some(ref msg) = dialog.status_message {
        format!("  {}", msg)
    } else {
        String::new()
    };
    let n_rows = field_labels.len() as u16;
    field_box::draw_status_row_at(app, frame, inner_area, n_rows, 0, &result_text);
    field_box::draw_status_row_at(
        app,
        frame,
        inner_area,
        n_rows,
        1,
        &format!("  {}", M::ReplaceHint.tr(lang)),
    );

    if let Some((x, y)) = cursor_pos {
        frame.set_cursor_position((x, y));
    }
}
