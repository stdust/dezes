use crate::app::App;
use crate::editor::AppView;

impl App {
    /// The goto() function handles moving page position instantly
    pub fn goto(&mut self, offset: usize) {
        self.goto_with_history(offset, true);
    }

    pub fn goto_with_history(&mut self, offset: usize, record_history: bool) {
        if offset >= self.file_info.size {
            return;
        }

        let cur_ofs = self.hex_view.offset;
        let cur_view = self.editor_view;

        if record_history && (cur_ofs != offset || cur_view != self.editor_view) {
            if self.hex_view.jump_history_back.last() != Some(&(cur_ofs, cur_view)) {
                self.hex_view.jump_history_back.push((cur_ofs, cur_view));
                if self.hex_view.jump_history_back.len() > 100 {
                    self.hex_view.jump_history_back.remove(0);
                }
            }
            self.hex_view.jump_history_forward.clear();
        }

        if self.editor_view == AppView::Disasm {
            // In Disasm view, only update offset.
            // Page scrolling is handled by draw_disasm_view() which understands
            // variable-length instruction boundaries. Do NOT touch page_start/page_end here.
            self.hex_view.last_visited_offset = self.hex_view.offset;
            self.hex_view.offset = offset;
            return;
        }

        let bytes_per_line = self.config.hex_mode_bytes_per_line.max(1);
        let page_size = self.reader.page_current_size.max(bytes_per_line);
        let lines_per_page = page_size / bytes_per_line;

        // Derived from `page_start`, never read from `reader.page_end`.
        //
        // Only this function maintains `page_end`, and only on this path: the Disasm
        // view moves `page_start` on its own (instruction boundaries) and returns
        // above without touching `page_end`. So on the first hex `goto` after leaving
        // Disasm, the stored `page_end` belonged to some earlier `page_start` and
        // could be far past the real bottom of the page - which made the test below
        // decide the target was already on screen and leave the page where it was.
        //
        // That is the Ctrl+Enter bug: following a data reference out of the Disasm
        // view switched to Hex with the right cursor offset but the old page, so the
        // screen showed where the cursor had *been*, and the next arrow key - the
        // next `goto`, by then with a `page_end` that matched - snapped it into place.
        let page_end = self
            .reader
            .page_start
            .saturating_add(page_size)
            .saturating_sub(1);

        if offset < self.reader.page_start {
            self.reader.page_start = (offset / bytes_per_line) * bytes_per_line;
        } else if offset > page_end {
            let offset_line_start = (offset / bytes_per_line) * bytes_per_line;
            if offset_line_start <= page_end + bytes_per_line * 4 {
                let new_start = offset_line_start.saturating_sub(lines_per_page.saturating_sub(1) * bytes_per_line);
                self.reader.page_start = (new_start / bytes_per_line) * bytes_per_line;
            } else {
                self.reader.page_start = offset_line_start;
            }
        }

        self.reader.page_end = self.reader.page_start.saturating_add(self.reader.page_current_size).saturating_sub(1);

        if offset >= self.reader.page_start {
            let rel = offset - self.reader.page_start;
            self.hex_view.cursor.y = rel / bytes_per_line;
            self.hex_view.cursor.x = rel % bytes_per_line;
        }

        self.hex_view.last_visited_offset = self.hex_view.offset;
        self.hex_view.offset = offset;

        if self.editor_view == AppView::Header {
            self.editor_view = AppView::Hex;
        }
    }

    pub fn jump_back(&mut self) {
        if let Some((prev_ofs, prev_view)) = self.hex_view.jump_history_back.pop() {
            let cur_ofs = self.hex_view.offset;
            let cur_view = self.editor_view;
            self.hex_view.jump_history_forward.push((cur_ofs, cur_view));

            self.editor_view = prev_view;
            self.goto_with_history(prev_ofs, false);
            // The jump can cross views, and `page_start` means different things in
            // each - see `App::align_page_for_view`.
            self.align_page_for_view();
            App::log(self, format!("Jumped back (Ctrl+Left) to {:?} 0x{:X}", prev_view, prev_ofs));
        } else {
            crate::beep!();
        }
    }

    pub fn jump_forward(&mut self) {
        if let Some((next_ofs, next_view)) = self.hex_view.jump_history_forward.pop() {
            let cur_ofs = self.hex_view.offset;
            let cur_view = self.editor_view;
            self.hex_view.jump_history_back.push((cur_ofs, cur_view));

            self.editor_view = next_view;
            self.goto_with_history(next_ofs, false);
            self.align_page_for_view();
            App::log(self, format!("Jumped forward (Ctrl+Right) to {:?} 0x{:X}", next_view, next_ofs));
        } else {
            crate::beep!();
        }
    }
}

#[cfg(test)]
mod cross_view_goto_tests {
    use crate::app::App;
    use crate::editor::AppView;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_with_file() -> App {
        let dir = std::env::temp_dir().join("dezes_cross_goto");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("g_{}_{}.bin", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, vec![0x90u8; 0x8000]).expect("write fixture");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.config.hex_mode_bytes_per_line = 16;
        app.reader.page_current_size = 16 * 20; // twenty rows
        app
    }

