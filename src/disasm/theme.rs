use ratatui::style::Color;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct DisasmTheme {
    pub name: String,
    pub call_bg: Color,
    pub call_fg: Color,
    pub jmp_bg: Color,
    pub jmp_fg: Color,
    pub jcc_bg: Color,
    pub jcc_fg: Color,
    pub push_pop_fg: Color,
    pub ret_bg: Color,
    pub ret_fg: Color,
    pub register_fg: Color,
    pub memory_op_fg: Color,
    pub immediate_fg: Color,
    pub keyword_fg: Color,
    pub comment_fg: Color,
    /// Segment prefix (`ds:`, `ss:`, `fs:`).
    ///
    /// Its own colour rather than the memory-operand one: the prefix says *which
    /// address space* the access goes through, which is a different fact from the
    /// address itself, and x64dbg colours it separately for that reason.
    pub segment_fg: Color,
    /// Background behind an inlined import name (`<CreateFileW>`).
    pub import_bg: Color,
    /// Foreground for an inlined import name.
    pub import_fg: Color,
}

impl Default for DisasmTheme {
    /// The colours a fresh install starts with, before any `disasm.theme` exists.
    ///
    /// Also the base every theme file is applied *onto*, so a file that omits a key
    /// - an older one written before the key existed - inherits the value here
    /// rather than something arbitrary.
    fn default() -> Self {
        Self {
            name: "dark".to_string(),
        
            call_bg: Color::Rgb(0, 255, 255),        // #00FFFF
            call_fg: Color::Rgb(0, 0, 0),            // #000000
            jmp_bg: Color::Rgb(255, 255, 0),        // #FFFF00
            jmp_fg: Color::Rgb(0, 0, 0),            // #000000
            jcc_bg: Color::Rgb(255, 255, 0),        // #FFFF00
            jcc_fg: Color::Rgb(255, 0, 0),          // #FF0000
            push_pop_fg: Color::Rgb(0, 0, 255),      // #0000FF
            ret_bg: Color::Rgb(0, 255, 255),         // #00FFFF
            ret_fg: Color::Rgb(0, 0, 0),            // #000000
            register_fg: Color::Rgb(0, 0x83, 0),     // #008300
            memory_op_fg: Color::Rgb(0, 0, 0x80),    // #000080
            immediate_fg: Color::Rgb(128, 128, 0),   // #808000
            keyword_fg: Color::Rgb(180, 0, 180),     // #B400B4
            comment_fg: Color::Rgb(0, 128, 128),     // #008080
            segment_fg: Color::Rgb(255, 0, 255),     // #FF00FF
            import_bg: Color::Rgb(255, 255, 0),      // #FFFF00
            import_fg: Color::Rgb(0, 0, 0),          // #000000
        }
    }
}

