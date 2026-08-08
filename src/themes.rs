use ratatui::style::{Color, Modifier, Style};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub main: Style,
    pub dimmed: Style,
    pub offsets: Style,
    pub changed_bytes: Style,
    pub highlight: Style,
    pub byte_highlight: Style,
    pub topbar: Style,
    pub error: Style,
    pub editing: Style,
    pub dialog: Style,
}

impl Theme {
    pub fn parse_color(hex: &str) -> Color {
        let clean = hex.trim().trim_start_matches('#').trim_start_matches("0x").trim_start_matches("0X");
        if clean.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                let r = ((rgb >> 16) & 0xFF) as u8;
                let g = ((rgb >> 8) & 0xFF) as u8;
                let b = (rgb & 0xFF) as u8;
                return Color::Rgb(r, g, b);
            }
        }
        Color::Reset
    }

    /// Hex text for a colour, for writing theme files.
    ///
    /// Named `ratatui` colours have to be spelled out rather than falling into a
    /// catch-all. They used to collapse to `#000000`, which turned the dark
    /// theme's `byte_highlight` (White on Red) into black on black the moment the
    /// file was written and read back - the search-hit cursor became invisible.
    pub fn color_to_hex(color: Color) -> String {
        match color {
            Color::Rgb(r, g, b) => format!("#{:02X}{:02X}{:02X}", r, g, b),
            Color::Indexed(idx) => format!("#{:06X}", idx),
            // Same named-colour table the disassembly theme writer uses, so the
            // two file formats can't disagree on what "red" means.
            other => crate::disasm::theme::color_to_hex_str(other),
        }
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let mut content = String::new();
        content.push_str("# Dezes Theme File\n");
        content.push_str(&format!("name = {}\n\n", self.name));

        let write_style = |content: &mut String, prefix: &str, style: &Style| {
            if let Some(fg) = style.fg {
                content.push_str(&format!("{}_fg = {}\n", prefix, Self::color_to_hex(fg)));
            }
            if let Some(bg) = style.bg {
                content.push_str(&format!("{}_bg = {}\n", prefix, Self::color_to_hex(bg)));
            }
        };

        write_style(&mut content, "main", &self.main);
        write_style(&mut content, "offsets", &self.offsets);
        write_style(&mut content, "dimmed", &self.dimmed);
        write_style(&mut content, "dialog", &self.dialog);
        write_style(&mut content, "changed_bytes", &self.changed_bytes);
        write_style(&mut content, "highlight", &self.highlight);
        write_style(&mut content, "byte_highlight", &self.byte_highlight);
        write_style(&mut content, "topbar", &self.topbar);
        write_style(&mut content, "error", &self.error);
        write_style(&mut content, "editing", &self.editing);

        fs::write(path, content)
    }

    // A `save_to_file_with_disasm` used to append the disassembly colours here, so
    // one `.theme` file described both views. It was removed along with the
    // duplication: the disassembly colours live only in `themes/disasm.theme` now.
    // Reading them out of a combined file is still supported, for files users
    // already have.

    pub fn load_from_file<P: AsRef<Path>>(path: P, fallback: &Theme) -> Theme {
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return fallback.clone(),
        };

        let mut theme = fallback.clone();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                continue;
            }

            let key = parts[0].trim();
            let val = parts[1].trim();

            if key == "name" {
                theme.name = val.to_string();
                continue;
            }

            let color = Self::parse_color(val);
            if color == Color::Reset {
                continue;
            }

            theme.apply_color(key, color);
        }

        theme.repair_unreadable_styles(fallback);
        theme
    }

    /// Sets the style field named by a theme-file key. False for an unknown key.
    ///
    /// The one place a key name maps to a field, so the file loader and any other
    /// caller cannot disagree about what `highlight_bg` means. `MAIN_KEYS` lists the
    /// same names and `main_keys_are_all_recognised` checks the two against each
    /// other.
    pub fn apply_color(&mut self, key: &str, color: Color) -> bool {
        match key {
            "main_fg" => self.main = self.main.fg(color),
            "main_bg" => self.main = self.main.bg(color),
            "offsets_fg" => self.offsets = self.offsets.fg(color),
            "offsets_bg" => self.offsets = self.offsets.bg(color),
            "dimmed_fg" => self.dimmed = self.dimmed.fg(color),
            "dimmed_bg" => self.dimmed = self.dimmed.bg(color),
            "dialog_fg" => self.dialog = self.dialog.fg(color),
            "dialog_bg" => self.dialog = self.dialog.bg(color),
            "changed_bytes_fg" => self.changed_bytes = self.changed_bytes.fg(color),
            "changed_bytes_bg" => self.changed_bytes = self.changed_bytes.bg(color),
            "highlight_fg" => self.highlight = self.highlight.fg(color),
            "highlight_bg" => self.highlight = self.highlight.bg(color),
            "byte_highlight_fg" => self.byte_highlight = self.byte_highlight.fg(color),
            "byte_highlight_bg" => self.byte_highlight = self.byte_highlight.bg(color),
            "topbar_fg" => self.topbar = self.topbar.fg(color),
            "topbar_bg" => self.topbar = self.topbar.bg(color),
            "error_fg" => self.error = self.error.fg(color),
            "error_bg" => self.error = self.error.bg(color),
            "editing_fg" => self.editing = self.editing.fg(color),
            "editing_bg" => self.editing = self.editing.bg(color),
            _ => return false,
        }
        true
    }

    /// Replaces any style whose foreground and background ended up identical.
    ///
    /// Such a style renders as a solid block - the text is there but invisible.
    /// Theme files written by older builds contain exactly that: `color_to_hex`
    /// collapsed every named `ratatui` colour to `#000000`, so the dark theme's
    /// `byte_highlight` (White on Red) was saved as black on black, and Alt+H
    /// turned the highlighted bytes into unreadable blocks.
    ///
    /// Fixing the writer stopped new files from being written that way, but files
    /// already on disk are never rewritten - a user with a long-standing
    /// `themes/dark.theme` keeps the broken value. Repairing on load is what
    /// actually clears it, and it costs one comparison per style.
    fn repair_unreadable_styles(&mut self, fallback: &Theme) {
        let pairs: [(&mut Style, &Style); 10] = [
            (&mut self.main, &fallback.main),
            (&mut self.dimmed, &fallback.dimmed),
            (&mut self.offsets, &fallback.offsets),
            (&mut self.changed_bytes, &fallback.changed_bytes),
            (&mut self.highlight, &fallback.highlight),
            (&mut self.byte_highlight, &fallback.byte_highlight),
            (&mut self.topbar, &fallback.topbar),
            (&mut self.error, &fallback.error),
            (&mut self.editing, &fallback.editing),
            (&mut self.dialog, &fallback.dialog),
        ];

        for (style, default) in pairs {
            if let (Some(fg), Some(bg)) = (style.fg, style.bg)
                && fg == bg
            {
                *style = **&default;
            }
        }
    }
}

