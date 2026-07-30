use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::App;

impl App {
    /// Where this file's annotations are stored: `<file>.dzdb`, in the same
    /// directory as the file itself.
    ///
    /// `None` when no file is open, which is the case at startup and while the
    /// file dialog is up.
    pub fn database_path(&self) -> Option<PathBuf> {
        if self.file_info.path.is_empty() || self.file_info.name.is_empty() {
            return None;
        }
        let dir = Path::new(&self.file_info.path)
            .parent()
            .unwrap_or(Path::new("."));
        Some(dir.join(format!("{}.{}", self.file_info.name, crate::app::DB_EXT)))
    }

    /// Sidecars written by earlier versions, newest naming first.
    ///
    /// Two things changed over time and both are still read so that annotations
    /// made with an older build are not lost:
    ///
    /// * the extension, which was `.dz6` before the program was renamed;
    /// * the location, which used to be the startup directory rather than beside
    ///   the file - that scattered sidecars into unrelated folders, so nothing is
    ///   written there any more.
    fn legacy_database_paths(&self) -> Vec<PathBuf> {
        if self.file_info.name.is_empty() {
            return Vec::new();
        }
        let name = &self.file_info.name;
        let legacy_ext = crate::app::LEGACY_DB_EXT;
        let startup = crate::util::startup_dir();

        let mut paths = Vec::with_capacity(3);
        // Beside the file, old extension.
        if !self.file_info.path.is_empty() {
            let dir = Path::new(&self.file_info.path)
                .parent()
                .unwrap_or(Path::new("."));
            paths.push(dir.join(format!("{}.{}", name, legacy_ext)));
        }
        // Startup directory, either extension.
        paths.push(startup.join(format!("{}.{}", name, crate::app::DB_EXT)));
        paths.push(startup.join(format!("{}.{}", name, legacy_ext)));
        paths
    }

    /// True when there is nothing worth writing a sidecar for.
    fn annotations_are_empty(&self) -> bool {
        self.hex_view.bookmarks.is_empty()
            && self.hex_view.comment_name_list.is_empty()
            && self.hex_view.comments.is_empty()
            && self.hex_view.blocks.is_empty()
    }

    pub fn save_database(&self) -> Result<(), Box<dyn Error>> {
        let Some(target_db) = self.database_path() else {
            return Ok(());
        };

        // Nothing to keep: drop any sidecar rather than leaving a stale one, in
        // both the current and the legacy location.
        if self.annotations_are_empty() {
            let _ = fs::remove_file(&target_db);
            for legacy in self.legacy_database_paths() {
                let _ = fs::remove_file(legacy);
            }
            return Ok(());
        }

        let toml_string = toml::to_string_pretty(&self.hex_view)?;

        // Written beside the file, with no fallback. The old code fell back to
        // the startup directory when that failed, which silently scattered
        // sidecars into unrelated folders and made "where did my comments go?"
        // unanswerable. Failing loudly instead lets the caller report it.
        fs::write(&target_db, &toml_string)?;

        Ok(())
    }