pub fn parse_color_str(s: &str) -> Option<Color> {
    let clean = s.trim();
    if let Some(digits) = clean.strip_prefix('#') {
        // `len() == 7` was a *byte* length check, so a 7-byte multi-byte string
        // passed it and then panicked slicing on a non-char boundary.
        if digits.len() == 6 && digits.is_ascii() {
            let r = u8::from_str_radix(&digits[0..2], 16).ok()?;
            let g = u8::from_str_radix(&digits[2..4], 16).ok()?;
            let b = u8::from_str_radix(&digits[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    } else if clean.contains(',') {
        let parts: Vec<&str> = clean.split(',').collect();
        if parts.len() == 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    } else {
        match clean.to_lowercase().as_str() {
            "black" => return Some(Color::Black),
            "red" => return Some(Color::Red),
            "green" => return Some(Color::Green),
            "yellow" => return Some(Color::Yellow),
            "blue" => return Some(Color::Blue),
            "magenta" => return Some(Color::Magenta),
            "cyan" => return Some(Color::Cyan),
            "gray" | "grey" => return Some(Color::Gray),
            "darkgray" | "darkgrey" => return Some(Color::DarkGray),
            "lightred" => return Some(Color::LightRed),
            "lightgreen" => return Some(Color::LightGreen),
            "lightyellow" => return Some(Color::LightYellow),
            "lightblue" => return Some(Color::LightBlue),
            "lightmagenta" => return Some(Color::LightMagenta),
            "lightcyan" => return Some(Color::LightCyan),
            "white" => return Some(Color::White),
            _ => {}
        }
    }
    None
}

pub fn color_to_hex_str(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{:02X}{:02X}{:02X}", r, g, b),
        Color::Black => "#000000".into(),
        Color::Red => "#FF0000".into(),
        Color::Green => "#00FF00".into(),
        Color::Yellow => "#FFFF00".into(),
        Color::Blue => "#0000FF".into(),
        Color::Magenta => "#FF00FF".into(),
        Color::Cyan => "#00FFFF".into(),
        Color::Gray => "#808080".into(),
        Color::DarkGray => "#555555".into(),
        Color::LightRed => "#FF5555".into(),
        Color::LightGreen => "#55FF55".into(),
        Color::LightYellow => "#FFFF55".into(),
        Color::LightBlue => "#5555FF".into(),
        Color::LightMagenta => "#FF55FF".into(),
        Color::LightCyan => "#55FFFF".into(),
        Color::White => "#FFFFFF".into(),
        _ => "#FFFFFF".into(),
    }
}

/// Name of the disassembly colour config, looked up next to the executable first
/// and then in the startup directory.
///
/// Contents use the same `key = #RRGGBB` format as the regular theme files in
/// `themes/` (see `themes.rs`), hence the `.theme` extension rather than the
/// old misleading `.json` one.
const DISASM_THEME_FILE: &str = "disasm.theme";

/// Previous name of the same file. Still read (once, as a fallback) so an
/// existing install doesn't silently lose its colours on upgrade; the next
/// save writes the new name.
const LEGACY_DISASM_THEME_FILE: &str = "disasm_theme.json";

/// Field name -> colour, in the order they are written to the file.
/// Single source of truth for both the loader and the saver, so a field can't
/// be saved without being loadable (the old code hand-wrote both lists).
macro_rules! disasm_theme_fields {
    ($theme:ident, $callback:ident) => {
        $callback!($theme, call_bg);
        $callback!($theme, call_fg);
        $callback!($theme, jmp_bg);
        $callback!($theme, jmp_fg);
        $callback!($theme, jcc_bg);
        $callback!($theme, jcc_fg);
        $callback!($theme, push_pop_fg);
        $callback!($theme, ret_bg);
        $callback!($theme, ret_fg);
        $callback!($theme, register_fg);
        $callback!($theme, memory_op_fg);
        $callback!($theme, immediate_fg);
        $callback!($theme, keyword_fg);
        $callback!($theme, comment_fg);
        $callback!($theme, segment_fg);
        $callback!($theme, import_bg);
        $callback!($theme, import_fg);
    };
}

/// Directories the config is searched in, in priority order: the install's
/// `themes/` folder, then the executable's own directory, then the startup
/// directory.
///
/// `themes/` comes first and is the only one ever written to - the file belongs
/// with the other `.theme` files rather than loose beside the binary. The two
/// plainer directories stay as read-only fallbacks so an install that already
/// has a `disasm.theme` next to the exe keeps its colours; the next save moves
/// it into `themes/`.
///
/// The startup directory rather than the live CWD, because the file dialog calls
/// `set_current_dir` as the user navigates.
fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(3);
    let exe = crate::util::exe_dir().to_path_buf();
    dirs.push(exe.join("themes"));
    dirs.push(exe.clone());
    let startup = crate::util::startup_dir().to_path_buf();
    if startup != exe {
        dirs.push(startup);
    }
    dirs
}

/// Where the config is written: the install's `themes/` folder, so saving can't
/// scatter `disasm.theme` copies through the directories the user browses.
/// Always the current file name, never the legacy one.
pub fn get_theme_config_path() -> PathBuf {
    crate::util::exe_dir().join("themes").join(DISASM_THEME_FILE)
}

/// Existing config to read, preferring the current name and falling back to
/// the legacy `disasm_theme.json` in either search directory.
fn find_existing_theme_file() -> Option<PathBuf> {
    let dirs = theme_search_dirs();
    for name in [DISASM_THEME_FILE, LEGACY_DISASM_THEME_FILE] {
        for dir in &dirs {
            let path = dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Applies `key = #RRGGBB` lines onto an existing theme.
///
/// Split out of `load_disasm_theme` so the named presets in `themes/disasm/`
/// go through exactly the same parser as `disasm.theme` - otherwise the two
/// paths could drift on which keys or colour spellings they accept.
fn apply_theme_text(theme: &mut DisasmTheme, data: &str) {
    for line in data.lines() {
        let trimmed = line.trim();
        // Skip comments, blanks, and the stray braces a leftover JSON file
        // still has.
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with('{')
            || trimmed.starts_with('}')
        {
            continue;
        }

        // `key = #RRGGBB` is the current format. `"key": "#RRGGBB",` is the
        // old JSON one, still accepted so an existing config keeps working.
        let (key, val) = match trimmed.split_once('=') {
            Some((k, v)) => (k, v),
            None => match trimmed.trim_end_matches(',').split_once(':') {
                Some((k, v)) => (k, v),
                None => continue,
            },
        };

        let key = key.trim().trim_matches('"');
        let val = val.trim().trim_end_matches(',').trim().trim_matches('"');

        // `name = ...` is metadata in the preset files, not a colour.
        if key == "name" {
            theme.name = val.to_string();
            continue;
        }

        let Some(col) = parse_color_str(val) else {
            continue;
        };

        macro_rules! match_field {
            ($t:ident, $field:ident) => {
                if key == stringify!($field) {
                    $t.$field = col;
                    continue;
                }
            };
        }
        disasm_theme_fields!(theme, match_field);
    }
}

pub fn load_disasm_theme() -> DisasmTheme {
    let mut theme = DisasmTheme::default();

    let Some(path) = find_existing_theme_file() else {
        save_disasm_theme(&theme);
        return theme;
    };

    // Anything not already at the canonical path gets migrated: the old JSON name,
    // and a `disasm.theme` sitting beside the exe from before the file moved into
    // `themes/`. Parse it first, then write the result where it now belongs so the
    // next run reads that. The old copy is left in place rather than deleted - it is
    // the user's file, and the search order already prefers the new location.
    let needs_migration = path != get_theme_config_path();

    if let Ok(data) = fs::read_to_string(&path) {
        apply_theme_text(&mut theme, &data);
    }

    if needs_migration {
        save_disasm_theme(&theme);
    }

    theme
}

pub fn save_disasm_theme(theme: &DisasmTheme) {
    let path = get_theme_config_path();

    // The target is inside `themes/`, which may not exist yet on a fresh install -
    // `ensure_and_load_themes` creates it, but nothing guarantees it has run first,
    // and a missing directory makes `fs::write` fail silently here.
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut content = String::new();
    content.push_str("# Dezes Disassembly Theme File\n");
    content.push_str("# Format: <key> = #RRGGBB  (same as the theme files in themes/)\n\n");

    macro_rules! write_field {
        ($t:ident, $field:ident) => {
            content.push_str(&format!(
                "{} = {}\n",
                stringify!($field),
                color_to_hex_str($t.$field)
            ));
        };
    }
    disasm_theme_fields!(theme, write_field);

    let _ = fs::write(path, content);
}

/// Legacy location of the named disassembly presets: `<exe_dir>/themes/disasm/`.
///
/// Disassembly colours now live in the same file as the main theme
/// (`themes/<name>.theme`), which is one directory level instead of two. This
/// path is still searched so an install that already has files here keeps
/// working; nothing is written to it any more.
///
/// Mixing the two key sets in one file is safe because they don't overlap -
/// `main_fg`/`offsets_bg` versus `call_bg`/`jcc_fg` - and both parsers ignore
/// keys they don't recognise.
fn legacy_disasm_theme_dir(base: &std::path::Path) -> PathBuf {
    base.join("themes").join("disasm")
}

/// Built-in preset names. `gray` is accepted as an alias of `grey` so these line
/// up with the main theme files, which are named `gray.theme`.
pub const DISASM_PRESETS: [&str; 3] = ["dark", "light", "grey"];

/// Built-in definition for a preset name, used both to write the initial files
/// and as the fallback when the file has been deleted.
///
/// `grey` and `gray` are accepted for the same preset, matching how
/// `parse_color_str` and the main `:set theme` already treat that spelling.
pub fn disasm_preset(name: &str) -> Option<DisasmTheme> {
    // Note on which fields actually reach the screen: `disasm/draw.rs` reads
    // call_bg, call_fg, jmp_bg, jcc_fg, push_pop_fg, register_fg, keyword_fg,
    // immediate_fg, comment_fg and memory_op_fg. jmp_fg, jcc_bg, ret_bg and
    // ret_fg are currently unused there (RET reuses the CALL colours), so they
    // are set consistently here rather than left at odds with the rest.
    let theme = match name.trim().to_lowercase().as_str() {
        // Tuned for the near-black #1E1E1E hex background. The opcode blocks
        // stay light with dark text, which is what keeps CALL/JMP scannable
        // without turning into glare on a dark terminal.
        // The classic saturated x64dbg-style scheme, which is also
        // `DisasmTheme::default()`. `.dz6init`'s `set theme dark` runs on every
        // launch and re-applies this, so it is what the disassembly view looks like
        // out of the box; keeping the two in step is what makes a hand-edited
        // `disasm.theme` and a fresh install agree.
        //
        // An earlier version desaturated these for the near-black #1E1E1E hex
        // background. That is still available as `grey`/`light`, but the bright
        // scheme is what CALL/JMP blocks are recognisable as.
        "dark" => DisasmTheme::default(),
        // For the #EEEEEE background. Foregrounds are darkened rather than
        // brightened, since on a light background it is the text that has to
        // carry the contrast.
        "light" => DisasmTheme {
            name: "light".to_string(),
            call_bg: Color::Rgb(0x66, 0xE0, 0xEA),
            call_fg: Color::Rgb(0x00, 0x00, 0x00),
            jmp_bg: Color::Rgb(0xF5, 0xD6, 0x4A),
            jmp_fg: Color::Rgb(0x00, 0x00, 0x00),
            jcc_bg: Color::Rgb(0xF5, 0xD6, 0x4A),
            jcc_fg: Color::Rgb(0xC0, 0x00, 0x2B),
            push_pop_fg: Color::Rgb(0x0A, 0x47, 0xA9),
            ret_bg: Color::Rgb(0x66, 0xE0, 0xEA),
            ret_fg: Color::Rgb(0x00, 0x00, 0x00),
            register_fg: Color::Rgb(0x06, 0x7A, 0x21),
            memory_op_fg: Color::Rgb(0x1F, 0x5F, 0xD0),
            immediate_fg: Color::Rgb(0x7A, 0x60, 0x00),
            keyword_fg: Color::Rgb(0x8B, 0x00, 0x8B),
            comment_fg: Color::Rgb(0x00, 0x7A, 0x7A),
            segment_fg: Color::Rgb(0xB0, 0x00, 0xB0),
            import_bg: Color::Rgb(0xF5, 0xD6, 0x4A),
            import_fg: Color::Rgb(0x00, 0x00, 0x00),
        },
        // For the mid #505050 background, which is the hardest case: neither
        // very dark nor very light foregrounds separate from it on their own,
        // so these are pushed toward pastels.
        "grey" | "gray" => DisasmTheme {
            name: "grey".to_string(),
            call_bg: Color::Rgb(0x7F, 0xD4, 0xDE),
            call_fg: Color::Rgb(0x14, 0x18, 0x1A),
            jmp_bg: Color::Rgb(0xE3, 0xCE, 0x63),
            jmp_fg: Color::Rgb(0x14, 0x18, 0x1A),
            jcc_bg: Color::Rgb(0xE3, 0xCE, 0x63),
            jcc_fg: Color::Rgb(0x8E, 0x16, 0x16),
            push_pop_fg: Color::Rgb(0xA8, 0xC8, 0xFF),
            ret_bg: Color::Rgb(0x7F, 0xD4, 0xDE),
            ret_fg: Color::Rgb(0x14, 0x18, 0x1A),
            register_fg: Color::Rgb(0x9B, 0xE0, 0xA5),
            memory_op_fg: Color::Rgb(0xA9, 0xCF, 0xFF),
            immediate_fg: Color::Rgb(0xEB, 0xD9, 0xA0),
            keyword_fg: Color::Rgb(0xE0, 0xA6, 0xF0),
            comment_fg: Color::Rgb(0xB8, 0xD8, 0xB8),
            segment_fg: Color::Rgb(0xF0, 0x9E, 0xF0),
            import_bg: Color::Rgb(0xE3, 0xCE, 0x63),
            import_fg: Color::Rgb(0x14, 0x18, 0x1A),
        },
        _ => return None,
    };
    Some(theme)
}

/// The disassembly half of a combined theme file.
///
/// Nothing writes combined files any more - the disassembly colours live in
/// `themes/disasm.theme` alone - but the format is still *read*, so this is kept
/// as the single description of those keys that the round-trip test checks the
/// parser against.
#[allow(dead_code)]
pub fn disasm_section_text(theme: &DisasmTheme) -> String {
    let mut content = String::from(
        "\n# --- Disassembly view colours ---\n\
         # Read by ':set theme <name>' along with the colours above, and by\n\
         # ':set disasmtheme <name>' on its own.\n\
         #\n\
         # Not every key reaches the screen yet: jmp_fg, jcc_bg, ret_bg and\n\
         # ret_fg are unused by the current renderer (RET reuses the CALL\n\
         # colours). They are kept so the file stays a complete record.\n\n",
    );

    macro_rules! write_field {
        ($t:ident, $field:ident) => {
            content.push_str(&format!(
                "{} = {}\n",
                stringify!($field),
                color_to_hex_str($t.$field)
            ));
        };
    }
    disasm_theme_fields!(theme, write_field);

    content
}

/// True when `data` carries at least one disassembly colour key.
///
/// This is what lets a main theme file that predates the merge be told apart
/// from one that includes disassembly colours: the old files simply have none of
/// these keys, and the caller then falls back to a built-in preset instead of
/// resetting the view to the compiled-in defaults.
pub fn has_disasm_keys(data: &str) -> bool {
    let mut probe = DisasmTheme::default();
    // Flip every field to a colour no preset uses, then see whether parsing put
    // anything back. Cheaper to reason about than duplicating the key list.
    let sentinel = Color::Rgb(1, 2, 3);
    macro_rules! set_sentinel {
        ($t:ident, $field:ident) => {
            $t.$field = sentinel;
        };
    }
    disasm_theme_fields!(probe, set_sentinel);

    apply_theme_text(&mut probe, data);

    let mut changed = false;
    macro_rules! check_field {
        ($t:ident, $field:ident) => {
            if $t.$field != sentinel {
                changed = true;
            }
        };
    }
    disasm_theme_fields!(probe, check_field);
    changed
}

/// Files a `<name>` argument could refer to, in priority order.
///
/// `themes/<name>.theme` is the combined file - one directory level. The old
/// `themes/disasm/<name>.theme` is kept last so existing installs still resolve.
fn disasm_theme_candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let clean = name.trim();
    if clean.is_empty() {
        return out;
    }

    // An explicit path the user typed out wins over any preset of the same name.
    let as_given = PathBuf::from(clean);
    if as_given.is_file() {
        out.push(as_given);
    }

    let file_name = if clean.ends_with(".theme") {
        clean.to_string()
    } else {
        format!("{}.theme", clean)
    };

    for base in theme_search_dirs() {
        out.push(base.join("themes").join(&file_name));
        out.push(base.join(&file_name));
        out.push(legacy_disasm_theme_dir(&base).join(&file_name));
    }
    out
}

/// Resolves a `<name>` to the file its disassembly colours would come from.
pub fn find_disasm_theme_path(name: &str) -> Option<PathBuf> {
    disasm_theme_candidates(name).into_iter().find(|p| {
        p.is_file() && fs::read_to_string(p).map(|d| has_disasm_keys(&d)).unwrap_or(false)
    })
}

/// Disassembly colours declared by a theme *file* of that name, if any.
///
/// Deliberately does not fall back to the built-in preset. `:set theme <name>` uses
/// this, so changing the hex-view colours leaves the disassembly colours alone
/// unless the theme file explicitly says otherwise. Falling back to the preset made
/// `:set theme grey` silently replace whatever the user had set up with
/// `:set disasmtheme`, and - because `.dz6init` runs `set theme` on every launch -
/// re-applied it at every start.
pub fn disasm_theme_from_file(name: &str) -> Option<DisasmTheme> {
    let path = find_disasm_theme_path(name)?;
    let data = fs::read_to_string(&path).ok()?;
    // Start from the matching preset when there is one, so a file that lists only a
    // few keys keeps sensible values for the rest.
    let mut theme = disasm_preset(name).unwrap_or_default();
    theme.name = name.to_string();
    apply_theme_text(&mut theme, &data);
    Some(theme)
}

/// Disassembly colours for a theme name, for when the user asked for them
/// explicitly (`:set disasmtheme <name>`).
///
/// Order of preference: a file that carries disassembly keys, then the built-in
/// preset of that name. `None` means the name is not a preset and no such file
/// exists, which the caller reports as an unknown name.
pub fn resolve_disasm_theme(name: &str) -> Option<DisasmTheme> {
    disasm_theme_from_file(name).or_else(|| disasm_preset(name))
}

#[cfg(test)]
mod disasm_theme_tests {
    use super::*;

    #[test]
    fn every_listed_preset_has_a_definition() {
        for name in DISASM_PRESETS {
            assert!(
                disasm_preset(name).is_some(),
                "'{}' is listed in DISASM_PRESETS but has no definition, so \
                 ':set disasmtheme {}' would report it as unknown",
                name,
                name
            );
        }
    }

    /// A preset name alone must not hand back disassembly colours to `:set theme`.
    ///
    /// This is what made `:set theme grey` replace the colours the user had chosen
    /// with `:set disasmtheme`: the name matched a built-in preset, so the fallback
    /// fired even though no theme file said anything about the disassembly view.
    #[test]
    fn a_preset_name_alone_does_not_change_the_disasm_colours() {
        for name in DISASM_PRESETS {
            // Only true while no `themes/<name>.theme` on this machine carries
            // disassembly keys, which is the layout the app now writes.
            if find_disasm_theme_path(name).is_some() {
                continue;
            }
            assert!(
                disasm_theme_from_file(name).is_none(),
                "':set theme {name}' must leave the disassembly colours alone"
            );
            // Asking for them explicitly still works.
            assert!(
                resolve_disasm_theme(name).is_some(),
                "':set disasmtheme {name}' must still resolve"
            );
        }
    }

    /// The config lives with the other `.theme` files, not loose beside the binary.
    #[test]
    fn the_config_is_written_into_the_themes_folder() {
        let path = get_theme_config_path();

        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(DISASM_THEME_FILE)
        );
        assert_eq!(
            path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
            Some("themes"),
            "expected <exe_dir>/themes/{}, got {}",
            DISASM_THEME_FILE,
            path.display()
        );
    }

    /// `themes/` must be searched before the plainer fallbacks, or a stale copy left
    /// beside the exe by an older build would keep winning after the migration.
    #[test]
    fn the_themes_folder_is_searched_first() {
        let dirs = theme_search_dirs();
        assert!(!dirs.is_empty());
        assert_eq!(
            dirs[0].file_name().and_then(|n| n.to_str()),
            Some("themes"),
            "first search directory was {}",
            dirs[0].display()
        );
        assert!(
            dirs.len() > 1,
            "the older locations must stay readable so an existing install keeps its colours"
        );
    }

    /// The default the file is generated from is the `dark` preset, so a fresh
    /// install and `:set theme dark` cannot disagree.
    #[test]
    fn the_dark_preset_is_the_default() {
        let dark = disasm_preset("dark").expect("dark preset");
        let default = DisasmTheme::default();

        macro_rules! same {
            ($t:ident, $field:ident) => {
                assert_eq!(
                    color_to_hex_str(dark.$field),
                    color_to_hex_str(default.$field),
                    concat!(stringify!($field), " differs between the dark preset and the default")
                );
            };
        }
        disasm_theme_fields!(dark, same);
    }

    /// The memory-operand colour is the navy the user asked for.
    #[test]
    fn the_memory_operand_colour_is_navy() {
        assert_eq!(
            color_to_hex_str(DisasmTheme::default().memory_op_fg),
            "#000080"
        );
    }

    #[test]
    fn unknown_name_is_rejected() {
        assert!(disasm_preset("nope").is_none());
        // No preset and no file with disassembly keys: the caller must be told
        // to leave the current colours alone rather than reset them.
        assert!(resolve_disasm_theme("definitely-not-a-theme-xyz").is_none());
    }

    #[test]
    fn gray_and_gray_spellings_agree() {
        let grey = disasm_preset("grey").expect("grey preset");
        let gray = disasm_preset("gray").expect("gray alias");
        assert_eq!(
            color_to_hex_str(grey.call_bg),
            color_to_hex_str(gray.call_bg)
        );
    }

    /// A preset must survive being written out and read back, otherwise editing
    /// a preset file by hand would silently lose colours.
    #[test]
    fn preset_survives_a_write_read_round_trip() {
        for name in DISASM_PRESETS {
            let original = disasm_preset(name).expect("preset");
            let text = disasm_section_text(&original);
            // Start from a deliberately different base so any key the writer
            // forgot shows up as a mismatch rather than passing by luck.
            let mut parsed = DisasmTheme::default();
            apply_theme_text(&mut parsed, &text);

            macro_rules! assert_field {
                ($t:ident, $field:ident) => {
                    assert_eq!(
                        color_to_hex_str(original.$field),
                        color_to_hex_str(parsed.$field),
                        "{} preset lost {}",
                        name,
                        stringify!($field)
                    );
                };
            }
            disasm_theme_fields!(parsed, assert_field);
        }
    }

    /// `name = grey` is metadata, not a colour - it must not be mistaken for a
    /// field or abort the rest of the parse.
    #[test]
    fn name_line_is_ignored() {
        let mut theme = DisasmTheme::default();
        apply_theme_text(&mut theme, "name = grey\nregister_fg = #123456\n");
        assert_eq!(color_to_hex_str(theme.register_fg), "#123456");
    }

    /// The whole premise of putting both key sets in one file: the main theme's
    /// keys must not disturb the disassembly parse.
    #[test]
    fn main_theme_keys_are_ignored_by_the_disasm_parser() {
        let combined = "\
name = dark
main_fg = #D4D4D4
main_bg = #1E1E1E
offsets_fg = #569CD6
highlight_bg = #E5E5E2
register_fg = #123456
";
        let mut theme = DisasmTheme::default();
        apply_theme_text(&mut theme, combined);
        assert_eq!(color_to_hex_str(theme.register_fg), "#123456");
        // Untouched by the main-theme keys around it.
        assert_eq!(
            color_to_hex_str(theme.call_bg),
            color_to_hex_str(DisasmTheme::default().call_bg)
        );
    }

    /// A file with only main-theme keys must report "no disassembly colours", so
    /// `:set theme` keeps the current ones instead of resetting the view.
    #[test]
    fn has_disasm_keys_distinguishes_old_and_new_files() {
        let old_style = "name = coffee\nmain_fg = #4A2E22\nmain_bg = #EDD9B8\n";
        assert!(!has_disasm_keys(old_style));

        let combined = format!(
            "name = dark\nmain_bg = #1E1E1E\n{}",
            disasm_section_text(&disasm_preset("dark").expect("preset"))
        );
        assert!(has_disasm_keys(&combined));

        // A single disassembly key is enough to count.
        assert!(has_disasm_keys("comment_fg = #6A9955\n"));
        assert!(!has_disasm_keys(""));
    }

    /// Built-in presets stand in for theme files that predate the merge.
    #[test]
    fn preset_names_resolve_without_any_file() {
        for name in ["dark", "light", "grey", "gray"] {
            assert!(
                resolve_disasm_theme(name).is_some(),
                "'{}' must resolve from the built-in presets alone",
                name
            );
        }
    }
}

#[cfg(test)]
mod combined_file_tests {
    use super::*;

    /// End-to-end check of the one-level layout: a `themes/<name>.theme` holding
    /// both key sets resolves through the public entry point, and a file with
    /// only main-theme keys does not.
    #[test]
    fn combined_file_resolves_and_old_style_does_not() {
        let dir = crate::util::exe_dir().join("themes");
        fs::create_dir_all(&dir).expect("themes dir");

        let combined = dir.join("dz6-test-combined.theme");
        fs::write(
            &combined,
            "name = dz6-test-combined\n\
             main_fg = #C8D8E4\n\
             main_bg = #081E32\n\
             call_bg = #ABCDEF\n\
             comment_fg = #123456\n",
        )
        .expect("write combined theme");

        let old_style = dir.join("dz6-test-oldstyle.theme");
        fs::write(&old_style, "name = dz6-test-oldstyle\nmain_bg = #EDD9B8\n")
            .expect("write old-style theme");

        let resolved = resolve_disasm_theme("dz6-test-combined");
        let old_resolved = resolve_disasm_theme("dz6-test-oldstyle");

        // Clean up before asserting, so a failure can't leave files behind.
        let _ = fs::remove_file(&combined);
        let _ = fs::remove_file(&old_style);

        let resolved = resolved.expect("a file with disassembly keys must resolve");
        assert_eq!(color_to_hex_str(resolved.call_bg), "#ABCDEF");
        assert_eq!(color_to_hex_str(resolved.comment_fg), "#123456");
        // Keys the file omits keep their default rather than becoming black.
        assert_eq!(
            color_to_hex_str(resolved.register_fg),
            color_to_hex_str(DisasmTheme::default().register_fg)
        );

        assert!(
            old_resolved.is_none(),
            "a theme file without disassembly keys must resolve to None so \
             ':set theme' leaves the current disassembly colours alone"
        );
    }
}



