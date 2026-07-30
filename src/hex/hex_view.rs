use std::collections::{HashMap, HashSet};

use ratatui::widgets::{ListState, TableState};
use serde::{Deserialize, Serialize};
use tui_input::Input;

use crate::hex::{blocks::ColoredBlock, comment::Comment};

// used in hex view struct to track the cursor position
#[derive(Default, Debug)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

#[derive(Default, Serialize, Deserialize)]
pub struct HexView {
    #[serde(skip)]
    #[allow(dead_code)]
    pub ascii_state: TableState,
    // blocks are ByteBlock structs -- ranges with different colors
    pub blocks: Vec<ColoredBlock>,
    pub bookmarks: Vec<usize>,
    #[serde(skip)]
    pub changed_bytes: HashMap<usize, String>,
    #[serde(skip)]
    pub changed_history: Vec<usize>,
    #[serde(skip)]
    pub redo_history: Vec<(usize, String)>,

    #[serde(skip)]
    pub comment_input: Input, // the input comment widget (tui-input)
    /// Character a Shift-selection in the comment box started from, or `None`.
    #[serde(skip)]
    pub comment_anchor: Option<usize>,

    // `comment_name_list` is used to show comments in Names list
    // and also on the conversion from selected item on the list
    // to file offset passed to goto()
    pub comment_name_list: Vec<Comment>,

    // `comments` store the comments internally as it is much easier
    // to handle that with a hash map
    pub comments: HashMap<usize, String>,

    #[serde(skip)]
    pub cursor: Point,
    #[serde(skip)]
    pub edit_dialog: crate::hex::edit_dialog::EditDialog,
    #[serde(skip)]
    pub modify_dialog: crate::hex::modify_dialog::ModifyDialog,
    #[serde(skip)]
    pub replace_dialog: crate::hex::replace_dialog::ReplaceDialog,
    #[serde(skip)]
    pub find_dialog: crate::hex::find_dialog::FindDialog,
    /// Offset a Shift+arrow selection was started from.
    ///
    /// Kept separate from `Selection::direction` (which the 'v' selection mode
    /// uses) because Shift-selection is anchor-based: the range is always
    /// between this offset and the cursor, whichever side the cursor ends up
    /// on. `None` means no Shift-selection is in progress.
    #[serde(skip)]
    pub shift_anchor: Option<usize>,
    #[serde(skip)]
    pub editing_hex: bool,
    /// Offset whose high nibble has just been typed, waiting for the low one.
    ///
    /// The half-typed state used to live in `changed_bytes` as a one-character
    /// string, but every consumer parses that map with `from_str_radix`, so a
    /// single keystroke was already a committed `0x0N`: the view showed it, and
    /// `:w` wrote it. Tracking the phase separately keeps `changed_bytes` holding
    /// only whole bytes.
    #[serde(skip)]
    pub nibble_pending: Option<usize>,
    #[serde(skip)]
    pub highlights: HashSet<u8>, // byte highlight
    #[serde(skip)]
    pub last_visited_offset: usize,
    #[serde(skip)]
    pub jump_history_back: Vec<(usize, crate::editor::AppView)>,
    #[serde(skip)]
    pub jump_history_forward: Vec<(usize, crate::editor::AppView)>,
    #[serde(skip)]
    pub names_list_state: ListState,
    #[serde(skip)]
    pub names_regex_input: Input,
    #[serde(skip)]
    pub names_regex: String,
    #[serde(skip)]
    pub offset_state: TableState,
    #[serde(skip)]
    pub editing_target: crate::editor::EditingTarget,
    /// Which column the active selection was made in.
    ///
    /// Decides what yanking produces: raw hex from the byte column, decoded text
    /// from either encoding column. Recorded when the selection starts rather than
    /// read at copy time, so switching columns afterwards does not change the
    /// meaning of a block that is already highlighted.
    #[serde(skip)]
    pub selection_target: crate::editor::EditingTarget,
    #[serde(skip)]
    pub enc2_table: Option<&'static encoding_rs::Encoding>,
    #[serde(skip)]
    pub last_ascii_width: u16,
    #[serde(skip)]
    pub last_enc2_width: u16,
    #[serde(skip)]
    pub offset: usize,
    #[serde(skip)]
    pub search: crate::hex::search::Search,
    #[serde(skip)]
    pub selection: crate::hex::selection::Selection,
    #[serde(skip)]
    pub strings_regex_input: Input,
    /// Character the strings filter's Shift-selection started from, or `None`.
    #[serde(skip)]
    pub strings_filter_anchor: Option<usize>,
    /// Character the Names filter's Shift-selection started from, or `None`.
    #[serde(skip)]
    pub names_filter_anchor: Option<usize>,
    /// Whether keystrokes go to the strings dialog's regex box or to its list.
    #[serde(skip)]
    pub strings_focus_filter: bool,
    /// Encoding the strings scan decodes byte runs as.
    #[serde(skip)]
    pub strings_encoding: crate::hex::strings::StringEncoding,
    /// The in-place string replacement box, when it is open.
    #[serde(skip)]
    pub string_edit: crate::hex::strings::StringEdit,
    /// Rows the strings list had room for on the last frame, so the paging keys
    /// move by a screenful of whatever size the dialog actually is.
    #[serde(skip)]
    pub strings_page_rows: usize,
    /// Indices into `App::strings` that pass the regex box, i.e. the rows the
    /// list actually draws. The selected row is an index into *this*, so Enter
    /// has to map through it.
    #[serde(skip)]
    pub strings_filtered: Vec<usize>,
    #[serde(skip)]
    #[allow(dead_code)]
    pub table_state: TableState,
    #[serde(skip)]
    pub show_va: bool,
}