    /// Writes the annotations out, logging the outcome. Returns a message when
    /// the write failed, so an exit path can also report it on the terminal.
    ///
    /// Saving used to be attached to *file writes* only (`:w`, `:wq`, F12), so
    /// quitting with `:q` - or opening another file - discarded every comment
    /// made in the session, and on a read-only file the write failed first and
    /// the annotations were never reached at all. Annotations are a separate
    /// sidecar; they have no reason to depend on patching the binary.
    pub fn persist_annotations(&mut self) -> Option<String> {
        if !self.config.database || self.database_path().is_none() {
            return None;
        }

        match self.save_database() {
            Ok(()) => {
                if !self.annotations_are_empty()
                    && let Some(path) = self.database_path()
                {
                    App::log(self, format!("Saved annotations to {}", path.display()));
                }
                None
            }
            Err(e) => {
                let message = match self.database_path() {
                    Some(path) => format!("could not save annotations to {}: {}", path.display(), e),
                    None => format!("could not save annotations: {}", e),
                };
                App::log(self, message.clone());
                Some(message)
            }
        }
    }
    pub fn load_database(&mut self) -> Result<(), Box<dyn Error>> {
        let target_db = self
            .database_path()
            .ok_or_else(|| Box::<dyn Error>::from("no file open"))?;

        // Beside the file first, which is the only place `save_database` writes.
        // The startup directory is read purely for sidecars left there by older
        // builds; load used to *prefer* it, so with a copy in both places every
        // reload restored the stale one.
        let data = match fs::read_to_string(&target_db) {
            Ok(data) => data,
            Err(e) => {
                // Older names and locations, in order. Load used to *prefer* the
                // startup directory, so with a copy in both places every reload
                // restored the stale one.
                let mut found = None;
                for legacy in self.legacy_database_paths() {
                    if let Ok(data) = fs::read_to_string(&legacy) {
                        found = Some(data);
                        break;
                    }
                }
                match found {
                    Some(data) => data,
                    None => return Err(Box::new(e)),
                }
            }
        };

        // Only the four persisted fields are copied across, instead of
        // replacing the whole `HexView`.
        //
        // The old `self.hex_view = toml::from_str(...)` also reset every
        // `#[serde(skip)]` field to its default, which is how `:set enc2` from
        // the init file was silently discarded: it is read at startup, then
        // opening a file with a sidecar wiped `enc2_table` back to
        // `None`. Highlights, the VA/offset display mode and the search state
        // were being cleared the same way.
        //
        // This also answers the old TODO here ("other fields might be loaded if
        // they are defined in the TOML file") - a hand-edited or stale sidecar
        // can no longer inject anything beyond these four.
        let loaded: crate::hex::hex_view::HexView = toml::from_str(&data)?;
        self.hex_view.blocks = loaded.blocks;
        self.hex_view.bookmarks = loaded.bookmarks;
        self.hex_view.comment_name_list = loaded.comment_name_list;
        self.hex_view.comments = loaded.comments;

        self.hex_view.editing_hex = true; // otherwise it defaults to false if a sidecar exists for the target
        self.sanitize_database();
        Ok(())
    }

    /// Drops or clamps anything in a loaded `.dz6` that points outside the file.
    ///
    /// The database is an on-disk TOML that gets deserialized straight into
    /// `HexView`, so a stale or hand-edited one could hand block/bookmark ranges
    /// past EOF to the draw loops and the selection logic.
    fn sanitize_database(&mut self) {
        let size = self.file_info.size;
        if size == 0 {
            self.hex_view.blocks.clear();
            self.hex_view.bookmarks.clear();
            self.hex_view.comment_name_list.clear();
            self.hex_view.comments.clear();
            return;
        }

        let last = size - 1;

        self.hex_view.blocks.retain(|b| b.start <= last);
        for b in &mut self.hex_view.blocks {
            b.end = b.end.min(last).max(b.start);
        }
        // `[` and `]` navigation relies on this ordering.
        self.hex_view.blocks.sort_by_key(|b| b.start);

        self.hex_view.bookmarks.retain(|ofs| *ofs <= last);
        self.hex_view.comment_name_list.retain(|c| c.offset <= last);
        self.hex_view.comments.retain(|ofs, _| *ofs <= last);
    }
}

#[cfg(test)]
mod load_file_reset_tests {
    use crate::app::App;
    use crate::hex::blocks::ColoredBlock;

    /// Two distinct real files to switch between. The test binary is a PE on
    /// Windows; `COPYING` is plain text, so the pair also covers the
    /// executable -> non-executable direction.
    fn two_files() -> Option<(String, String)> {
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        let txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("COPYING");
        if !txt.is_file() {
            return None;
        }
        Some((exe, txt.to_str()?.to_string()))
    }

