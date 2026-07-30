use crate::{app::App, util::parse_offset, widgets::MessageType};
use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{editor::UIState, widgets::Message};

use crate::app::Dz6Error;
use crate::i18n::M;
use clap::{Parser, Subcommand};
use ratatui::crossterm::event::{Event, KeyCode};
use std::io::Result;

pub struct Commands;

/// Upper bound for `:set byteline N` when the terminal size isn't known yet
/// (i.e. before the first frame has been drawn).
const MAX_BYTES_PER_LINE_FALLBACK: usize = 64;

#[derive(Subcommand, Debug)]
enum Command {
    Q,
    /// Program info dialog, same as F8. `:ver` is accepted as an alias.
    About,
    Ver,
    W {
        filename: Option<String>,
    },
    Wq {
        filename: Option<String>,
    },
    X {
        filename: Option<String>,
    },
    Wb {
        filename: String,
    },
    Wblock {
        filename: String,
    },
    O {
        filename: Option<String>,
    },
    Open {
        filename: Option<String>,
    },
    Cmt {
        offset: String,
        comment: String,
    },
    /// Undocumented.
    #[command(hide = true)]
    Matrix {
        /// `kana` for half-width katakana, `hex` for hex digits, nothing for the
        /// bytes of the open file.
        glyphs: Option<String>,
    },
    /// `:set` with no arguments lists every option and its current value.
    Set {
        option: Option<String>,
        value: Option<String>,
    },
}

#[derive(Parser, Debug)]
struct CommandLine {
    #[clap(subcommand)]
    command: Option<Command>,
}
#[derive(Debug, PartialEq)]
enum OffsetType {
    Backward,
    Absolute,
    Forward,
}

pub fn resolve_keywords(app: &App, input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return input.to_string();
    }

    // Fast-path Lazy Check: If input contains no keywords, return immediately (0ms)
    let lower_input = input.to_lowercase();
    // `cur` (the cursor's address) was called `sel`, which read as "selection" -
    // it has never had anything to do with the selected block.
    let has_cur = lower_input.contains("cur");
    let has_base = lower_input.contains("base");
    let has_oep = lower_input.contains("oep");

    if !has_cur && !has_base && !has_oep {
        return input.to_string();
    }

    let is_disasm = app.editor_view == crate::editor::AppView::Disasm;
    let cur_addr = if app.hex_view.show_va || is_disasm {
        app.get_va(app.hex_view.offset)
    } else {
        app.hex_view.offset as u64
    };
    let cur_str = format!("0x{:X}", cur_addr);
    let base_str = format!("0x{:X}", app.get_image_base());
    let oep_str = format!("0x{:X}", app.get_oep());

    let replace_word = |text: &str, word: &str, replacement: &str| -> String {
        let mut out = String::new();
        let mut i = 0;
        let len = text.len();
        let wlen = word.len();

        while i < len {
            if text.is_char_boundary(i) && i + wlen <= len && text.is_char_boundary(i + wlen) {
                if text[i..i + wlen].eq_ignore_ascii_case(word) {
                    let prev_is_alnum = if i > 0 {
                        let prev_ch = text[..i].chars().last().unwrap();
                        prev_ch.is_alphanumeric() || prev_ch == '_'
                    } else {
                        false
                    };
                    let next_is_alnum = if i + wlen < len {
                        let next_ch = text[i + wlen..].chars().next().unwrap();
                        next_ch.is_alphanumeric() || next_ch == '_'
                    } else {
                        false
                    };

                    if !prev_is_alnum && !next_is_alnum {
                        out.push_str(replacement);
                        i += wlen;
                        continue;
                    }
                }
            }

            if let Some(ch) = text[i..].chars().next() {
                out.push(ch);
                i += ch.len_utf8();
            } else {
                break;
            }
        }
        out
    };

    let mut result = input.to_string();
    if has_cur {
        result = replace_word(&result, "cur", &cur_str);
    }
    if has_base {
        result = replace_word(&result, "base", &base_str);
    }
    if has_oep {
        result = replace_word(&result, "oep", &oep_str);
    }

    result
}

pub fn parse_single_num(token: &str) -> Option<u64> {
    let clean = token.trim();
    if clean.is_empty() {
        return None;
    }

    if let Some(num_str) = clean.strip_suffix('t').or_else(|| clean.strip_suffix('T')) {
        return num_str.parse::<u64>().ok();
    }

    let hex_clean = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")).unwrap_or(clean);
    if let Ok(val) = u64::from_str_radix(hex_clean, 16) {
        return Some(val);
    }

    clean.parse::<u64>().ok()
}

pub fn eval_address_expression(app: &App, raw_expr: &str) -> Option<u64> {
    let resolved = resolve_keywords(app, raw_expr);
    let clean_expr = resolved.trim();

    if clean_expr.is_empty() {
        return None;
    }

    let mut current_op = '+';
    let mut token_buf = String::new();
    let mut ops_and_tokens: Vec<(char, String)> = Vec::new();

    for ch in clean_expr.chars() {
        if ch == '+' || ch == '-' {
            if !token_buf.trim().is_empty() {
                ops_and_tokens.push((current_op, token_buf.trim().to_string()));
                token_buf.clear();
            }
            current_op = ch;
        } else {
            token_buf.push(ch);
        }
    }
    if !token_buf.trim().is_empty() {
        ops_and_tokens.push((current_op, token_buf.trim().to_string()));
    }

    if ops_and_tokens.is_empty() {
        return None;
    }

    let mut current_val: u64 = 0;
    for (op, tok) in ops_and_tokens {
        let val = parse_single_num(&tok)?;
        if op == '+' {
            current_val = current_val.wrapping_add(val);
        } else if op == '-' {
            current_val = current_val.wrapping_sub(val);
        }
    }

    Some(current_val)
}

/// True when addresses the user types are virtual addresses rather than file
/// offsets.
///
/// The same condition the `g` handler uses to pre-fill the Goto box and that the
/// address column is drawn with, so what is shown, what is typed and what is jumped
/// to all mean the same thing.
pub fn addresses_are_virtual(app: &App) -> bool {
    app.hex_view.show_va || app.editor_view == crate::editor::AppView::Disasm
}

/// Resolves an evaluated address to a file offset, honouring the display mode.
///
/// Both Goto paths used to try `va_to_offset` first no matter what, so in offset
/// mode a value that happened to land inside a section was translated as a virtual
/// address: with `.text` at RVA 0x1000 / raw 0x400, typing `94D+1000` evaluated to
/// 0x194D and then jumped to offset 0xD4D. The arithmetic was right; the
/// interpretation was not, and it contradicted the offset the box had been
/// pre-filled with.
///
/// The other reading is kept as a fallback, but only when the primary one cannot
/// apply - a value past the end of the file cannot be an offset, and an address
/// outside every section cannot be translated - so it never silently overrides a
/// valid answer.
pub fn address_to_offset(app: &App, value: u64) -> Option<usize> {
    let limit = app.file_info.size.min(app.file_info.buffer_len());
    let as_offset = usize::try_from(value).ok().filter(|ofs| *ofs < limit);

    if addresses_are_virtual(app) {
        app.va_to_offset(value)
            .filter(|ofs| *ofs < limit)
            .or(as_offset)
    } else {
        as_offset.or_else(|| app.va_to_offset(value).filter(|ofs| *ofs < limit))
    }
}

fn try_goto(app: &mut App, raw_offset: &str) {
    let trimmed = raw_offset.trim();
    let offset_direction = if trimmed.starts_with('+') && !trimmed.contains("base") && !trimmed.contains("oep") && !trimmed.contains("cur") {
        OffsetType::Forward
    } else if trimmed.starts_with('-') && !trimmed.contains("base") && !trimmed.contains("oep") && !trimmed.contains("cur") {
        OffsetType::Backward
    } else {
        OffsetType::Absolute
    };

    let parsed_val = eval_address_expression(app, raw_offset);

    let mut final_ofs: Option<usize> = None;

    if let Some(val) = parsed_val {
        if offset_direction != OffsetType::Absolute {
            // A relative `+n` / `-n` is a byte count, not an address, so it is never
            // translated - it is added to the cursor below.
            final_ofs = usize::try_from(val).ok().filter(|n| *n < app.file_info.size);
        } else {
            final_ofs = address_to_offset(app, val);
        }
    }

    if let Some(mut ofs) = final_ofs {
        if offset_direction == OffsetType::Forward {
            ofs = app.hex_view.offset.saturating_add(ofs);
        } else if offset_direction == OffsetType::Backward {
            ofs = app.hex_view.offset.saturating_sub(ofs);
        }
        if ofs < app.file_info.size {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            app.goto(ofs);
            return;
        }
    }

    app.last_error = Dz6Error {
        message: format!(
            "Invalid range or address expression: {}; maximum offset for this file is 0x{:X}",
            raw_offset,
            app.file_info.size.saturating_sub(1)
        ),
    };
    app.dialog_renderer = Some(command_error_draw);
}

