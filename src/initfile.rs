use std::{env, error::Error, fs, path::PathBuf};

use directories_next::UserDirs;

use crate::{app::App, commands::parse_command};

/// True for the command forms that end the session.
///
/// Recognised before the line is executed, so the log can say why it was skipped
/// rather than the program simply vanishing.
fn is_quit_command(line: &str) -> bool {
    let head = line
        .trim_start_matches(':')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(head.as_str(), "q" | "quit" | "wq" | "x" | "exit")
}

impl App {
    /// Reads `.dzsrc` (or the older `.dz6init`), running each line as a command.
    ///
    /// `loading_initfile` is set for the duration so the `:set` handlers don't
    /// call `save_initfile()` while the file is being replayed - that used to
    /// rewrite the user's own `.dz6init` at every startup, throwing away their
    /// comments, blank lines and any option dz6 doesn't persist itself.
    pub fn read_initfile(&mut self) -> Result<(), Box<dyn Error>> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        // Each directory is tried with the current name first and the pre-rename
        // `.dz6init` second, so an existing config keeps working untouched.
        let names = [crate::app::INIT_FILE, crate::app::LEGACY_INIT_FILE];

        // 1. Directory where the executable is located
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                for name in names {
                    candidates.push(exe_dir.join(name));
                }
            }
        }

        // 2. Startup directory (not the live CWD, which the file dialog moves)
        for name in names {
            let cwd_path = crate::util::startup_dir().join(name);
            if !candidates.contains(&cwd_path) {
                candidates.push(cwd_path);
            }
        }

        // 3. User home directory
        if let Some(user_dirs) = UserDirs::new() {
            for name in names {
                let home_path = user_dirs.home_dir().join(name);
                if !candidates.contains(&home_path) {
                    candidates.push(home_path);
                }
            }
        }

        // Try reading from the first candidate that exists
        for path in candidates {
            if path.is_file() {
                if let Ok(data) = fs::read_to_string(&path) {
                    App::log(self, format!("Loading .dz6init from: {}", path.display()));
                    self.initfile_loaded = Some(path.clone());
                    self.loading_initfile = true;
                    for cmdline in data.lines() {
                        let trimmed = cmdline.trim();
                        // Skip empty lines and comments
                        if trimmed.is_empty() || trimmed.starts_with('#') {
                            continue;
                        }

                        // A config file must not be able to end the session or
                        // leave the app in an error state before the first frame
                        // is even drawn. Every line goes through `parse_command`,
                        // so a stray `q` quit immediately - with the terminal not
                        // yet in raw mode, that looked like dz6 failing to start -
                        // and a bad `set` installed the error dialog over the
                        // opening screen.
                        if is_quit_command(trimmed) {
                            App::log(
                                self,
                                format!("Ignoring '{}' in .dz6init: it would quit at startup", trimmed),
                            );
                            continue;
                        }

                        parse_command(self, cmdline);

                        if !self.running {
                            App::log(
                                self,
                                format!("'{}' in .dz6init asked to quit; ignoring", trimmed),
                            );
                            self.running = true;
                        }
                    }
                    self.loading_initfile = false;
                    // Any error a `set` line raised belongs in the log, not over
                    // the first frame.
                    if self.state == crate::editor::UIState::Error {
                        self.state = crate::editor::UIState::Normal;
                    }
                    self.dialog_renderer = None;
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Rewrites only the lines dz6 owns, leaving the rest of the file as it was.
///
/// `save_initfile` used to `fs::write` three lines over the whole file, so any
/// `:set enc1`, `:set enc2` or `:set lang` during a session deleted everything else
/// the user had put in `.dzsrc` - `set theme grey3` among them. The next launch had
/// no theme line, so it came up in the built-in dark theme, which is the "sometimes
/// it goes dark" this fixes. The guard against writing during the startup replay
/// carried a comment about exactly this hazard; it just did not cover the
/// interactive case.
///
/// What is preserved: comments, blank lines, ordering, any other command, the
/// keyword spelling the user chose (`encoding1` stays `encoding1`), a trailing
/// comment on a rewritten line, and the file's line endings.
pub fn merge_initfile(existing: Option<&str>, enc1: &str, enc2: &str, lang: &str) -> String {
    /// The three settings this writer owns, with every spelling `:set` accepts.
    const OWNED: [(&str, &[&str]); 3] = [
        ("enc1", &["enc1", "encoding1", "hex_mode_encoding"]),
        ("enc2", &["enc2", "encoding2", "hex_mode_second_encoding"]),
        ("lang", &["lang", "language"]),
    ];

    let value_for = |slot: &str| match slot {
        "enc1" => enc1,
        "enc2" => enc2,
        _ => lang,
    };

    let Some(existing) = existing.filter(|text| !text.trim().is_empty()) else {
        // Nothing to preserve: the file dz6 writes from scratch.
        return format!(
            "# dezes configuration\nset enc1 {}\nset enc2 {}\nset lang {}\n",
            enc1, enc2, lang
        );
    };

    // Keep the endings the file already uses, so a rewrite is not a whole-file
    // change in whatever the user diffs it with.
    let newline = if existing.contains("\r\n") { "\r\n" } else { "\n" };
    let ended_with_newline = existing.ends_with('\n');

    let mut written: Vec<&str> = Vec::new();
    let mut out: Vec<String> = Vec::new();

    for line in existing.lines() {
        match owned_slot(line, &OWNED) {
            Some((slot, rebuilt)) if !written.contains(&slot) => {
                written.push(slot);
                out.push(rebuilt(value_for(slot)));
            }
            // A second line for the same setting is dropped: the file is replayed
            // top to bottom, so a duplicate would override what was just written.
            Some(_) => continue,
            None => out.push(line.to_string()),
        }
    }

    for (slot, names) in OWNED {
        if !written.contains(&slot) {
            out.push(format!("set {} {}", names[0], value_for(slot)));
        }
    }

    let mut text = out.join(newline);
    if ended_with_newline || !text.is_empty() {
        text.push_str(newline);
    }
    text
}

/// If `line` is a `set` command for one of `owned`, returns its slot and a closure
/// that rebuilds the line around a new value.
///
/// The rebuild keeps everything except the value itself: leading whitespace, an
/// optional `:` prefix, the keyword as spelled, and a trailing comment.
fn owned_slot<'a>(
    line: &'a str,
    owned: &[(&'static str, &[&'static str])],
) -> Option<(&'static str, impl Fn(&str) -> String + use<'a>)> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let body = trimmed.strip_prefix(':').unwrap_or(trimmed);

    let mut words = body.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("set") {
        return None;
    }
    let key = words.next()?;
    let slot = owned
        .iter()
        .find(|(_, names)| names.iter().any(|name| name.eq_ignore_ascii_case(key)))
        .map(|(slot, _)| *slot)?;

    // Everything up to and including the keyword stays byte for byte.
    let key_end = line.find(key).map(|at| at + key.len())?;
    let head = &line[..key_end];

    // A trailing comment is the user's, not ours.
    let tail = &line[key_end..];
    let comment = tail.find('#').map(|at| tail[at..].to_string());

    let head = head.to_string();
    let indent_ok = indent_len <= line.len();
    debug_assert!(indent_ok);

    Some((slot, move |value: &str| match &comment {
        Some(comment) => format!("{} {} {}", head, value, comment),
        None => format!("{} {}", head, value),
    }))
}