/// Background of the built-in dark theme: near-black, matching the value the
/// hand-edited `dark.theme` files in the wild already use.
const DARK_BG: u32 = 0x1e1e1e;

pub fn get_default_dark() -> Theme {
    Theme {
        name: "dark".to_string(),
        offsets: Style::new()
            .fg(Color::from_u32(0x569cd6))
            .bg(Color::from_u32(DARK_BG))
            .add_modifier(Modifier::BOLD),
        main: Style::new()
            .fg(Color::from_u32(0xd4d4d4))
            .bg(Color::from_u32(DARK_BG))
            .add_modifier(Modifier::BOLD),
        dimmed: Style::new()
            .fg(Color::from_u32(0x949494))
            .bg(Color::from_u32(DARK_BG))
            .add_modifier(Modifier::BOLD),
        dialog: Style::new()
            .fg(Color::Rgb(204, 204, 204))
            .bg(Color::from_u32(0x081e32))
            .add_modifier(Modifier::BOLD),
        changed_bytes: Style::new()
            .fg(Color::Rgb(255, 215, 0))
            .bg(Color::from_u32(DARK_BG)),
        highlight: Style::new()
            .fg(Color::from_u32(DARK_BG))
            .bg(Color::from_u32(0xe5e5e2)),
        byte_highlight: Style::new().fg(Color::White).bg(Color::Red),
        topbar: Style::new()
            .fg(Color::from_u32(0xffffff))
            .bg(Color::from_u32(0x555555)),
        error: Style::new()
            .fg(Color::Rgb(255, 85, 85))
            .bg(Color::from_u32(0x400000)),
        editing: Style::new()
            .fg(Color::from_u32(DARK_BG))
            .bg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::RAPID_BLINK),
    }
}