/// A translated message with no placeholders.
fn tr(app: &App, message: M) -> String {
    message.tr(app.config.lang).to_string()
}

/// A translated message with its placeholders filled in.
fn tr1(app: &App, message: M, arg: &str) -> String {
    crate::i18n::fill(message.tr(app.config.lang), &[arg])
}

fn tr2(app: &App, message: M, first: &str, second: &str) -> String {
    crate::i18n::fill(message.tr(app.config.lang), &[first, second])
}

/// "':set <option>' takes on, off or toggle, got '<value>'", in the interface
/// language. All six on/off options share it.
fn switch_error(app: &App, option: &str, got: &str) -> String {
    tr2(app, M::ErrSwitchValue, option, got)
}

/// Reports a bad `:set` (or any command) argument on the command bar.
///
/// Every failure mode in `:set` used to fall through the final `_ =>` arm and do
/// nothing at all: a misspelled option, a value the option cannot take, a missing
/// value. There was no way to tell that from "the option did what I asked".
fn command_error(app: &mut App, message: String) {
    app.last_error = Dz6Error { message };
    app.dialog_renderer = Some(command_error_draw);
    app.state = UIState::Normal;
}

/// Parses the `on | off | toggle` argument shared by every boolean option.
///
/// A missing value means "on", which is what `:set db` has always meant here -
/// `.dz6init` files rely on it, so it cannot become a toggle.
fn parse_switch(value: Option<&str>, current: bool) -> std::result::Result<bool, String> {
    let clean = value.map(|v| v.trim().to_ascii_lowercase());
    match clean.as_deref() {
        None | Some("") | Some("on") | Some("1") | Some("true") | Some("yes") => Ok(true),
        Some("off") | Some("0") | Some("false") | Some("no") => Ok(false),
        Some("toggle") | Some("!") => Ok(!current),
        Some(other) => Err(other.to_string()),
    }
}

/// Wraps `#RRGGBB` and `#RGB` in single quotes so the splitter keeps them.
///
/// `shell_words::split` follows POSIX, where `#` starts a comment: `set bg #2B3339`
/// arrived as `["set", "bg"]` and the colour was gone. Only a run of three or six
/// hex digits ending at a word boundary is protected, so `# a note` at the end of a
/// line is still dropped the way it always was.
fn quote_colour_literals(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len() + 8);
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'#' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        let digits = bytes[i + 1..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        let ends_word = bytes
            .get(i + 1 + digits)
            .is_none_or(|b| b.is_ascii_whitespace());

        if (digits == 3 || digits == 6) && ends_word {
            out.push('\'');
            out.push('#');
            out.push_str(&line[i + 1..i + 1 + digits]);
            out.push('\'');
            i += 1 + digits;
        } else {
            out.push('#');
            i += 1;
        }
    }

    out
}