    /// Unsaved edits must not survive into the next file.
    ///
    /// They used to: `load_file` never cleared `changed_bytes`, so editing file A
    /// and then opening file B left A's offset->byte map pending, and `:w` wrote
    /// A's bytes into B at those offsets.
    #[test]
    fn pending_edits_do_not_follow_the_next_file() {
        let Some((a, b)) = two_files() else { return };
        let mut app = App::new();

        app.load_file(&a, 0, true).expect("open A");
        app.hex_view.changed_bytes.insert(0x40, "90".to_string());
        app.hex_view.changed_history.push(0x40);
        app.hex_view.redo_history.push((0x41, "00".to_string()));

        app.load_file(&b, 0, true).expect("open B");
        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "edits from the previous file are still pending, ':w' would write them into this one"
        );
        assert!(app.hex_view.changed_history.is_empty());
        assert!(app.hex_view.redo_history.is_empty());
    }

    /// Annotations must not leak either, since `load_database` returns early when
    /// the new file has no sidecar and so cannot overwrite them.
    #[test]
    fn annotations_do_not_follow_the_next_file() {
        let Some((a, b)) = two_files() else { return };
        let mut app = App::new();
        app.config.database = false; // isolate from any real .dz6 on disk

        app.load_file(&a, 0, true).expect("open A");
        app.hex_view.bookmarks.push(0x100);
        app.hex_view.comments.insert(0x100, "note".to_string());
        app.hex_view.blocks.push(ColoredBlock::default());

        app.load_file(&b, 0, true).expect("open B");
        assert!(
            app.hex_view.bookmarks.is_empty() && app.hex_view.comments.is_empty(),
            "previous file's annotations are still shown and would be saved into this file's .dz6"
        );
        assert!(app.hex_view.blocks.is_empty());
    }

    /// Opening a non-executable after an executable must drop the parsed image.
    #[test]
    fn parsed_image_is_dropped_when_the_new_file_is_not_executable() {
        let Some((exe, txt)) = two_files() else { return };
        let mut app = App::new();

        app.load_file(&exe, 0, true).expect("open exe");
        assert!(app.is_executable(), "precondition: the test binary parses as PE");

        app.load_file(&txt, 0, true).expect("open text");
        assert!(
            !app.is_executable(),
            "the previous file's section table is still in place, so VA translation \
             and the Disasm view would use the wrong layout"
        );
        assert!(app.header_view.pe.is_none());
        assert!(app.header_view.elf.is_none());
    }

    /// Settings are session-level and must survive a file change - clearing
    /// `enc2_table` here is what previously made `:set enc2` from `.dz6init` look
    /// like it had never loaded.
    #[test]
    fn session_settings_survive_a_file_change() {
        let Some((a, b)) = two_files() else { return };
        let mut app = App::new();

        app.load_file(&a, 0, true).expect("open A");
        app.hex_view.enc2_table = Some(encoding_rs::EUC_KR);
        app.hex_view.show_va = true;
        app.hex_view.highlights.insert(0x90);

        app.load_file(&b, 0, true).expect("open B");
        assert_eq!(
            app.hex_view.enc2_table.map(|e| e.name()),
            Some("EUC-KR"),
            "the secondary encoding is a session setting, not file data"
        );
        assert!(app.hex_view.show_va);
        assert!(app.hex_view.highlights.contains(&0x90));
    }

    /// Stale offsets from the previous file must not linger.
    #[test]
    fn offsets_from_the_previous_file_are_dropped() {
        let Some((a, b)) = two_files() else { return };
        let mut app = App::new();

        app.load_file(&a, 0, true).expect("open A");
        app.hex_view.selection.start = 0x1000;
        app.hex_view.selection.end = 0x1100;
        app.hex_view.shift_anchor = Some(0x1000);
        app.hex_view.search.matches = vec![0x1000, 0x2000];
        app.hex_view.search.match_index = Some(1);
        app.hex_view.jump_history_back.push((0x1000, crate::editor::AppView::Hex));

        app.load_file(&b, 0, true).expect("open B");
        assert_eq!(app.hex_view.selection.start, 0);
        assert_eq!(app.hex_view.selection.end, 0);
        assert!(app.hex_view.shift_anchor.is_none());
        assert!(app.hex_view.search.matches.is_empty());
        assert!(app.hex_view.search.match_index.is_none());
        assert!(app.hex_view.jump_history_back.is_empty());
    }
}