#[cfg(test)]
mod initfile_tests {
    use super::is_quit_command;

    /// The quit forms have to be recognised before the line runs.
    ///
    /// Every `.dz6init` line goes through `parse_command`, so a stray `q` ended the
    /// session before the first frame - with the terminal not yet in raw mode, that
    /// looked like dz6 failing to start rather than like a config error.
    #[test]
    fn quit_commands_are_recognised() {
        for line in ["q", ":q", "quit", ":quit", "wq", ":wq", "x", ":x", "exit", "Q", "WQ"] {
            assert!(is_quit_command(line), "'{}' should be treated as a quit", line);
        }
        // With arguments too.
        assert!(is_quit_command("wq out.bin"));
        assert!(is_quit_command(":x out.bin"));
    }

    /// Everything a config file legitimately contains must still run.
    #[test]
    fn ordinary_commands_are_not_quits() {
        for line in [
            "set enc1 cp949",
            "set enc2 UTF-8",
            "set theme dark",
            "set disasmtheme light",
            "set byteline 16",
            "1000",
            "cmt 1000 note",
        ] {
            assert!(!is_quit_command(line), "'{}' must still run", line);
        }
    }

    /// Commands that merely start with a quit letter are not quits.
    #[test]
    fn similar_looking_commands_are_left_alone() {
        assert!(!is_quit_command("query"));
        assert!(!is_quit_command("xref"));
        assert!(!is_quit_command("quiet"));
        assert!(!is_quit_command(""));
    }
}