pub fn parse_command(app: &mut App, cmdline_raw: &str) {
    let cmdline_str = quote_colour_literals(&resolve_keywords(app, cmdline_raw));
    let cmdline = cmdline_str.as_str();

    if cmdline.is_empty() {
        app.state = UIState::Normal;
        app.dialog_renderer = None;
        return;
    }

    let args = shell_words::split(cmdline).unwrap_or_default();
    let mut argv: Vec<&str> = Vec::with_capacity(args.len() + 1);
    argv.push("dezes");

    for s in args.iter() {
        argv.push(s.as_str());
    }

    match CommandLine::try_parse_from(argv) {
        Ok(cli) => match cli.command {
            // quit
            Some(Command::Q) => app.running = false,
            // program info
            Some(Command::About) | Some(Command::Ver) => app.open_about_dialog(),
            // open file
            Some(Command::O { filename }) | Some(Command::Open { filename }) => {
                if let Some(fname) = filename {
                    let fname_clean = fname.trim();
                    if fname_clean.is_empty() {
                        app.open_file_dialog();
                    } else {
                        match app.load_file(fname_clean, 0, false) {
                            Ok(_) => {
                                App::log(app, format!("Opened file '{}'", fname_clean));
                                app.state = UIState::Normal;
                                app.dialog_renderer = None;
                            }
                            Err(e) => {
                                app.last_error = Dz6Error {
                                    message: format!("Error opening '{}': {}", fname_clean, e),
                                };
                                app.dialog_renderer = Some(command_error_draw);
                            }
                        }
                    }
                } else {
                    app.open_file_dialog();
                }
            }
            // write to file
            Some(Command::W { filename }) => {
                let res = if let Some(fname) = filename {
                    let fname_clean = fname.trim();
                    if fname_clean.is_empty() {
                        app.write_to_file()
                    } else {
                        let path_input = std::path::Path::new(fname_clean);
                        let target_path = if path_input.is_absolute() {
                            path_input.to_path_buf()
                        } else {
                            let current_path = std::path::Path::new(&app.file_info.path);
                            if let Some(parent) = current_path.parent() {
                                if !parent.as_os_str().is_empty() {
                                    parent.join(path_input)
                                } else {
                                    path_input.to_path_buf()
                                }
                            } else {
                                path_input.to_path_buf()
                            }
                        };
                        app.write_to_file_as(&target_path)
                    }
                } else {
                    app.write_to_file()
                };

                if let Err(e) = res {
                    app.last_error = Dz6Error {
                        message: crate::i18n::fill(M::ErrSaveError.tr(app.config.lang), &[&e.to_string()]),
                    };
                    app.dialog_renderer = Some(command_error_draw);
                } else {
                    // `persist_annotations` checks `config.database` itself and
                    // logs a failed write, which `let _ = save_database()` hid.
                    app.persist_annotations();
                    app.dialog_renderer = None;
                    app.state = UIState::Normal;
                }
            }
            // write and quit
            Some(Command::Wq { filename }) | Some(Command::X { filename }) => {
                let res = if let Some(fname) = filename {
                    let fname_clean = fname.trim();
                    if fname_clean.is_empty() {
                        app.write_to_file()
                    } else {
                        let path_input = std::path::Path::new(fname_clean);
                        let target_path = if path_input.is_absolute() {
                            path_input.to_path_buf()
                        } else {
                            let current_path = std::path::Path::new(&app.file_info.path);
                            if let Some(parent) = current_path.parent() {
                                if !parent.as_os_str().is_empty() {
                                    parent.join(path_input)
                                } else {
                                    path_input.to_path_buf()
                                }
                            } else {
                                path_input.to_path_buf()
                            }
                        };
                        app.write_to_file_as(&target_path)
                    }
                } else {
                    app.write_to_file()
                };

                if let Err(e) = res {
                    app.last_error = Dz6Error {
                        message: crate::i18n::fill(M::ErrSaveError.tr(app.config.lang), &[&e.to_string()]),
                    };
                    app.dialog_renderer = Some(command_error_draw);
                } else {
                    app.persist_annotations();
                    app.dialog_renderer = None;
                    app.running = false;
                }
            }
            // write selected block to file
            Some(Command::Wb { filename }) | Some(Command::Wblock { filename }) => {
                let fname_clean = filename.trim();
                if fname_clean.is_empty() {
                    app.last_error = Dz6Error {
                        message: "Filename required for :wb (e.g. :wb dump.bin)".to_string(),
                    };
                    app.dialog_renderer = Some(command_error_draw);
                } else {
                    let path_input = std::path::Path::new(fname_clean);
                    let target_path = if path_input.is_absolute() {
                        path_input.to_path_buf()
                    } else {
                        let current_path = std::path::Path::new(&app.file_info.path);
                        if let Some(parent) = current_path.parent() {
                            if !parent.as_os_str().is_empty() {
                                parent.join(path_input)
                            } else {
                                path_input.to_path_buf()
                            }
                        } else {
                            path_input.to_path_buf()
                        }
                    };

                    match app.write_block_to_file(&target_path) {
                        Ok(_) => {
                            app.dialog_renderer = None;
                            app.state = UIState::Normal;
                        }
                        Err(e) => {
                            app.last_error = Dz6Error {
                                message: format!("Save block error: {}", e),
                            };
                            app.dialog_renderer = Some(command_error_draw);
                        }
                    }
                }
            }
            // comment <offset> <comment>
            Some(Command::Cmt { offset, comment }) => {
                if let Ok(mut ofs) = parse_offset(&offset) {
                    if offset.starts_with('+') {
                        ofs = ofs.saturating_add(app.hex_view.offset);
                    }
                    if ofs < app.file_info.size {
                        Commands::comment(app, ofs, comment);
                        app.dialog_renderer = None;
                    } else {
                        app.last_error = Dz6Error {
                            message: format!(
                                "Invalid range: {}; maximum offset for this file is {}",
                                cmdline,
                                app.file_info.size.saturating_sub(1)
                            ),
                        };
                        app.dialog_renderer = Some(command_error_draw);
                    }
                } else {
                    app.last_error = Dz6Error {
                        message: format!("Invalid argument: {}", offset),
                    };
                    app.dialog_renderer = Some(command_error_draw);
                }
                app.state = UIState::Normal;
            }
            // set
            Some(Command::Matrix { glyphs }) => {
            use crate::global::matrix::GlyphSource;
            match GlyphSource::parse(glyphs.as_deref()) {
                Some(source) => crate::global::matrix::open(app, source),
                // Katakana needs a font that has it, so the choice is explicit and a
                // typo should say so rather than silently rain something else.
                None => command_error(
                    app,
                    format!(
                        "':matrix' takes kana, hex or nothing, got '{}'",
                        glyphs.unwrap_or_default()
                    ),
                ),
            }
        }
        Some(Command::Set { option, value }) => {
                // Bare `:set` shows the table instead of being an error, which is
                // the only way to find out what anything is currently set to.
                let Some(option) = option else {
                    if app.loading_initfile {
                        // A `.dz6init` line must not leave a dialog open over the
                        // first frame; the table goes to the log instead.
                        let table = crate::global::settings::settings_text(app);
                        App::log(app, format!("Settings:\n{}", table));
                    } else {
                        crate::global::settings::open_settings_dialog(app);
                    }
                    return;
                };
                let option = option.trim().to_ascii_lowercase();
                match option.as_str() {
                    // bytes per line
                    "byteline" | "width" => {
                        let Some(val) = value else {
                            command_error(app, tr(app, M::ErrNeedsNumberAuto));
                            return;
                        };
                        let val = val.trim().to_ascii_lowercase();
                        if let Ok(bpl) = val.parse::<usize>() {
                            if bpl == 0 {
                                command_error(app, tr(app, M::ErrBytelineZero));
                                return;
                            }
                            // Clamped: `:set byteline 0` used to be accepted and
                            // then panicked on the next frame in six different draw
                            // paths (division by zero and `bpl - 1` underflows).
                            let max = if app.screen.width > 0 {
                                crate::util::max_bytes_per_line(app.screen.width)
                            } else {
                                MAX_BYTES_PER_LINE_FALLBACK
                            };
                            app.config.hex_mode_bytes_per_line = bpl.clamp(1, max);
                            app.config.hex_mode_bytes_per_line_auto = false;
                            App::log(app, format!("Bytes per line: {}", app.config.hex_mode_bytes_per_line));
                        } else if val == "auto" {
                            app.config.hex_mode_bytes_per_line_auto = true;
                            if app.screen.width > 0 {
                                app.config.hex_mode_bytes_per_line =
                                    crate::util::max_bytes_per_line(app.screen.width);
                            }
                            App::log(app, "Bytes per line: auto".to_string());
                        } else {
                            command_error(app, tr1(app, M::ErrNotByteCount, &val));
                            return;
                        }
                        app.dialog_renderer = None;
                    }
                    // control / non-graphic bytes
                    "ctrlchar" => {
                        // `chars().count()`, not `len()`: the latter is a byte
                        // count, so a single multi-byte character was rejected.
                        match value.as_deref() {
                            Some(val) if val.chars().count() == 1 => {
                                let c = val.chars().next().expect("one character");
                                app.config.hex_mode_non_graphic_char = c;
                                app.dialog_renderer = None;
                            }
                            Some(val) => {
                                command_error(app, tr1(app, M::ErrOneCharacter, val));
                                return;
                            }
                            None => {
                                command_error(app, tr(app, M::ErrNeedsCharacter));
                                return;
                            }
                        }
                    }
                    // save database files <filename>.dz6
                    "db" => {
                        match parse_switch(value.as_deref(), app.config.database) {
                            Ok(on) => {
                                app.config.database = on;
                                App::log(app, format!("Database sidecar: {}", if on { "on" } else { "off" }));
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "db", &bad));
                                return;
                            }
                        }
                    }
                    "nodb" => {
                        app.config.database = false;
                        app.dialog_renderer = None;
                    }
                    // dim (gray out) control bytes
                    //
                    // Independent of `dimzero` now. Setting one used to clear the
                    // other, and the only way off was `nodim`, which killed both -
                    // so "dim control bytes but not nulls" was unreachable.
                    "dimctrl" => {
                        match parse_switch(value.as_deref(), app.config.dim_control_chars) {
                            Ok(on) => {
                                app.config.dim_control_chars = on;
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "dimctrl", &bad));
                                return;
                            }
                        }
                    }
                    // dim null bytes
                    "dimzero" => {
                        match parse_switch(value.as_deref(), app.config.dim_zeroes) {
                            Ok(on) => {
                                app.config.dim_zeroes = on;
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "dimzero", &bad));
                                return;
                            }
                        }
                    }
                    "nodim" => {
                        app.config.dim_control_chars = false;
                        app.config.dim_zeroes = false;
                        app.dialog_renderer = None;
                    }
                    // primary encoding: hex view text column, plus the first
                    // field of the Edit Data and Find Pattern dialogs.
                    // No "none" here - the hex view always needs a text column.
                    "enc1" | "encoding1" | "hex_mode_encoding" => {
                        if let Some(val) = value {
                            match crate::text::dialog_encoding::encoding_from_name(&val) {
                                Some(enc) => {
                                    app.text_view.table = enc;
                                    App::log(app, format!("Set primary encoding (enc1) to: {}", enc.name()));
                                    if !app.loading_initfile {
                                        app.save_initfile();
                                    }
                                }
                                None => {
                                    app.last_error = Dz6Error {
                                        message: tr2(
                                            app,
                                            M::ErrUnknownEncoding,
                                            val.trim(),
                                            "utf-8, cp949, cp936, iso-8859-1, iso-8859-2, utf-16le, utf-16be",
                                        ),
                                    };
                                    app.dialog_renderer = Some(command_error_draw);
                                    return;
                                }
                            }
                        }
                        app.dialog_renderer = None;
                    }
                    "enc2" | "encoding2" | "hex_mode_second_encoding" => {
                        if let Some(val) = value {
                            if crate::text::dialog_encoding::is_encoding_none(&val) {
                                app.hex_view.enc2_table = None;
                                App::log(app, "Set secondary encoding (enc2) to: none".to_string());
                                if !app.loading_initfile {
                                    app.save_initfile();
                                }
                            } else {
                                match crate::text::dialog_encoding::encoding_from_name(&val) {
                                    Some(enc) => {
                                        app.hex_view.enc2_table = Some(enc);
                                        App::log(app, format!("Set secondary encoding (enc2) to: {}", enc.name()));
                                        if !app.loading_initfile {
                                            app.save_initfile();
                                        }
                                    }
                                    None => {
                                        app.last_error = Dz6Error {
                                            message: tr2(
                                                app,
                                                M::ErrUnknownEncoding,
                                                val.trim(),
                                                "none, utf-8, cp949, cp936, iso-8859-1, iso-8859-2, utf-16le, utf-16be",
                                            ),
                                        };
                                        app.dialog_renderer = Some(command_error_draw);
                                        return;
                                    }
                                }
                            }
                        }
                        app.dialog_renderer = None;
                    }
                    // theme
                    //
                    // A theme file now carries the disassembly colours as well,
                    // so one command colours both views. Older files that have
                    // no disassembly section resolve to the built-in preset of
                    // the same name, and anything else leaves the disassembly
                    // colours as they were rather than snapping them back to the
                    // compiled-in defaults.
                    "theme" => {
                        if let Some(val) = value {
                            // Refuse a file that has no main-view keys at all -
                            // a disassembly-only theme. Loading it would leave
                            // the fallback (near-black dark) in place and look
                            // like the theme had been applied.
                            if let Some(path) = crate::themes::find_theme_path(&val)
                                && let Ok(data) = std::fs::read_to_string(&path)
                                && !crate::themes::has_main_keys(&data)
                            {
                                let disasm_only =
                                    crate::disasm::theme::has_disasm_keys(&data);
                                app.last_error = Dz6Error {
                                    message: if disasm_only {
                                        format!(
                                            "'{}' holds only disassembly colours - use ':set disasmtheme {}'",
                                            val.trim(),
                                            val.trim()
                                        )
                                    } else {
                                        format!(
                                            "'{}' has no theme colours in it (expected keys like main_fg, main_bg)",
                                            val.trim()
                                        )
                                    },
                                };
                                app.dialog_renderer = Some(command_error_draw);
                                return;
                            }

                            app.config.theme = crate::themes::load_theme_or_fallback(&val);
                            // Only a theme file that *declares* disassembly colours
                            // changes them. Without this the same-named built-in
                            // preset was applied, so picking a hex-view theme threw
                            // away whatever `:set disasmtheme` had been used to set -
                            // and `.dz6init`'s `set theme` line did it again on every
                            // launch. Use `:set disasmtheme <name>` to change them.
                            match crate::disasm::theme::disasm_theme_from_file(&val) {
                                Some(dt) => {
                                    app.config.disasm_theme = dt;
                                    crate::disasm::theme::save_disasm_theme(&app.config.disasm_theme);
                                    App::log(
                                        app,
                                        format!(
                                            "Theme '{}' applied (hex + disassembly)",
                                            val.trim()
                                        ),
                                    );
                                }
                                None => App::log(
                                    app,
                                    format!(
                                        "Theme '{}' applied (hex only; disassembly colours kept)",
                                        val.trim()
                                    ),
                                ),
                            }
                            app.dialog_renderer = None;
                        }
                    }
                    // forced decoding width: 16 / 32 / 64 / auto
                    "bitness" | "bits" => {
                        match value.as_deref().map(str::trim) {
                            Some("auto") | Some("") | None => {
                                app.config.bitness_override = None;
                                let label = app.describe_bitness();
                                App::log(app, format!("Decoding width: {}", label));
                                app.dialog_renderer = None;
                            }
                            Some(val) => match val.parse::<u32>() {
                                Ok(bits) if App::BITNESS_CHOICES.contains(&bits) => {
                                    app.config.bitness_override = Some(bits);
                                    let label = app.describe_bitness();
                                    App::log(app, format!("Decoding width: {}", label));
                                    app.dialog_renderer = None;
                                }
                                _ => {
                                    app.last_error = Dz6Error {
                                        message: format!(
                                            "Invalid width '{}' (expected 16, 32, 64 or auto)",
                                            val
                                        ),
                                    };
                                    app.dialog_renderer = Some(command_error_draw);
                                    return;
                                }
                            },
                        }
                    }
                    // bottom hint line
                    "hintbar" | "hints" => {
                        match parse_switch(value.as_deref(), app.config.hint_bar) {
                            Ok(on) => {
                                app.config.hint_bar = on;
                                App::log(app, format!("Hint bar: {}", if on { "on" } else { "off" }));
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "hintbar", &bad));
                                return;
                            }
                        }
                    }
                    "nohintbar" | "nohints" => {
                        app.config.hint_bar = false;
                        App::log(app, "Hint bar: off".to_string());
                        app.dialog_renderer = None;
                    }
                    // IME indicator (`EN` / `Han`) in the status bar.
                    //
                    // Undocumented on purpose: not in the `:set` table, not in the
                    // name list the typo suggestions come from, not in the help.
                    // It is a switch for the few people who type through an IME,
                    // and the indicator costs a window-message round-trip to read.
                    "han" => {
                        match parse_switch(value.as_deref(), app.config.show_ime) {
                            Ok(on) => {
                                app.config.show_ime = on;
                                App::log(app, format!("IME indicator: {}", if on { "on" } else { "off" }));
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "han", &bad));
                                return;
                            }
                        }
                    }

                    // search wrap
                    "wrapscan" => {
                        match parse_switch(value.as_deref(), app.config.search_wrap) {
                            Ok(on) => {
                                app.config.search_wrap = on;
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "wrapscan", &bad));
                                return;
                            }
                        }
                    }
                    "nowrapscan" => {
                        app.config.search_wrap = false;
                        app.dialog_renderer = None;
                    }
                    // syntax highlighting (highlight / hilight)
                    "highlight" | "hilight" => {
                        match parse_switch(value.as_deref(), app.config.syntax_highlight) {
                            Ok(on) => {
                                app.config.syntax_highlight = on;
                                app.dialog_renderer = None;
                            }
                            Err(bad) => {
                                command_error(app, switch_error(app, "highlight", &bad));
                                return;
                            }
                        }
                    }
                    "nohilight" | "nohighlight" => {
                        app.config.syntax_highlight = false;
                        app.dialog_renderer = None;
                    }
                    // view
                    //
                    // `disasm` used to be missing, so the one view you might
                    // actually want to reach from `.dz6init` was the one view this
                    // could not select - and an unknown name did nothing at all.
                    "view" => {
                        use crate::editor::AppView;
                        let target = match value.as_deref().map(str::trim) {
                            Some("hex") => Some(AppView::Hex),
                            Some("disasm") | Some("disassembly") | Some("asm") => Some(AppView::Disasm),
                            Some("text") => Some(AppView::Text),
                            Some("header") => Some(AppView::Header),
                            other => {
                                command_error(
                                    app,
                                    tr1(app, M::ErrViewNames, other.unwrap_or("")),
                                );
                                return;
                            }
                        };
                        if let Some(target) = target {
                            if target == AppView::Disasm && !app.is_executable() {
                                command_error(app, tr(app, M::ErrNoCodeSection));
                                return;
                            }
                            if app.editor_view == AppView::Hex || app.editor_view == AppView::Disasm {
                                app.prev_editor_view = app.editor_view;
                            }
                            app.editor_view = target;
                            if target == AppView::Hex || target == AppView::Disasm {
                                app.last_primary_view = target;
                            }
                            // The two views read `page_start` differently; see
                            // `App::align_page_for_view`.
                            app.align_page_for_view();
                            app.dialog_renderer = None;
                        }
                    }
                    // interface language
                    //
                    // Persisted through `save_initfile`, like the encodings: a
                    // language that resets to English on every launch is worse than
                    // not having the option.
                    "lang" | "language" => {
                        let names = crate::i18n::Lang::ALL
                            .iter()
                            .map(|l| l.name())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let Some(val) = value else {
                            command_error(app, tr1(app, M::ErrLangNeedsValue, &names));
                            return;
                        };
                        match crate::i18n::Lang::from_name(&val) {
                            Some(lang) => {
                                app.config.lang = lang;
                                App::log(app, format!("Language: {}", lang.label()));
                                if !app.loading_initfile {
                                    app.save_initfile();
                                }
                                app.dialog_renderer = None;
                            }
                            None => {
                                command_error(
                                    app,
                                    tr2(app, M::ErrUnknownLang, val.trim(), &names),
                                );
                                return;
                            }
                        }
                    }
                    // address column: VA or file offset
                    //
                    // `:set va` and `:set offset` were two options for one setting.
                    // They still work, as aliases.
                    "addr" | "address" => {
                        match value.as_deref().map(|v| v.trim().to_ascii_lowercase()).as_deref() {
                            Some("va") => app.hex_view.show_va = true,
                            Some("offset") | Some("ofs") => app.hex_view.show_va = false,
                            Some("toggle") | Some("!") | None | Some("") => {
                                app.hex_view.show_va = !app.hex_view.show_va;
                            }
                            Some(other) => {
                                command_error(app, tr1(app, M::ErrAddrNames, other));
                                return;
                            }
                        }
                        let mode = if app.hex_view.show_va { "VA" } else { "file offset" };
                        App::log(app, format!("Address display: {}", mode));
                        app.dialog_renderer = None;
                    }
                    "va" => {
                        app.hex_view.show_va = true;
                        app.dialog_renderer = None;
                    }
                    "offset" => {
                        app.hex_view.show_va = false;
                        app.dialog_renderer = None;
                    }
                    // whole disassembly theme by name
                    //
                    // Saved into `disasm.theme` like the individual
                    // `:set disasm_*` colour commands, so the choice survives a
                    // restart without needing a separate entry in `.dz6init`.
                    "disasmtheme" | "disasm_theme" => {
                        if let Some(val) = value {
                            match crate::disasm::theme::resolve_disasm_theme(&val) {
                                Some(theme) => {
                                    app.config.disasm_theme = theme;
                                    crate::disasm::theme::save_disasm_theme(&app.config.disasm_theme);
                                    App::log(
                                        app,
                                        format!("Loaded disassembly theme '{}'", val.trim()),
                                    );
                                    app.dialog_renderer = None;
                                }
                                None => {
                                    app.last_error = Dz6Error {
                                        message: format!(
                                            "Unknown disassembly theme '{}' (available: {}; or give a path to a .theme file)",
                                            val.trim(),
                                            crate::disasm::theme::DISASM_PRESETS.join(", ")
                                        ),
                                    };
                                    app.dialog_renderer = Some(command_error_draw);
                                    return;
                                }
                            }
                        } else {
                            app.dialog_renderer = None;
                        }
                    }
                    // disassembly colours, and everything unrecognised.
                    //
                    // The eight colour options were eight near-identical arms that
                    // each ignored a bad colour string in silence. One lookup and
                    // one assignment now, with a real error on a bad value.
                    other => {
                        if let Some(key) = crate::themes::resolve_color_key(other) {
                            let Some(val) = value else {
                                command_error(app, format!("':set {}' needs a colour", other));
                                return;
                            };
                            let text = val.trim().to_string();
                            let color = match crate::themes::Theme::parse_color(&text) {
                                ratatui::style::Color::Reset => {
                                    crate::disasm::theme::parse_color_str(&text)
                                }
                                parsed => Some(parsed),
                            };
                            match color {
                                Some(color) => {
                                    app.config.theme.apply_color(key, color);
                                    // The disassembly view caches rendered rows and
                                    // fingerprints the styles they were built with,
                                    // so the colour change reaches it on the next
                                    // frame without any extra bookkeeping here.
                                    app.dialog_renderer = None;
                                    App::log(app, format!("{} = {}", key, text));
                                }
                                None => {
                                    command_error(app, tr1(app, M::ErrNotColour, &text));
                                    return;
                                }
                            }
                        } else if let Some(target) = crate::global::settings::disasm_color_target(other) {
                            let Some(val) = value else {
                                command_error(app, tr1(app, M::ErrNeedsColour, other));
                                return;
                            };
                            match crate::disasm::theme::parse_color_str(&val) {
                                Some(color) => {
                                    crate::global::settings::set_disasm_color(
                                        &mut app.config.disasm_theme,
                                        target,
                                        color,
                                    );
                                    crate::disasm::theme::save_disasm_theme(&app.config.disasm_theme);
                                    app.view_generation = app.view_generation.wrapping_add(1);
                                    app.dialog_renderer = None;
                                }
                                None => {
                                    command_error(app, tr1(app, M::ErrNotColour, val.trim()));
                                    return;
                                }
                            }
                        } else {
                            let message = match crate::global::settings::suggest(other) {
                                Some(name) => tr2(app, M::ErrUnknownOptionSuggest, other, name),
                                None => tr1(app, M::ErrUnknownOption, other),
                            };
                            command_error(app, message);
                            return;
                        }
                    }
                }
                app.state = UIState::Normal;
            }
            None => {
                try_goto(app, &cmdline);
            }
        },
        Err(_) => {
            // goto as :offset
            try_goto(app, &cmdline);
        }
    }
}