pub fn get_default_light() -> Theme {
    Theme {
        name: "light".to_string(),
        offsets: Style::new()
            .fg(Color::from_u32(0x15141e))
            .bg(Color::from_u32(0xeeeeee))
            .add_modifier(Modifier::BOLD),
        main: Style::new()
            .fg(Color::from_u32(0x000000))
            .bg(Color::from_u32(0xeeeeee))
            .add_modifier(Modifier::BOLD),
        dimmed: Style::new()
            .fg(Color::from_u32(0x707070))
            .bg(Color::from_u32(0xeeeeee))
            .add_modifier(Modifier::BOLD),
        dialog: Style::new()
            .fg(Color::from_u32(0x333333))
            .bg(Color::from_u32(0xe7f3ff))
            .add_modifier(Modifier::BOLD),
        changed_bytes: Style::new()
            .fg(Color::from_u32(0x795e00))
            .bg(Color::from_u32(0xeeeeee)),
        highlight: Style::new()
            .fg(Color::from_u32(0x000000))
            .bg(Color::from_u32(0xd8bfa3)),
        byte_highlight: Style::new().fg(Color::Black).bg(Color::from_u32(0xffb3b3)),
        topbar: Style::new()
            .fg(Color::from_u32(0xffffff))
            .bg(Color::from_u32(0x919191)),
        error: Style::new()
            .fg(Color::from_u32(0xe51400))
            .bg(Color::from_u32(0xf2dede)),
        editing: Style::new()
            .fg(Color::from_u32(0xffffff))
            .bg(Color::from_u32(0xffcc00))
            .add_modifier(Modifier::RAPID_BLINK),
    }
}

pub fn get_default_gray() -> Theme {
    Theme {
        name: "gray".to_string(),
        offsets: Style::new()
            .fg(Color::from_u32(0x004080))
            .bg(Color::from_u32(0xc4c4c4))
            .add_modifier(Modifier::BOLD),
        main: Style::new()
            .fg(Color::from_u32(0x505050))
            .bg(Color::from_u32(0xd9d9d9))
            .add_modifier(Modifier::BOLD),
        dimmed: Style::new()
            .fg(Color::from_u32(0x787878))
            .bg(Color::from_u32(0xd9d9d9))
            .add_modifier(Modifier::BOLD),
        dialog: Style::new()
            .fg(Color::from_u32(0x111111))
            .bg(Color::from_u32(0xe6e6e6))
            .add_modifier(Modifier::BOLD),
        changed_bytes: Style::new()
            .fg(Color::from_u32(0x990000))
            .bg(Color::from_u32(0xd9d9d9)),
        highlight: Style::new()
            .fg(Color::from_u32(0x505050))
            .bg(Color::from_u32(0xb1c3e7)),
        byte_highlight: Style::new()
            .fg(Color::from_u32(0xffffff))
            .bg(Color::from_u32(0xd9534f)),
        topbar: Style::new()
            .fg(Color::from_u32(0x15141e))
            .bg(Color::from_u32(0xadb5bd)),
        error: Style::new()
            .fg(Color::from_u32(0x721c24))
            .bg(Color::from_u32(0xf8d7da)),
        editing: Style::new()
            .fg(Color::from_u32(0xffffff))
            .bg(Color::from_u32(0xd9534f))
            .add_modifier(Modifier::RAPID_BLINK),
    }
}