impl HexView {
    /// Drops everything that belongs to the file being closed, keeping the
    /// settings that belong to the session.
    ///
    /// `load_file` used to clear only `strings` and the staged extension, which
    /// left three kinds of state pointing at the previous file:
    ///
    /// * `changed_bytes` / `changed_history` - unsaved edits made to file A were
    ///   still pending after opening file B, so `:w` wrote A's bytes into B at
    ///   the same offsets, and the hex view marked those offsets as modified.
    /// * `blocks` / `bookmarks` / `comments` / `comment_name_list` - `load_database`
    ///   returns early when the new file has no `.dz6` sidecar, so A's
    ///   annotations stayed on screen and were then saved into `B.dz6`.
    /// * `selection`, `search` results and the jump history - offsets from A
    ///   interpreted against B.
    ///
    /// Deliberately preserved, because they are user settings rather than file
    /// data: `enc2_table` (set by `:set enc2`, restored from `.dz6init` at
    /// startup - wiping it here is the bug that made that setting look like it
    /// never loaded), `show_va`, `highlights` (keyed by byte value, not offset),
    /// `editing_target`, and the dialog input buffers.
    pub fn reset_for_new_file(&mut self) {
        // Pending edits.
        self.changed_bytes.clear();
        self.changed_history.clear();
        self.redo_history.clear();

        // Annotations, i.e. everything the `.dz6` sidecar owns.
        self.blocks.clear();
        self.bookmarks.clear();
        self.comments.clear();
        self.comment_name_list.clear();

        // Offsets into the old file.
        self.selection = Default::default();
        self.shift_anchor = None;
        self.nibble_pending = None;
        self.search = Default::default();
        self.jump_history_back.clear();
        self.jump_history_forward.clear();
        self.last_visited_offset = 0;
        self.cursor = Point::default();
        // `load_file` clears `App::strings`, so these indices would point into a
        // list that no longer exists.
        self.strings_filtered.clear();
    }

    /// Secondary encoding, or UTF-8 when none is configured.
    ///
    /// The fallback used to be EUC-KR, which quietly assumed a Korean user:
    /// anyone else who reached a code path needing this before setting `enc2`
    /// got mojibake. UTF-8 is the locale-neutral choice, and callers that care
    /// whether a secondary encoding is actually enabled check `enc2_table`
    /// directly (see `ruler.rs`).
    pub fn get_enc2_table(&self) -> &'static encoding_rs::Encoding {
        self.enc2_table.unwrap_or(encoding_rs::UTF_8)
    }
}