// command bar

pub fn command_draw(app: &mut App, frame: &mut Frame) {
    let val = app.command_input.input.value();
    let chars: Vec<char> = val.chars().collect();
    let char_count = chars.len();

    let cur = app.command_input.cursor_pos.min(char_count);

    let (sel_start, sel_end) = if let Some(anchor) = app.command_input.selection_anchor {
        let a = anchor.min(char_count);
        if a != cur {
            (a.min(cur), a.max(cur))
        } else {
            (cur, cur)
        }
    } else {
        (cur, cur)
    };

    let main_style = app.config.theme.main;
    let hl_style = app.config.theme.highlight;

    let mut spans = Vec::new();
    spans.push(Span::styled(":", main_style));

    if sel_start < sel_end {
        let before: String = chars[..sel_start].iter().collect();
        let selected: String = chars[sel_start..sel_end].iter().collect();
        let after: String = chars[sel_end..].iter().collect();

        if !before.is_empty() {
            spans.push(Span::styled(before, main_style));
        }
        spans.push(Span::styled(selected, hl_style));
        if !after.is_empty() {
            spans.push(Span::styled(after, main_style));
        }
    } else {
        spans.push(Span::styled(val.to_string(), main_style));
    }

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(main_style);

    frame.render_widget(Clear, app.command_area);
    frame.render_widget(para, app.command_area);
    frame.set_cursor_position((app.command_area.x + 1 + cur as u16, app.command_area.y));
}