pub fn ensure_and_load_themes() -> (Theme, Theme, Theme) {
    // Anchored to the executable's directory, so one install has one set of
    // themes no matter which directory dz6 is launched from.
    //
    // This used to be the startup directory, which meant launching dz6 with the
    // CWD set to some data folder (as Explorer's "open with" does) created a
    // `themes/` there, wrote the built-in defaults into it, and rendered the
    // same file in different colours than the previous folder did.
    let dir = crate::util::exe_dir().join("themes");
    let dir = dir.as_path();
    if !dir.exists() {
        let _ = fs::create_dir_all(dir);
    }

    let dark_path = dir.join("dark.theme");
    let light_path = dir.join("light.theme");
    let gray_path = dir.join("gray.theme");

    let dark_def = get_default_dark();
    let light_def = get_default_light();
    let gray_def = get_default_gray();

    // Only the hex-view colours. The disassembly colours are not duplicated into
    // these files: they live in `themes/disasm.theme`, and having the same keys in
    // two places meant `:set theme <name>` re-applied a stale copy over whatever was
    // in `disasm.theme` on every launch. Files that already exist are left untouched.
    //
    // `:set theme <name>` still colours both views: with no disassembly keys in the
    // file, `resolve_disasm_theme` falls back to the built-in preset of the same
    // name.
    if !dark_path.exists() {
        let _ = dark_def.save_to_file(&dark_path);
    }
    if !light_path.exists() {
        let _ = light_def.save_to_file(&light_path);
    }
    if !gray_path.exists() {
        let _ = gray_def.save_to_file(&gray_path);
    }

    let dark = Theme::load_from_file(&dark_path, &dark_def);
    let light = Theme::load_from_file(&light_path, &light_def);
    let gray = Theme::load_from_file(&gray_path, &gray_def);

    (dark, light, gray)
}

pub fn find_theme_path(name: &str) -> Option<std::path::PathBuf> {
    let clean_name = name.trim();
    let theme_file = if clean_name.ends_with(".theme") {
        clean_name.to_string()
    } else {
        format!("{}.theme", clean_name)
    };

    let mut candidates = Vec::new();

    // Executable directory first - that's where `ensure_and_load_themes` keeps
    // the install's theme set.
    let exe = crate::util::exe_dir();
    // 1. <exe_dir>/themes/<theme_file>
    candidates.push(exe.join("themes").join(&theme_file));
    // 2. <exe_dir>/<theme_file>
    candidates.push(exe.join(&theme_file));

    // Startup directory kept as a read-only fallback, so a theme a user already
    // dropped next to their data files still resolves. Nothing is ever written
    // there.
    let startup = crate::util::startup_dir();
    if startup != exe {
        // 3. <startup_dir>/themes/<theme_file>
        candidates.push(startup.join("themes").join(&theme_file));
        // 4. <startup_dir>/<theme_file>
        candidates.push(startup.join(&theme_file));
    }

    for path in candidates {
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    None
}

/// Every key `Theme::load_from_file` understands.
///
/// Must track the match arms in that function. `main_keys_are_all_recognised`
/// in the tests below fails if an entry here stops having an effect, which is
/// what keeps the two from drifting.
const MAIN_KEYS: [&str; 20] = [
    "main_fg",
    "main_bg",
    "offsets_fg",
    "offsets_bg",
    "dimmed_fg",
    "dimmed_bg",
    "dialog_fg",
    "dialog_bg",
    "changed_bytes_fg",
    "changed_bytes_bg",
    "highlight_fg",
    "highlight_bg",
    "byte_highlight_fg",
    "byte_highlight_bg",
    "topbar_fg",
    "topbar_bg",
    "error_fg",
    "error_bg",
    "editing_fg",
    "editing_bg",
];

/// The theme key a `:set` option name refers to.
///
/// `bg` and `fg` are shorthands for the two that get changed most; every key a
/// theme file accepts works as well.
pub fn resolve_color_key(name: &str) -> Option<&'static str> {
    match name {
        "bg" => Some("main_bg"),
        "fg" => Some("main_fg"),
        other => MAIN_KEYS.iter().copied().find(|key| *key == other),
    }
}