#[cfg(test)]
mod sidecar_precedence_tests {
    use crate::app::App;

    /// `save_database` only ever writes beside the file; `load_database` has to
    /// prefer that copy over one left in the startup directory by an older build.
    ///
    /// It used to read the startup directory first, so with a `<name>.dz6` in
    /// both places every reload restored the stale copy and the annotations just
    /// written next to the file were ignored.
    #[test]
    fn load_prefers_the_sidecar_next_to_the_file() {
        // A scratch file in its own directory, so the sidecar beside it can be
        // distinguished from one in the startup directory.
        let dir = std::env::temp_dir()
            .join(format!("dz6_sidecar_precedence_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // The startup-directory copy below is shared by every process, so the file
        // name carries the pid: two test binaries running at once would otherwise
        // write the same `sample.bin.dzdb` there and read each other's.
        let name = format!("sample_{}.bin", std::process::id());
        let target = dir.join(&name);
        std::fs::write(&target, vec![0u8; 256]).expect("write sample");

        let beside_file = dir.join(format!("{}.dzdb", name));
        let beside_startup = crate::util::startup_dir().join(format!("{}.dzdb", name));

        // Distinguishable contents: the bookmark offset identifies which file
        // won. Both must be inside the 256-byte sample, or `sanitize_database`
        // discards them as past EOF and the test can't tell the two apart.
        //
        // All four persisted fields have to be present: `HexView`'s derived
        // `Deserialize` has no `#[serde(default)]`, so a partial sidecar fails to
        // parse and `load_database` returns `Err` - which would make this test
        // pass for the wrong reason.
        let sidecar = |bookmark: usize| {
            format!("blocks = []\nbookmarks = [{}]\ncomment_name_list = []\ncomments = {{}}\n", bookmark)
        };
        std::fs::write(&beside_file, sidecar(16)).expect("write near sidecar");
        std::fs::write(&beside_startup, sidecar(32)).expect("write far sidecar");

        let mut app = App::new();
        app.config.database = true;
        let loaded = app.load_file(target.to_str().expect("path"), 0, true);

        let bookmarks = app.hex_view.bookmarks.clone();

        let _ = std::fs::remove_file(&beside_file);
        let _ = std::fs::remove_file(&beside_startup);
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);

        loaded.expect("open sample");
        assert_eq!(
            bookmarks,
            vec![16],
            "the sidecar beside the file must win over the one in the startup directory"
        );
    }
}

#[cfg(test)]
mod save_path_tests {
    use crate::app::App;