pub fn command_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        let val = app.command_input.input.value().to_string();
        let mut chars: Vec<char> = val.chars().collect();
        let len = chars.len();
        let cur = app.command_input.cursor_pos.min(len);
        let has_shift = key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::SHIFT);
        let has_ctrl = key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL);

        let get_selection = |app: &App, cur: usize, len: usize| -> Option<(usize, usize)> {
            if let Some(anchor) = app.command_input.selection_anchor {
                let a = anchor.min(len);
                if a != cur {
                    return Some((a.min(cur), a.max(cur)));
                }
            }
            None
        };

        let delete_word_left = |app: &mut App, chars: &mut Vec<char>, cur: usize, len: usize| {
            if let Some((s, e)) = get_selection(app, cur, len) {
                chars.drain(s..e);
                let new_val: String = chars.iter().collect();
                app.command_input.input = tui_input::Input::new(new_val);
                app.command_input.cursor_pos = s;
                app.command_input.selection_anchor = None;
            } else if cur > 0 {
                let mut target = cur;
                while target > 0 && chars[target - 1].is_whitespace() {
                    target -= 1;
                }
                if target > 0 {
                    let is_alnum = chars[target - 1].is_alphanumeric() || chars[target - 1] == '_';
                    while target > 0 {
                        let ch = chars[target - 1];
                        if (ch.is_alphanumeric() || ch == '_') == is_alnum && !ch.is_whitespace() {
                            target -= 1;
                        } else {
                            break;
                        }
                    }
                }
                chars.drain(target..cur);
                let new_val: String = chars.iter().collect();
                app.command_input.input = tui_input::Input::new(new_val);
                app.command_input.cursor_pos = target;
                app.command_input.selection_anchor = None;
            }
        };

        match key.code {
            KeyCode::Esc => {
                app.command_input.selection_anchor = None;
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
            KeyCode::Enter => {
                app.command_input.selection_anchor = None;
                let v = app.command_input.input.value_and_reset();
                app.command_input.cursor_pos = 0;
                parse_command(app, &v);
                app.command_input.push(v);
            }
            KeyCode::Up => {
                app.command_input.up();
            }
            KeyCode::Down => {
                app.command_input.down();
            }
            KeyCode::Home => {
                if has_shift {
                    if app.command_input.selection_anchor.is_none() {
                        app.command_input.selection_anchor = Some(cur);
                    }
                } else {
                    app.command_input.selection_anchor = None;
                }
                app.command_input.cursor_pos = 0;
            }
            KeyCode::End => {
                if has_shift {
                    if app.command_input.selection_anchor.is_none() {
                        app.command_input.selection_anchor = Some(cur);
                    }
                } else {
                    app.command_input.selection_anchor = None;
                }
                app.command_input.cursor_pos = len;
            }
            KeyCode::Left => {
                if has_shift {
                    if app.command_input.selection_anchor.is_none() {
                        app.command_input.selection_anchor = Some(cur);
                    }
                    app.command_input.cursor_pos = cur.saturating_sub(1);
                } else {
                    if let Some((s, _)) = get_selection(app, cur, len) {
                        app.command_input.cursor_pos = s;
                    } else {
                        app.command_input.cursor_pos = cur.saturating_sub(1);
                    }
                    app.command_input.selection_anchor = None;
                }
            }
            KeyCode::Right => {
                if has_shift {
                    if app.command_input.selection_anchor.is_none() {
                        app.command_input.selection_anchor = Some(cur);
                    }
                    app.command_input.cursor_pos = (cur + 1).min(len);
                } else {
                    if let Some((_, e)) = get_selection(app, cur, len) {
                        app.command_input.cursor_pos = e;
                    } else {
                        app.command_input.cursor_pos = (cur + 1).min(len);
                    }
                    app.command_input.selection_anchor = None;
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') if has_ctrl => {
                if let Some((s, e)) = get_selection(app, cur, len) {
                    let sel_str: String = chars[s..e].iter().collect();
                    if let Ok(clipboard) = &mut app.clipboard {
                        let _ = clipboard.set_text(sel_str);
                    }
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') if has_ctrl => {
                if let Some((s, e)) = get_selection(app, cur, len) {
                    let sel_str: String = chars[s..e].iter().collect();
                    if let Ok(clipboard) = &mut app.clipboard {
                        let _ = clipboard.set_text(sel_str);
                    }
                    chars.drain(s..e);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = s;
                    app.command_input.selection_anchor = None;
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') if has_ctrl => {
                if let Ok(clipboard) = &mut app.clipboard {
                    if let Ok(text) = clipboard.get_text() {
                        let text_clean = text.trim();
                        if let Some((s, e)) = get_selection(app, cur, len) {
                            chars.drain(s..e);
                            let paste_chars: Vec<char> = text_clean.chars().collect();
                            for (idx, ch) in paste_chars.into_iter().enumerate() {
                                chars.insert(s + idx, ch);
                            }
                            let new_val: String = chars.into_iter().collect();
                            app.command_input.input = tui_input::Input::new(new_val);
                            app.command_input.cursor_pos = s + text_clean.chars().count();
                        } else {
                            let paste_chars: Vec<char> = text_clean.chars().collect();
                            for (idx, ch) in paste_chars.into_iter().enumerate() {
                                chars.insert(cur + idx, ch);
                            }
                            let new_val: String = chars.into_iter().collect();
                            app.command_input.input = tui_input::Input::new(new_val);
                            app.command_input.cursor_pos = cur + text_clean.chars().count();
                        }
                        app.command_input.selection_anchor = None;
                    }
                }
            }
            KeyCode::Char('\u{7f}') | KeyCode::Char('\u{8}') => {
                delete_word_left(app, &mut chars, cur, len);
            }
            KeyCode::Backspace if has_ctrl => {
                delete_word_left(app, &mut chars, cur, len);
            }
            KeyCode::Backspace => {
                if let Some((s, e)) = get_selection(app, cur, len) {
                    chars.drain(s..e);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = s;
                    app.command_input.selection_anchor = None;
                } else if cur > 0 {
                    chars.remove(cur - 1);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = cur - 1;
                }
            }
            KeyCode::Delete => {
                if let Some((s, e)) = get_selection(app, cur, len) {
                    chars.drain(s..e);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = s;
                    app.command_input.selection_anchor = None;
                } else if cur < len {
                    chars.remove(cur);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = cur;
                }
            }
            KeyCode::Char(c) if !has_ctrl => {
                if let Some((s, e)) = get_selection(app, cur, len) {
                    chars.drain(s..e);
                    chars.insert(s, c);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = s + 1;
                } else {
                    chars.insert(cur, c);
                    let new_val: String = chars.into_iter().collect();
                    app.command_input.input = tui_input::Input::new(new_val);
                    app.command_input.cursor_pos = cur + 1;
                }
                app.command_input.selection_anchor = None;
            }
            _ => {}
        }
    }
    Ok(false)
}

pub fn command_error_draw(app: &mut App, frame: &mut Frame) {
    let mut dialog = Message::from(&app.last_error.message);
    dialog.kind = MessageType::Error;
    dialog.render(app, frame);
    app.state = UIState::Error;
}

#[cfg(test)]
mod keyword_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn app_at(offset: usize) -> App {
        let dir = std::env::temp_dir().join("dz6_keywords");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("k_{n}.bin"));
        std::fs::write(&path, vec![0u8; 0x200]).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app.hex_view.offset = offset;
        app
    }

    /// The cursor address keyword is `cur`.
    ///
    /// It used to be `sel`, which read as "selection" even though it has never had
    /// anything to do with the selected block - it is just where the cursor is.
    #[test]
    fn cur_expands_to_the_cursor_address() {
        let app = app_at(0x40);
        let expected = format!("0x{:X}", app.hex_view.offset);

        assert_eq!(resolve_keywords(&app, "cur"), expected);
        assert_eq!(resolve_keywords(&app, "cur+10"), format!("{}+10", expected));
        // Case-insensitive, like the other keywords.
        assert_eq!(resolve_keywords(&app, "CUR"), expected);
    }

    /// The old name must no longer expand, or a stale `.dz6init` would silently keep
    /// working and the two spellings would drift.
    #[test]
    fn sel_is_no_longer_a_keyword() {
        let app = app_at(0x40);
        assert_eq!(
            resolve_keywords(&app, "sel"),
            "sel",
            "'sel' was renamed to 'cur' and must be left untouched"
        );
    }

    /// Only whole words are replaced, so an address or command containing the letters
    /// is not mangled.
    #[test]
    fn only_whole_words_are_replaced() {
        let app = app_at(0x40);
        for text in ["current", "curl", "occur", "cursor"] {
            assert_eq!(
                resolve_keywords(&app, text),
                text,
                "'{text}' contains 'cur' but is not the keyword"
            );
        }
    }

    /// In offset mode a typed address is a file offset, not a VA.
    ///
    /// The reported case: with `.text` at RVA 0x1000 / raw 0x400, `94D+1000`
    /// evaluates to 0x194D and used to be translated as a virtual address, landing on
    /// offset 0xD4D. The arithmetic was right; the interpretation contradicted the
    /// offset the Goto box had been pre-filled with.
    #[test]
    fn offset_mode_does_not_translate_through_the_section_table() {
        let mut app = App::new();
        app.config.database = false;
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(exe) = exe.to_str() else { return };
        if app.load_file(exe, 0, true).is_err() {
            return;
        }
        let Some(pe) = app.header_view.pe.as_ref() else { return };

        // A section whose RVA and raw offset differ is what makes the two readings
        // disagree at all.
        let Some(section) = pe
            .sections
            .iter()
            .find(|s| s.virtual_address != s.pointer_to_raw_data && s.pointer_to_raw_data > 0)
        else {
            return;
        };
        // An address inside that section, expressed as a VA.
        let inside_rva = section.virtual_address as u64 + 0x20;
        let va = app.get_image_base() + inside_rva;
        let raw_offset = section.pointer_to_raw_data as u64 + 0x20;

        app.hex_view.show_va = false;
        app.editor_view = crate::editor::AppView::Hex;
        assert_eq!(
            address_to_offset(&app, inside_rva),
            Some(inside_rva as usize),
            "in offset mode the number typed is the offset itself"
        );

        app.hex_view.show_va = true;
        assert_eq!(
            address_to_offset(&app, va),
            Some(raw_offset as usize),
            "in VA mode the same box must translate through the section table"
        );
    }

    /// The mode follows what the address column shows, including the Disasm view,
    /// which is always in VA terms.
    #[test]
    fn the_disasm_view_is_always_virtual() {
        let mut app = App::new();
        app.config.database = false;
        app.hex_view.show_va = false;

        app.editor_view = crate::editor::AppView::Hex;
        assert!(!addresses_are_virtual(&app));

        app.editor_view = crate::editor::AppView::Disasm;
        assert!(
            addresses_are_virtual(&app),
            "the Disasm view labels every row with a VA, so Goto must read one too"
        );
    }

    /// A value that cannot be an offset still falls back to the other reading, so a
    /// pasted VA keeps working in offset mode.
    #[test]
    fn an_out_of_range_offset_falls_back_to_a_virtual_address() {
        let mut app = App::new();
        app.config.database = false;
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(exe) = exe.to_str() else { return };
        if app.load_file(exe, 0, true).is_err() {
            return;
        }
        if app.header_view.pe.is_none() {
            return;
        }

        app.hex_view.show_va = false;
        app.editor_view = crate::editor::AppView::Hex;

        // A full VA is far past the end of the file, so it cannot be an offset.
        let va = app.get_va(0x100);
        assert!(
            va as usize >= app.file_info.size,
            "precondition: the VA must be out of range as an offset"
        );
        assert_eq!(
            address_to_offset(&app, va),
            Some(0x100),
            "a value that cannot be an offset must still be tried as a VA"
        );
    }

    /// The other two keywords are unaffected by the rename.
    #[test]
    fn base_and_oep_still_expand() {
        let app = app_at(0x40);
        assert_eq!(resolve_keywords(&app, "base"), format!("0x{:X}", app.get_image_base()));
        assert_eq!(resolve_keywords(&app, "oep"), format!("0x{:X}", app.get_oep()));
    }
}