/// True when `data` carries at least one main-view colour key.
///
/// A file holding only disassembly keys is not a main theme. Loading one used to
/// succeed "quietly": every key was ignored, so the result was the untouched
/// dark fallback and the screen simply went near-black, which looks like the
/// theme applied rather than like an error.
pub fn has_main_keys(data: &str) -> bool {
    data.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            return false;
        }
        match line.split_once('=') {
            Some((key, _)) => MAIN_KEYS.contains(&key.trim()),
            None => false,
        }
    })
}

pub fn load_theme_or_fallback(name: &str) -> Theme {
    let (dark_def, light_def, gray_def) = ensure_and_load_themes();

    let clean_name = name.trim();
    let fallback = match clean_name {
        "light" => light_def,
        "gray" | "grey" => gray_def,
        _ => dark_def,
    };

    if let Some(path) = find_theme_path(clean_name) {
        Theme::load_from_file(path, &fallback)
    } else {
        fallback
    }
}

#[cfg(test)]
mod theme_key_tests {
    use super::*;

    /// Every name in `MAIN_KEYS` must actually be handled by
    /// `Theme::load_from_file`, otherwise `has_main_keys` would accept a file
    /// whose keys do nothing and the screen would silently fall back to dark.
    #[test]
    fn main_keys_are_all_recognised() {
        let base = get_default_dark();
        let sentinel = "#0A0B0C";
        let dir = std::env::temp_dir();
        for key in MAIN_KEYS {
            let path = dir.join(format!("dz6-key-test-{}.theme", key));
            fs::write(&path, format!("{} = {}\n", key, sentinel)).expect("write");
            let loaded = Theme::load_from_file(&path, &base);
            let _ = fs::remove_file(&path);

            let styles = [
                loaded.main,
                loaded.dimmed,
                loaded.offsets,
                loaded.changed_bytes,
                loaded.highlight,
                loaded.byte_highlight,
                loaded.topbar,
                loaded.error,
                loaded.editing,
                loaded.dialog,
            ];
            let applied = styles.iter().any(|s| {
                s.fg.map(|c| Theme::color_to_hex(c) == sentinel).unwrap_or(false)
                    || s.bg.map(|c| Theme::color_to_hex(c) == sentinel).unwrap_or(false)
            });
            assert!(
                applied,
                "'{}' is in MAIN_KEYS but load_from_file ignores it",
                key
            );
        }
    }

    /// The exact situation that turned the screen black: a file carrying only
    /// disassembly keys must not be accepted as a main theme.
    #[test]
    fn disasm_only_file_has_no_main_keys() {
        let disasm_only = "\
name = grey
call_bg = #7FD4DE
call_fg = #14181A
jcc_fg = #8E1616
register_fg = #9BE0A5
";
        assert!(!has_main_keys(disasm_only));
        assert!(crate::disasm::theme::has_disasm_keys(disasm_only));
    }

    #[test]
    fn combined_file_has_both_key_sets() {
        let combined = "\
name = dark
main_bg = #1E1E1E
call_bg = #56C8D8
";
        assert!(has_main_keys(combined));
        assert!(crate::disasm::theme::has_disasm_keys(combined));
    }

    /// Commented-out keys must not count, or a file could pass the check on the
    /// strength of its documentation header alone.
    #[test]
    fn commented_keys_do_not_count() {
        assert!(!has_main_keys("# main_bg = #1E1E1E\n"));
        assert!(has_main_keys("# main_bg = #1E1E1E\nmain_fg = #FFFFFF\n"));
    }

    /// Named ratatui colours must round-trip instead of collapsing to black.
    #[test]
    fn named_colors_serialize_to_their_real_hex() {
        assert_eq!(Theme::color_to_hex(Color::White), "#FFFFFF");
        assert_eq!(Theme::color_to_hex(Color::Red), "#FF0000");
        assert_eq!(Theme::color_to_hex(Color::Black), "#000000");
        assert_eq!(Theme::color_to_hex(Color::Rgb(0x08, 0x1E, 0x32)), "#081E32");
    }
}
