use crate::app::App;
use crate::editor::AppView;

impl App {
    pub fn switch_editor_view(&mut self) {
        if self.is_executable() {
            match self.editor_view {
                AppView::Hex => self.editor_view = AppView::Disasm,
                AppView::Disasm => self.editor_view = AppView::Hex,
                _ => self.editor_view = AppView::Hex,
            }
        } else {
            self.editor_view = AppView::Hex;
        }
        self.prev_editor_view = self.editor_view;
        self.last_primary_view = self.editor_view;
        self.align_page_for_view();
    }

    /// Re-anchors `reader.page_start` for the view that is now on screen.
    ///
    /// The two views want different things from the same field and neither used to
    /// fix it up when the other had last written it:
    ///
    /// * The Disasm view decodes forward from `page_start`. Arriving from the Hex
    ///   view it inherited a multiple of the bytes-per-line setting, which is
    ///   almost never an instruction boundary, so the top rows of the listing were
    ///   decoded out of phase - x86 re-synchronises after a few instructions, which
    ///   is why it looked like a couple of odd lines rather than an obvious bug.
    /// * The Hex view lays out rows from `page_start`. Arriving from the Disasm view
    ///   it inherited an instruction boundary, so the grid started at an address
    ///   like `40E3` and every row was off the 16-byte alignment that makes a hex
    ///   dump readable.
    ///
    /// Both are anchored here instead of moving the cursor, so the view keeps
    /// roughly the same scroll position across a switch.
    /// Leaves the Text or Header view for the Hex or Disasm view it was reached
    /// from.
    ///
    /// F4 and F7 used to be one-way: pressing F4 in the Header view, or F7 in the
    /// Text view, did nothing at all, so the only way back was Esc. They toggle
    /// now, and both land on a primary view rather than bouncing between the two
    /// secondary ones.
    pub fn return_to_primary_view(&mut self) {
        let target = if matches!(self.prev_editor_view, AppView::Text | AppView::Header) {
            self.last_primary_view
        } else {
            self.prev_editor_view
        };
        self.editor_view = target;
        self.last_primary_view = target;
        self.prev_editor_view = target;
        // `page_start` means different things in each view - see below.
        self.align_page_for_view();
    }

    pub fn align_page_for_view(&mut self) {
        match self.editor_view {
            AppView::Disasm => {
                let page_start = self.reader.page_start;
                // Re-sync at the current page top rather than jumping the page to
                // the cursor: the boundary this finds is at or just before where the
                // user was already looking.
                self.reader.page_start = crate::disasm::nav::containing_instruction(self, page_start);
            }
            AppView::Hex => {
                let bpl = self.config.hex_mode_bytes_per_line.max(1);
                let aligned = (self.reader.page_start / bpl) * bpl;
                self.reader.page_start = aligned;
                self.reader.page_end = aligned
                    .saturating_add(self.reader.page_current_size)
                    .saturating_sub(1);

                // Snapping the page moves every row, so the cursor's screen
                // position has to be recomputed or the highlight lands on the wrong
                // byte until the next keypress.
                let offset = self.hex_view.offset;
                if offset >= aligned {
                    let rel = offset - aligned;
                    self.hex_view.cursor.y = rel / bpl;
                    self.hex_view.cursor.x = rel % bpl;
                }
            }
            AppView::Text | AppView::Header => {}
        }
    }
}

#[cfg(test)]
mod view_switch_tests {
    use super::*;
    use crate::disasm::nav;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Five-byte instructions from offset 0x100, so a page start that is a multiple
    /// of 16 is provably *not* a boundary.
    fn app_with_code() -> App {
        let dir = std::env::temp_dir().join("dz6_view_switch");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("v_{}.bin", SEQ.fetch_add(1, Ordering::Relaxed)));

        let mut bytes = vec![0x90u8; 0x400];
        // `mov eax, imm32` (5 bytes) repeated across 0x100..0x200.
        let mut ofs = 0x100;
        while ofs + 5 <= 0x200 {
            bytes[ofs] = 0xB8;
            bytes[ofs + 1] = 0x78;
            bytes[ofs + 2] = 0x56;
            bytes[ofs + 3] = 0x34;
            bytes[ofs + 4] = 0x12;
            ofs += 5;
        }
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.config.hex_mode_bytes_per_line = 16;
        app.reader.page_current_size = 0x100;
        app
    }

    /// Entering the Disasm view must start the page on an instruction boundary.
    #[test]
    fn entering_disasm_snaps_to_an_instruction_boundary() {
        let mut app = app_with_code();
        app.editor_view = AppView::Disasm;
        // 0x110 is a multiple of 16 and, in this fixture, mid-instruction
        // (boundaries run 0x100, 0x105, 0x10A, 0x10F, 0x114...).
        app.reader.page_start = 0x110;
        app.hex_view.offset = 0x118;

        app.align_page_for_view();

        let start = app.reader.page_start;
        assert_eq!(
            nav::containing_instruction(&app, start),
            start,
            "0x{:X} is not an instruction boundary",
            start
        );
        assert!(start <= 0x110, "the page must not jump forward past what was shown");
        assert!(0x110 - start < 16, "and it must stay near where the user was looking");
    }

    /// Entering the Hex view must put the grid back on a bytes-per-line boundary.
    #[test]
    fn entering_hex_snaps_to_the_byte_grid() {
        let mut app = app_with_code();
        app.editor_view = AppView::Hex;
        app.reader.page_start = 0x10F; // an instruction boundary, not a row boundary
        app.hex_view.offset = 0x114;

        app.align_page_for_view();

        assert_eq!(app.reader.page_start % 16, 0);
        assert_eq!(app.reader.page_start, 0x100);
        // The cursor's screen position follows the page it is drawn on.
        assert_eq!(app.hex_view.cursor.y, 1, "0x114 is on the second row of 0x100");
        assert_eq!(app.hex_view.cursor.x, 4);
    }

    /// Tab does the alignment, and a round trip leaves the cursor where it was.
    #[test]
    fn tab_aligns_and_keeps_the_cursor() {
        let mut app = app_with_code();
        app.header_view.pe = None;
        app.editor_view = AppView::Hex;
        app.reader.page_start = 0x110;
        app.hex_view.offset = 0x118;

        // A non-executable file always lands on Hex, so drive the helper directly
        // for the Disasm leg; `switch_editor_view` is what the test is about for Hex.
        app.editor_view = AppView::Disasm;
        app.align_page_for_view();
        let disasm_start = app.reader.page_start;
        assert_eq!(nav::containing_instruction(&app, disasm_start), disasm_start);

        app.switch_editor_view();
        assert!(app.editor_view == AppView::Hex);
        assert_eq!(app.reader.page_start % 16, 0, "back in hex, back on the grid");
        assert_eq!(app.hex_view.offset, 0x118, "the cursor itself never moves");
    }
}