#[cfg(test)]
mod dz6init_tests {
    use super::*;

    /// `.dz6init` lines go through `parse_command`, so the `set enc1 <name>` /
    /// `set enc2 <name>` forms this file writes must round-trip through clap.
    fn parse_set(line: &str) -> Option<(String, Option<String>)> {
        let args = shell_words::split(line).unwrap_or_default();
        let mut argv: Vec<&str> = vec!["dz6"];
        argv.extend(args.iter().map(|s| s.as_str()));
        match CommandLine::try_parse_from(argv) {
            Ok(cli) => match cli.command {
                // `option` is optional now, since bare `:set` lists everything.
                Some(Command::Set { option, value }) => option.map(|opt| (opt, value)),
                _ => None,
            },
            Err(_) => None,
        }
    }

    #[test]
    fn set_encoding_lines_parse() {
        assert_eq!(
            parse_set("set enc1 cp949"),
            Some(("enc1".to_string(), Some("cp949".to_string())))
        );
        assert_eq!(
            parse_set("set enc2 UTF-8"),
            Some(("enc2".to_string(), Some("UTF-8".to_string())))
        );
    }

    #[test]
    fn encoding_names_written_by_save_are_accepted() {
        use crate::text::dialog_encoding::encoding_from_name;
        for name in [
            "cp949", "UTF-8", "EUC-KR", "GBK", "windows-1252", "ISO-8859-2", "UTF-16LE", "UTF-16BE",
        ] {
            assert!(encoding_from_name(name).is_some(), "rejected {:?}", name);
        }
    }
}