#[cfg(test)]
mod merge_tests {
    use super::merge_initfile;

    /// The reported bug: changing the encoding deleted every other line.
    ///
    /// `:set enc1` rewrote the file with three lines, so a hand-written
    /// `set theme grey3` was gone and the next launch came up dark.
    #[test]
    fn a_hand_written_file_keeps_everything_it_did_not_own() {
        let existing = "\
# my settings
set theme grey3
set disasmtheme grey3
set enc1 UTF-8
set enc2 none
set lang en
set byteline 16
";
        let merged = merge_initfile(Some(existing), "EUC-KR", "UTF-16LE", "ko");

        assert!(merged.contains("set theme grey3"), "the theme line was lost:\n{}", merged);
        assert!(merged.contains("set disasmtheme grey3"));
        assert!(merged.contains("set byteline 16"));
        assert!(merged.contains("# my settings"), "the comment was lost");

        // And the three it does own are updated, not duplicated.
        assert!(merged.contains("set enc1 EUC-KR"));
        assert!(merged.contains("set enc2 UTF-16LE"));
        assert!(merged.contains("set lang ko"));
        assert_eq!(merged.matches("set enc1").count(), 1);
        assert_eq!(merged.matches("set lang").count(), 1);
        assert!(!merged.contains("UTF-8"), "the old value is still there:\n{}", merged);
    }

    /// Order is kept, so a file stays recognisable to the person who wrote it.
    #[test]
    fn the_order_of_the_file_is_kept() {
        let existing = "set theme grey3\nset enc1 UTF-8\n# tail comment\n";
        let merged = merge_initfile(Some(existing), "EUC-KR", "none", "en");
        let lines: Vec<&str> = merged.lines().collect();

        assert_eq!(lines[0], "set theme grey3");
        assert_eq!(lines[1], "set enc1 EUC-KR");
        assert_eq!(lines[2], "# tail comment");
        // The two that were not in the file are appended.
        assert_eq!(lines[3], "set enc2 none");
        assert_eq!(lines[4], "set lang en");
    }

    /// No file yet: write the three lines dz6 owns.
    #[test]
    fn a_missing_file_is_created_from_scratch() {
        for existing in [None, Some(""), Some("   \n\n")] {
            let merged = merge_initfile(existing, "UTF-8", "none", "en");
            assert!(merged.starts_with("# dezes configuration"), "{:?}", merged);
            assert!(merged.contains("set enc1 UTF-8"));
            assert!(merged.contains("set enc2 none"));
            assert!(merged.contains("set lang en"));
        }
    }