    /// A jump out of the Disasm view has to bring the page with it.
    ///
    /// Ctrl+Enter on `lea rdx,[...]` switched to the Hex view with the cursor on the
    /// target but the page still showing where the cursor had been; one arrow key
    /// then snapped it into place. The cause was `reader.page_end`: only the Hex path
    /// of `goto` maintains it, the Disasm view moves `page_start` without it, so the
    /// stale value made "is the target already on screen?" answer yes when it was
    /// nowhere near.
    #[test]
    fn following_a_reference_out_of_disasm_moves_the_page() {
        let mut app = app_with_file();

        // Where the user was: browsing code near the top of the file.
        app.editor_view = AppView::Disasm;
        app.hex_view.offset = 0x100;
        app.reader.page_start = 0x100;
        // A `page_end` left over from an earlier position deep in the file, which is
        // exactly what the Disasm view leaves behind.
        app.reader.page_end = 0x7FFF;

        // Ctrl+Enter: switch view, go to the target, align the page.
        let target = 0x4000usize;
        app.editor_view = AppView::Hex;
        app.goto_with_history(target, false);
        app.align_page_for_view();

        let page_size = app.reader.page_current_size;
        assert_eq!(app.hex_view.offset, target, "the cursor must be on the target");
        assert!(
            app.reader.page_start <= target && target < app.reader.page_start + page_size,
            "the target 0x{:X} is off screen: page 0x{:X}..0x{:X}",
            target,
            app.reader.page_start,
            app.reader.page_start + page_size
        );
        assert_eq!(app.reader.page_start % 16, 0, "the hex grid must stay aligned");
        // And the highlight has to be on a row that exists.
        assert!(
            app.hex_view.cursor.y < page_size / 16,
            "cursor row {} is past the bottom of the page",
            app.hex_view.cursor.y
        );
    }

    /// The same for a jump backwards through the file.
    #[test]
    fn a_backwards_jump_out_of_disasm_moves_the_page() {
        let mut app = app_with_file();
        app.editor_view = AppView::Disasm;
        app.hex_view.offset = 0x6000;
        app.reader.page_start = 0x6000;
        app.reader.page_end = 0x7FFF;

        app.editor_view = AppView::Hex;
        app.goto_with_history(0x200, false);
        app.align_page_for_view();

        let page_size = app.reader.page_current_size;
        assert!(
            app.reader.page_start <= 0x200 && 0x200 < app.reader.page_start + page_size,
            "page 0x{:X} does not contain 0x200",
            app.reader.page_start
        );
    }

    /// A short hop inside the current page must not scroll: that is what keeps
    /// arrow-key movement from jumping the screen around.
    #[test]
    fn a_move_inside_the_page_leaves_it_alone() {
        let mut app = app_with_file();
        app.editor_view = AppView::Hex;
        app.reader.page_start = 0x1000;
        app.reader.page_end = 0x1000 + app.reader.page_current_size - 1;
        app.hex_view.offset = 0x1010;

        app.goto_with_history(0x1020, false);

        assert_eq!(app.reader.page_start, 0x1000, "the page scrolled for a move inside it");
    }
}
#[cfg(test)]
mod addr_mode_tests {
    use crate::app::App;

    fn scratch(name: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("dz6_addr_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        std::fs::write(&path, vec![0x90u8; 0x400]).expect("write");
        let s = path.to_str().expect("utf-8").to_string();
        (dir, s)
    }

    /// `set addr va` belongs in `.dzsrc`, which is replayed *before* the file is
    /// opened - so the setting has to survive `load_file`. It does, because
    /// `reset_for_new_file` treats the address mode as a session setting rather than
    /// as file data; this test is what keeps that true.
    #[test]
    fn addr_va_survives_opening_a_file() {
        let (dir, path) = scratch("initfile_order");
        let mut app = App::new();
        app.config.database = false;
        assert!(!app.hex_view.show_va, "file offsets are the default");

        // What the init file does.
        crate::commands::parse_command(&mut app, "set addr va");
        assert!(app.hex_view.show_va);

        // What main() does next.
        app.load_file(&path, 0, true).expect("open");

        let show_va = app.hex_view.show_va;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(show_va, "'set addr va' was lost when the file was opened");
    }

    /// The other two forms, so the option is not just a one-way switch.
    #[test]
    fn addr_takes_offset_and_toggle_too() {
        let (dir, path) = scratch("forms");
        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, true).expect("open");

        crate::commands::parse_command(&mut app, "set addr va");
        assert!(app.hex_view.show_va);
        crate::commands::parse_command(&mut app, "set addr offset");
        assert!(!app.hex_view.show_va);
        crate::commands::parse_command(&mut app, "set addr toggle");
        assert!(app.hex_view.show_va);
        crate::commands::parse_command(&mut app, "set addr");
        assert!(!app.hex_view.show_va, "a bare 'set addr' toggles");

        let _ = std::fs::remove_dir_all(&dir);
    }
}