#[cfg(test)]
mod set_command_tests {
    use super::*;
    use crate::editor::UIState;

    fn app() -> App {
        let mut app = App::new();
        app.config.database = false;
        app
    }

    fn run(app: &mut App, line: &str) {
        parse_command(app, line);
    }

    /// `:set han` switches the IME indicator, and stays out of everything that
    /// documents the option set.
    ///
    /// Hidden on purpose. It is checked here so "hidden" keeps meaning "absent from
    /// the table, the settings dialog and the suggestions" rather than "absent
    /// until someone adds it to the list by habit".
    #[test]
    fn han_is_hidden_but_works() {
        let mut app = app();
        assert!(!app.config.show_ime, "the indicator is off by default");

        run(&mut app, "set han on");
        assert!(app.config.show_ime);
        run(&mut app, "set han off");
        assert!(!app.config.show_ime);
        run(&mut app, "set han toggle");
        assert!(app.config.show_ime);
        run(&mut app, "set han toggle");
        assert!(!app.config.show_ime);

        // Not an unknown option, so no error was raised.
        run(&mut app, "set han on");
        assert!(
            !app.last_error.message.contains("Unknown option"),
            "got: {}",
            app.last_error.message
        );

        // And invisible to every list that describes the option set.
        assert!(
            !crate::global::settings::OPTION_NAMES.contains(&"han"),
            "'han' is in the name list, so ':set' would document it and typos would suggest it"
        );
        let table = crate::global::settings::settings_text(&app);
        assert!(!table.contains("han"), "'han' shows up in the ':set' table");
    }

    /// A bad value is still reported, hidden or not.
    #[test]
    fn han_rejects_junk() {
        let mut app = app();
        run(&mut app, "set han maybe");
        assert!(
            app.last_error.message.contains("han"),
            "got: {}",
            app.last_error.message
        );
        assert!(!app.config.show_ime);
    }

    /// `bg` and `fg` change the main style, and every theme-file key works too.
    #[test]
    fn colours_can_be_set_directly() {
        use ratatui::style::Color;

        let mut app = app();
        run(&mut app, "set bg #2B3339");
        assert_eq!(app.config.theme.main.bg, Some(Color::Rgb(0x2B, 0x33, 0x39)));
        run(&mut app, "set fg #D3C6AA");
        assert_eq!(app.config.theme.main.fg, Some(Color::Rgb(0xD3, 0xC6, 0xAA)));

        // The full key names, one per style, exactly as a theme file spells them.
        for (option, hex) in [
            ("offsets_fg", "#859289"),
            ("dimmed_fg", "#4A555B"),
            ("dialog_bg", "#343F44"),
            ("changed_bytes_fg", "#DBBC7F"),
            ("highlight_bg", "#A7C080"),
            ("byte_highlight_bg", "#83C092"),
            ("topbar_bg", "#3A464C"),
            ("error_fg", "#E67E80"),
            ("editing_fg", "#2B3339"),
        ] {
            run(&mut app, &format!("set {} {}", option, hex));
        }
        assert_eq!(app.config.theme.offsets.fg, Some(Color::Rgb(0x85, 0x92, 0x89)));
        assert_eq!(app.config.theme.dimmed.fg, Some(Color::Rgb(0x4A, 0x55, 0x5B)));
        assert_eq!(app.config.theme.dialog.bg, Some(Color::Rgb(0x34, 0x3F, 0x44)));
        assert_eq!(app.config.theme.changed_bytes.fg, Some(Color::Rgb(0xDB, 0xBC, 0x7F)));
        assert_eq!(app.config.theme.highlight.bg, Some(Color::Rgb(0xA7, 0xC0, 0x80)));
        assert_eq!(app.config.theme.byte_highlight.bg, Some(Color::Rgb(0x83, 0xC0, 0x92)));
        assert_eq!(app.config.theme.topbar.bg, Some(Color::Rgb(0x3A, 0x46, 0x4C)));
        assert_eq!(app.config.theme.error.fg, Some(Color::Rgb(0xE6, 0x7E, 0x80)));
        assert_eq!(app.config.theme.editing.fg, Some(Color::Rgb(0x2B, 0x33, 0x39)));
    }

    /// Hex with or without a prefix, and the named colours the disassembly options
    /// already accept.
    #[test]
    fn colour_values_take_several_spellings() {
        use ratatui::style::Color;

        let mut app = app();
        run(&mut app, "set bg 2B3339");
        assert_eq!(app.config.theme.main.bg, Some(Color::Rgb(0x2B, 0x33, 0x39)));
        run(&mut app, "set bg 0x1E1E1E");
        assert_eq!(app.config.theme.main.bg, Some(Color::Rgb(0x1E, 0x1E, 0x1E)));
        run(&mut app, "set fg red");
        assert_eq!(app.config.theme.main.fg, Some(Color::Red));
    }

    /// A junk value is refused and changes nothing.
    #[test]
    fn a_bad_colour_is_refused() {
        let mut app = app();
        let before = app.config.theme.main.bg;

        run(&mut app, "set bg #GGGGGG");
        assert_eq!(app.config.theme.main.bg, before, "a bad colour was applied");
        assert!(!app.last_error.message.is_empty());

        run(&mut app, "set bg");
        assert!(
            app.last_error.message.contains("colour"),
            "got: {}",
            app.last_error.message
        );
    }

    /// The colour options stay out of everything that documents the option set.
    #[test]
    fn colours_are_hidden() {
        let app = app();
        for name in ["bg", "fg", "main_bg", "main_fg", "highlight_bg", "editing_fg"] {
            assert!(
                !crate::global::settings::OPTION_NAMES.contains(&name),
                "'{}' is in the name list",
                name
            );
        }
        let table = crate::global::settings::settings_text(&app);
        for name in ["main_bg", "highlight_bg", "editing_fg"] {
            assert!(!table.contains(name), "'{}' shows up in the ':set' table", name);
        }
        for text in [
            crate::hex::help::HELP_EN,
            crate::hex::help::HELP_KO,
            crate::hex::help::HELP_ZH,
        ] {
            assert!(!text.contains("main_bg"), "the help documents the colour keys");
        }
    }

    /// A misspelled option must say so, and point at the right name.
    ///
    /// Every unrecognised option used to fall through the final `_ =>` arm and do
    /// nothing, which is indistinguishable from having worked.
    #[test]
    fn an_unknown_option_is_reported() {
        let mut app = app();
        run(&mut app, "set bytelin 32");

        assert!(
            app.last_error.message.contains("Unknown option"),
            "got: {}",
            app.last_error.message
        );
        assert!(
            app.last_error.message.contains("byteline"),
            "the suggestion is the point, got: {}",
            app.last_error.message
        );
    }