    /// The keyword the user chose is theirs. `:set` takes three spellings for the
    /// first encoding, and rewriting one into another is a change nobody asked for.
    #[test]
    fn the_users_spelling_survives() {
        let existing = "set encoding1 UTF-8\nset hex_mode_second_encoding none\nset language en\n";
        let merged = merge_initfile(Some(existing), "EUC-KR", "UTF-8", "ko");

        assert!(merged.contains("set encoding1 EUC-KR"), "{}", merged);
        assert!(merged.contains("set hex_mode_second_encoding UTF-8"), "{}", merged);
        assert!(merged.contains("set language ko"), "{}", merged);
        assert!(!merged.contains("set enc1"), "a second line was added:\n{}", merged);
    }

    /// Indentation, a leading `:` and a trailing comment all belong to the user.
    #[test]
    fn decoration_around_the_value_is_left_alone() {
        let existing = "  :set lang en   # my language\n";
        let merged = merge_initfile(Some(existing), "UTF-8", "none", "zh");

        let line = merged.lines().next().expect("a line");
        assert!(line.starts_with("  :set lang "), "prefix lost: {:?}", line);
        assert!(line.contains("zh"), "value not replaced: {:?}", line);
        assert!(line.contains("# my language"), "comment lost: {:?}", line);
    }

    /// CRLF stays CRLF: a rewrite should not turn the whole file into a change.
    #[test]
    fn line_endings_are_preserved() {
        let crlf = "set theme grey3\r\nset enc1 UTF-8\r\n";
        let merged = merge_initfile(Some(crlf), "EUC-KR", "none", "en");
        assert!(merged.contains("\r\n"));
        assert!(!merged.replace("\r\n", "").contains('\n'), "mixed endings: {:?}", merged);

        let lf = "set theme grey3\nset enc1 UTF-8\n";
        let merged = merge_initfile(Some(lf), "EUC-KR", "none", "en");
        assert!(!merged.contains('\r'), "CR appeared out of nowhere: {:?}", merged);
    }

    /// A duplicate line for the same setting is dropped rather than left to
    /// override the value that was just written - the file is replayed in order.
    #[test]
    fn a_duplicate_setting_is_collapsed() {
        let existing = "set enc1 UTF-8\nset theme grey3\nset enc1 CP949\n";
        let merged = merge_initfile(Some(existing), "EUC-KR", "none", "en");

        assert_eq!(merged.matches("set enc1").count(), 1, "{}", merged);
        assert!(merged.contains("set enc1 EUC-KR"));
        assert!(merged.contains("set theme grey3"));
    }

    /// Saving twice with the same settings must not keep changing the file.
    #[test]
    fn merging_is_idempotent() {
        let existing = "# mine\nset theme grey3\nset enc1 UTF-8\nset enc2 none\nset lang en\n";
        let once = merge_initfile(Some(existing), "EUC-KR", "UTF-8", "ko");
        let twice = merge_initfile(Some(&once), "EUC-KR", "UTF-8", "ko");
        assert_eq!(once, twice);
    }

    /// Lines that merely look like a setting are not touched.
    #[test]
    fn other_commands_are_not_mistaken_for_settings() {
        let existing = "cmt 1000 enc1 is not a setting here\n1000\nset bg #2B3339\n";
        let merged = merge_initfile(Some(existing), "UTF-8", "none", "en");

        assert!(merged.contains("cmt 1000 enc1 is not a setting here"));
        assert!(merged.contains("set bg #2B3339"), "a colour command was rewritten:\n{}", merged);
        assert!(merged.contains("\n1000"), "a goto line was lost:\n{}", merged);
    }

    /// Every line the merge keeps still parses as a command, i.e. the rewrite
    /// cannot produce a file that the reader chokes on.
    #[test]
    fn rewritten_lines_are_still_commands() {
        let existing = "  :set   encoding1   UTF-8   # spaced out\n";
        let merged = merge_initfile(Some(existing), "EUC-KR", "none", "en");
        let line = merged.lines().next().expect("a line");

        let body = line.trim_start().trim_start_matches(':');
        let mut words = body.split_whitespace();
        assert_eq!(words.next(), Some("set"));
        assert_eq!(words.next(), Some("encoding1"));
        assert_eq!(words.next(), Some("EUC-KR"));
    }
}