    struct Scratch {
        dir: std::path::PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            // Pid-scoped: see the note in `annotation_persistence_tests::Scratch`.
            let dir = std::env::temp_dir().join(format!("dz6_save_{}_{}", name, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self { dir }
        }
        fn file(&self, name: &str, bytes: &[u8]) -> String {
            let p = self.dir.join(name);
            std::fs::write(&p, bytes).expect("write scratch file");
            p.to_str().expect("utf-8 path").to_string()
        }
        fn path(&self, name: &str) -> std::path::PathBuf {
            self.dir.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A staged extension must be appended exactly once, however many times the
    /// file is saved, and must not still be staged afterwards.
    ///
    /// Scope note: this does **not** reproduce the original double-append, which
    /// needed `write_to_file` to fail *after* the append (the reload at the end
    /// of a successful save cleared the field as a side effect). Both the append
    /// and the subsequent write open the same path with write access, so a test
    /// cannot make one succeed and the other fail without a seam in the code.
    /// What is checked here is the invariant the fix establishes - staged bytes
    /// are marked written as soon as they are on disk - plus the end-to-end
    /// behaviour that must not regress.
    #[test]
    fn staged_extension_is_appended_only_once() {
        let scratch = Scratch::new("staged_once");
        let path = scratch.file("sample.bin", &[0u8; 0x100]);

        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, false).expect("open");
        assert!(!app.file_info.is_read_only, "scratch file must be writable");

        let original_len = app.file_info.buffer_len();
        app.file_info.stage_extension(&vec![0xAAu8; 0x40]);
        assert_eq!(app.file_info.staged_extension.len(), 0x40);

        app.write_to_file().expect("first save");
        let after_first = std::fs::metadata(&path).expect("stat").len() as usize;
        assert_eq!(
            after_first,
            original_len + 0x40,
            "the staged payload should have been appended once"
        );
        assert!(
            app.file_info.staged_extension.is_empty(),
            "the payload is on disk and must no longer be staged"
        );

        app.write_to_file().expect("second save");
        let after_second = std::fs::metadata(&path).expect("stat").len() as usize;
        assert_eq!(
            after_second, after_first,
            "saving again appended the payload a second time"
        );
    }

    /// The staged bytes must stay readable between the append and the reload.
    #[test]
    fn staged_bytes_remain_readable_after_being_written() {
        let scratch = Scratch::new("staged_readable");
        let path = scratch.file("sample.bin", &[0u8; 0x100]);

        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, false).expect("open");
        app.file_info.stage_extension(&vec![0xAAu8; 0x40]);
        app.write_to_file().expect("save");

        assert_eq!(
            app.file_info.buffer_len(),
            0x140,
            "the appended bytes must still be part of the readable buffer"
        );
        let buffer = app.file_info.get_buffer_ref();
        assert_eq!(
            buffer.get(0x100).copied(),
            Some(0xAA),
            "extension byte must be readable straight after the save"
        );
    }

    /// A failed Save As must leave the pending edits alone.
    ///
    /// Scope note: this exercises the early failure (the copy itself fails), not
    /// the later one the fix targets - `fs::write` succeeding and then
    /// `load_file` failing - which a test cannot arrange, since the file has just
    /// been written successfully and so opens and maps fine. The fix is still the
    /// right shape: state is now cleared only by the switch that consumes it,
    /// instead of before an operation that can fail.
    #[test]
    fn save_as_keeps_edits_when_the_switch_fails() {
        let scratch = Scratch::new("save_as_fail");
        let path = scratch.file("sample.bin", &[0u8; 0x100]);

        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, false).expect("open");
        app.hex_view.changed_bytes.insert(0x10, "AB".to_string());
        app.hex_view.changed_history.push(0x10);

        // A directory as the target: `fs::write` fails, so the whole Save As
        // fails before any state is touched.
        let bad_target = scratch.path("subdir");
        std::fs::create_dir_all(&bad_target).expect("dir");

        let res = app.write_to_file_as(&bad_target);
        assert!(res.is_err(), "writing over a directory must fail");
        assert_eq!(
            app.hex_view.changed_bytes.get(&0x10).map(String::as_str),
            Some("AB"),
            "a failed Save As must leave the pending edits in place"
        );
    }

    /// The successful path still switches over and clears the edits.
    #[test]
    fn save_as_switches_to_the_new_file() {
        let scratch = Scratch::new("save_as_ok");
        let path = scratch.file("sample.bin", &[0u8; 0x100]);

        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, false).expect("open");
        app.hex_view.changed_bytes.insert(0x10, "AB".to_string());

        let target = scratch.path("copy.bin");
        app.write_to_file_as(&target).expect("save as");