    /// A value the option cannot take is an error too, not a silent no-op.
    #[test]
    fn bad_values_are_reported() {
        let mut app = app();
        let before = app.config.hex_mode_bytes_per_line;

        run(&mut app, "set byteline abc");
        assert!(app.last_error.message.contains("not a byte count"));
        assert_eq!(app.config.hex_mode_bytes_per_line, before, "nothing changed");

        run(&mut app, "set hintbar maybe");
        assert!(
            app.last_error.message.contains("on, off or toggle"),
            "got: {}",
            app.last_error.message
        );

        run(&mut app, "set view sideways");
        assert!(app.last_error.message.contains("hex, disasm, text or header"));

        run(&mut app, "set ctrlchar abc");
        assert!(app.last_error.message.contains("one character"));

        run(&mut app, "set disasm_mem notacolour");
        assert!(app.last_error.message.contains("not a colour"));
    }

    /// on / off / toggle, and the bare form that has to keep meaning "on" because
    /// existing `.dz6init` files use it.
    #[test]
    fn switches_take_on_off_and_toggle() {
        let mut app = app();

        run(&mut app, "set hintbar off");
        assert!(!app.config.hint_bar);
        run(&mut app, "set hintbar on");
        assert!(app.config.hint_bar);
        run(&mut app, "set hintbar toggle");
        assert!(!app.config.hint_bar);
        run(&mut app, "set hintbar");
        assert!(app.config.hint_bar, "a bare switch still means on");

        // The old spellings keep working.
        run(&mut app, "set nohintbar");
        assert!(!app.config.hint_bar);
        run(&mut app, "set nowrapscan");
        assert!(!app.config.search_wrap);
        run(&mut app, "set wrapscan");
        assert!(app.config.search_wrap);
    }

    /// The two dim options are independent now.
    ///
    /// Setting one used to clear the other, and the only way off was `nodim`, which
    /// killed both - so "dim control bytes but not nulls" could not be expressed.
    #[test]
    fn dim_options_are_independent() {
        let mut app = app();

        run(&mut app, "set dimctrl on");
        run(&mut app, "set dimzero on");
        assert!(app.config.dim_control_chars && app.config.dim_zeroes);

        run(&mut app, "set dimzero off");
        assert!(app.config.dim_control_chars, "dimctrl must survive");
        assert!(!app.config.dim_zeroes);

        run(&mut app, "set nodim");
        assert!(!app.config.dim_control_chars && !app.config.dim_zeroes);
    }

    /// One option for the address column, with the two old ones as aliases.
    #[test]
    fn addr_replaces_va_and_offset() {
        let mut app = app();

        run(&mut app, "set addr va");
        assert!(app.hex_view.show_va);
        run(&mut app, "set addr offset");
        assert!(!app.hex_view.show_va);
        run(&mut app, "set addr toggle");
        assert!(app.hex_view.show_va);

        run(&mut app, "set offset");
        assert!(!app.hex_view.show_va);
        run(&mut app, "set va");
        assert!(app.hex_view.show_va);
    }

    /// `:set view disasm` exists at all - it was the one view the command could not
    /// select - and it refuses a file with no code rather than doing nothing.
    #[test]
    fn view_accepts_disasm() {
        let mut app = app();
        run(&mut app, "set view text");
        assert!(app.editor_view == crate::editor::AppView::Text);

        run(&mut app, "set view disasm");
        assert!(
            app.last_error.message.contains("no code section"),
            "an empty App has nothing to disassemble, got: {}",
            app.last_error.message
        );
        assert!(app.editor_view == crate::editor::AppView::Text, "the view must not change");

        run(&mut app, "set view hex");
        assert!(app.editor_view == crate::editor::AppView::Hex);
    }

    /// The interface language, with the spellings people actually type.
    #[test]
    fn lang_switches_the_interface() {
        use crate::i18n::{Lang, M};
        let mut app = app();
        assert_eq!(app.config.lang, Lang::En);

        run(&mut app, "set lang ko");
        assert_eq!(app.config.lang, Lang::Ko);
        assert_eq!(M::Help.tr(app.config.lang), "도움말");

        run(&mut app, "set lang chinese");
        assert_eq!(app.config.lang, Lang::Zh);
        assert_eq!(M::Help.tr(app.config.lang), "帮助");

        run(&mut app, "set lang en");
        assert_eq!(app.config.lang, Lang::En);

        run(&mut app, "set lang klingon");
        assert!(
            app.last_error.message.contains("Unknown language"),
            "got: {}",
            app.last_error.message
        );
        assert_eq!(app.config.lang, Lang::En, "a bad name must not change it");
    }

    /// The settings table follows the language, and lists `lang` itself.
    #[test]
    fn the_settings_table_is_translated() {
        let mut app = app();
        run(&mut app, "set lang ko");

        let text = crate::global::settings::settings_text(&app);
        assert!(text.contains("lang"), "the option must list itself");
        assert!(text.contains("한국어"), "the value shows the language's own name");
        assert!(
            text.contains("하단 힌트 줄"),
            "the notes column must be translated, got:\n{}",
            text
        );
        // Option names are identifiers shared with the documentation and must not
        // move with the language.
        assert!(text.contains("byteline") && text.contains("hintbar"));
    }

    /// Bare `:set` opens the table.
    #[test]
    fn bare_set_shows_the_table() {
        let mut app = app();
        run(&mut app, "set");

        assert!(app.state == UIState::DialogSettings);
        assert!(app.dialog_renderer.is_some());
    }

    /// A `.dz6init` line must not leave a dialog over the first frame.
    #[test]
    fn bare_set_in_the_init_file_logs_instead() {
        let mut app = app();
        app.loading_initfile = true;
        run(&mut app, "set");

        assert!(app.state != UIState::DialogSettings);
        assert!(
            app.logs.iter().any(|line| line.contains("byteline")),
            "the table should have gone to the log"
        );
    }
}

#[cfg(test)]
mod localized_message_tests {
    use super::*;
    use crate::i18n::Lang;

    fn app_in(lang: Lang) -> App {
        let mut app = App::new();
        app.config.database = false;
        app.config.lang = lang;
        app
    }

    /// `:set` errors follow the interface language, and still name the option and
    /// the value the user typed - those are identifiers, not prose.
    #[test]
    fn set_errors_are_localized() {
        let mut app = app_in(Lang::Ko);
        parse_command(&mut app, "set hintbar maybe");
        assert!(
            app.last_error.message.contains("입력값") && app.last_error.message.contains("maybe"),
            "got: {}",
            app.last_error.message
        );

        let mut app = app_in(Lang::Zh);
        parse_command(&mut app, "set bytelin 32");
        assert!(
            app.last_error.message.contains("未知选项") && app.last_error.message.contains("byteline"),
            "got: {}",
            app.last_error.message
        );

        let mut app = app_in(Lang::En);
        parse_command(&mut app, "set view sideways");
        assert!(app.last_error.message.starts_with("':set view'"));
    }

    /// A read-only refusal is translated on both halves - the prefix and the action.
    #[test]
    fn read_only_refusals_are_localized() {
        let mut app = app_in(Lang::Ko);
        app.read_only_error(crate::i18n::M::RoEditData);
        let message = app.status_error.clone().expect("a message");
        assert!(
            message.contains("읽기 전용") && message.contains("데이터 편집"),
            "got: {}",
            message
        );

        let mut app = app_in(Lang::Zh);
        app.read_only_error(crate::i18n::M::RoPaste);
        let message = app.status_error.clone().expect("a message");
        assert!(message.contains("只读") && message.contains("粘贴"), "got: {}", message);
    }
}

#[cfg(test)]
mod option_name_tests {
    /// Every name in `OPTION_NAMES` must be an option `:set` actually handles.
    ///
    /// The list drives the `:set` table and the typo suggestions, so a name in it
    /// that nothing handles sends the user to a dead option - and the suggestion
    /// machinery would confidently point at it.
    #[test]
    fn every_listed_option_is_handled() {
        for name in crate::global::settings::OPTION_NAMES {
            let mut app = crate::app::App::new();
            app.config.database = false;
            // The value is deliberately plausible-but-generic: what is being checked
            // is only whether the *name* is recognised.
            crate::commands::parse_command(&mut app, &format!("set {} on", name));
            assert!(
                !app.last_error.message.contains("Unknown option"),
                "':set {}' is listed but not handled: {}",
                name,
                app.last_error.message
            );
        }
    }
}