        assert_eq!(app.file_info.name, "copy.bin", "session must follow the new file");
        assert!(
            app.hex_view.changed_bytes.is_empty(),
            "edits are on disk now and must not stay pending"
        );
        let written = std::fs::read(&target).expect("read copy");
        assert_eq!(written[0x10], 0xAB, "the edit must be present in the new file");
    }
}

#[cfg(test)]
mod annotation_persistence_tests {
    use crate::app::App;
    use crate::commands::Commands;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// A scratch directory of its own, so the sidecar written beside the file can
    /// be told apart from any other copy and cleaned up afterwards.
    struct Scratch {
        dir: std::path::PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            // The pid is part of the name because two test binaries can be running
            // at once (a release build alongside a debug one, say): without it they
            // share `dz6_persist_0` and each one's `remove_dir_all` deletes the
            // other's fixture mid-test.
            let pid = std::process::id();
            let dir = std::env::temp_dir().join(format!("dz6_persist_{pid}_{n}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self { dir }
        }

        /// Fixture files need distinct names: opening one maps it, and Windows
        /// refuses to rewrite a mapped file.
        fn file(&self, name: &str) -> String {
            let p = self.dir.join(name);
            std::fs::write(&p, vec![0u8; 0x200]).expect("write fixture");
            p.to_str().expect("utf-8 path").to_string()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A sidecar written by the old `dz6` build is still read.
    ///
    /// The extension changed with the rename. Annotations are work the user did, so
    /// the old name has to keep loading - it is only the writing that moved on.
    #[test]
    fn a_legacy_sidecar_is_still_loaded() {
        let scratch = Scratch::new();
        let path = scratch.file("legacy.bin");

        // Produce a real sidecar, then rename it to the pre-rename extension.
        let mut first = App::new();
        first.config.database = true;
        first.load_file(&path, 0, true).expect("open");
        Commands::comment(&mut first, 0x40, "from the old build".to_string());
        first.persist_annotations();
        drop(first);

        let current = scratch.dir.join(format!("legacy.bin.{}", crate::app::DB_EXT));
        let legacy = scratch
            .dir
            .join(format!("legacy.bin.{}", crate::app::LEGACY_DB_EXT));
        std::fs::rename(&current, &legacy).expect("rename to the legacy extension");
        assert!(!current.exists(), "only the legacy sidecar may be on disk");

        let mut second = App::new();
        second.config.database = true;
        second.load_file(&path, 0, true).expect("reopen");

        assert_eq!(
            second.hex_view.comments.get(&0x40).map(String::as_str),
            Some("from the old build"),
            "the legacy sidecar was ignored, so the comments are lost"
        );

        // The next save moves it to the new name and drops the old one.
        second.persist_annotations();
        assert!(current.is_file(), "the save must write the new extension");
    }

    /// The sidecar lands next to the file, named `<file>.dzdb`.
    #[test]
    fn the_sidecar_is_written_beside_the_file() {
        let scratch = Scratch::new();
        let path = scratch.file("asdf.exe");

        let mut app = App::new();
        app.config.database = true;
        app.load_file(&path, 0, true).expect("open");

        Commands::comment(&mut app, 0x40, "hello".to_string());
        assert!(app.persist_annotations().is_none(), "save must succeed");

        let expected = scratch.dir.join("asdf.exe.dzdb");
        assert!(
            expected.is_file(),
            "expected the sidecar at {}, directory holds: {:?}",
            expected.display(),
            std::fs::read_dir(&scratch.dir)
                .map(|d| d.filter_map(|e| e.ok().map(|e| e.file_name())).collect::<Vec<_>>())
                .unwrap_or_default()
        );
        assert_eq!(
            app.database_path().as_deref(),
            Some(expected.as_path()),
            "database_path must agree with where the file actually went"
        );
    }

    /// A comment made in one session is there again in the next.
    #[test]
    fn a_comment_survives_reopening_the_file() {
        let scratch = Scratch::new();
        let path = scratch.file("target.bin");

        let mut first = App::new();
        first.config.database = true;
        first.load_file(&path, 0, true).expect("open");
        Commands::comment(&mut first, 0x40, "remember me".to_string());
        first.persist_annotations();
        drop(first);

        let mut second = App::new();
        second.config.database = true;
        second.load_file(&path, 0, true).expect("reopen");

        assert_eq!(
            second.hex_view.comments.get(&0x40).map(String::as_str),
            Some("remember me"),
            "the comment was not restored from the sidecar"
        );
        assert_eq!(
            second.hex_view.comment_name_list.len(),
            1,
            "the Names list entry must come back too"
        );
    }

    /// Read-only files keep their annotations: the sidecar is separate from the
    /// binary, so nothing here depends on being able to patch the file.
    ///
    /// This is what made the old behaviour so lossy - saving hung off a successful
    /// `:w`, which on a read-only file can never happen.
    #[test]
    fn annotations_are_saved_for_a_read_only_file() {
        let scratch = Scratch::new();
        let path = scratch.file("readonly.bin");

        let mut app = App::new();
        app.config.database = true;
        app.load_file(&path, 0, true).expect("open read-only");
        assert!(app.file_info.is_read_only, "opened read-only on purpose");

        Commands::comment(&mut app, 0x10, "note".to_string());
        assert!(app.persist_annotations().is_none());

        assert!(
            scratch.dir.join("readonly.bin.dzdb").is_file(),
            "a read-only file must still get its annotations saved"
        );
    }

    /// Opening another file inside the same session must flush the first one's
    /// annotations rather than discard them.
    #[test]
    fn switching_files_flushes_the_previous_ones_annotations() {
        let scratch = Scratch::new();
        let first = scratch.file("first.bin");
        let second = scratch.file("second.bin");

        let mut app = App::new();
        app.config.database = true;
        app.load_file(&first, 0, true).expect("open first");
        Commands::comment(&mut app, 0x20, "from the first file".to_string());

        // No explicit save: opening another file is the only thing that happens.
        app.load_file(&second, 0, true).expect("open second");

        assert!(
            app.hex_view.comments.is_empty(),
            "the second file has no sidecar, so it must start clean"
        );

        let sidecar = scratch.dir.join("first.bin.dzdb");
        assert!(
            sidecar.is_file(),
            "the first file's comment was dropped when the second was opened"
        );

        // And it comes back when that file is opened again.
        app.load_file(&first, 0, true).expect("reopen first");
        assert_eq!(
            app.hex_view.comments.get(&0x20).map(String::as_str),
            Some("from the first file")
        );
    }

    /// Deleting the last annotation removes the sidecar instead of leaving an
    /// empty one behind that would be reloaded forever.
    #[test]
    fn removing_the_last_annotation_deletes_the_sidecar() {
        let scratch = Scratch::new();
        let path = scratch.file("clean.bin");
        let sidecar = scratch.dir.join("clean.bin.dzdb");

        let mut app = App::new();
        app.config.database = true;
        app.load_file(&path, 0, true).expect("open");

        Commands::comment(&mut app, 0x30, "temporary".to_string());
        app.persist_annotations();
        assert!(sidecar.is_file(), "precondition: the sidecar exists");

        // An empty comment deletes it, as `Commands::comment` documents.
        Commands::comment(&mut app, 0x30, String::new());
        app.persist_annotations();

        assert!(
            !sidecar.exists(),
            "an annotation-free file must not keep a stale sidecar"
        );
    }

    /// With the database disabled nothing is written at all.
    #[test]
    fn nothing_is_written_when_the_database_is_off() {
        let scratch = Scratch::new();
        let path = scratch.file("nodb.bin");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(&path, 0, true).expect("open");
        Commands::comment(&mut app, 0x10, "note".to_string());

        assert!(app.persist_annotations().is_none());
        assert!(
            !scratch.dir.join("nodb.bin.dzdb").exists(),
            "':set nodb' must mean no sidecar"
        );
    }
}
