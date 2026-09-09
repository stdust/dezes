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
        crate::disasm::theme::parse_color_str(hex).unwrap_or(Color::Reset)
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

    pub fn load_from_str(content: &str, fallback: &Theme) -> Theme {
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

    pub fn load_from_file<P: AsRef<Path>>(path: P, fallback: &Theme) -> Theme {
        match fs::read_to_string(&path) {
            Ok(c) => Self::load_from_str(&c, fallback),
            Err(_) => fallback.clone(),
        }
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
                *style = *default;
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
            .fg(Color::from_u32(0x3B4A5A))
            .bg(Color::from_u32(0xC2C2BE))
            .add_modifier(Modifier::BOLD),
        main: Style::new()
            .fg(Color::from_u32(0x2B2B2B))
            .bg(Color::from_u32(0xD6D6D2))
            .add_modifier(Modifier::BOLD),
        dimmed: Style::new()
            .fg(Color::from_u32(0x7C7C78))
            .bg(Color::from_u32(0xD6D6D2))
            .add_modifier(Modifier::BOLD),
        dialog: Style::new()
            .fg(Color::from_u32(0x262626))
            .bg(Color::from_u32(0xE2E2DE))
            .add_modifier(Modifier::BOLD),
        changed_bytes: Style::new()
            .fg(Color::from_u32(0xA13A3A))
            .bg(Color::from_u32(0xD6D6D2)),
        highlight: Style::new()
            .fg(Color::from_u32(0x2B2B2B))
            .bg(Color::from_u32(0xBCCBE8)),
        byte_highlight: Style::new()
            .fg(Color::from_u32(0xF5F1E8))
            .bg(Color::from_u32(0xC05A52)),
        topbar: Style::new()
            .fg(Color::from_u32(0x262530))
            .bg(Color::from_u32(0xABB0A9)),
        error: Style::new()
            .fg(Color::from_u32(0x6E2A30))
            .bg(Color::from_u32(0xEFD6D2)),
        editing: Style::new()
            .fg(Color::from_u32(0xF5F1E8))
            .bg(Color::from_u32(0xC05A52))
            .add_modifier(Modifier::RAPID_BLINK),
    }
}

pub const EXTRA_BUILTIN_THEMES: &[(&str, &str)] = &[
    ("arctic_ice.theme", "# DZ6 Arctic Ice Theme\nname = arctic_ice\nmain_fg = #7FDBFF\nmain_bg = #08131F\noffsets_fg = #4A90A4\noffsets_bg = #08131F\ndimmed_fg = #2C4A5C\ndimmed_bg = #08131F\ndialog_fg = #B3ECFF\ndialog_bg = #0F2438\nchanged_bytes_fg = #FFDC73\nchanged_bytes_bg = #08131F\nhighlight_fg = #08131F\nhighlight_bg = #7FDBFF\nbyte_highlight_fg = #FFFFFF\nbyte_highlight_bg = #4A90A4\ntopbar_fg = #B3ECFF\ntopbar_bg = #163049\nerror_fg = #FF6B6B\nerror_bg = #2A0808\nediting_fg = #08131F\nediting_bg = #FFDC73\n"),
    ("coffee.theme", "# Coffee & Cognac\nname = coffee\nmain_fg = #E8D2A0\nmain_bg = #2B1D14\noffsets_fg = #A67C52\noffsets_bg = #2B1D14\ndimmed_fg = #5A4632\ndimmed_bg = #2B1D14\ndialog_fg = #2B1D14\ndialog_bg = #C08A4E\nchanged_bytes_fg = #D98C3F\nchanged_bytes_bg = #2B1D14\nhighlight_fg = #2B1D14\nhighlight_bg = #B57A3F\nbyte_highlight_fg = #FFF3D9\nbyte_highlight_bg = #8B5E34\ntopbar_fg = #E8D2A0\ntopbar_bg = #4A3320\nerror_fg = #E05C3E\nerror_bg = #4A2418\nediting_fg = #2B1D14\nediting_bg = #D98C3F\n"),
    ("darkone.theme", "# Dezes Theme File - darkone\nname = darkone\nmain_fg = #F2F2F2\nmain_bg = #0A0A0A\noffsets_fg = #29B6F6\noffsets_bg = #0A0A0A\ndimmed_fg = #78909C\ndimmed_bg = #0A0A0A\ndialog_fg = #F2F2F2\ndialog_bg = #0277BD\nchanged_bytes_fg = #FFD54F\nchanged_bytes_bg = #0A0A0A\nhighlight_fg = #F2F2F2\nhighlight_bg = #C62828\nbyte_highlight_fg = #F2F2F2\nbyte_highlight_bg = #00ACC1\ntopbar_fg = #F2F2F2\ntopbar_bg = #2C82C9\nerror_fg = #FFEBEE\nerror_bg = #C62828\nediting_fg = #000000\nediting_bg = #FFCA28\n"),
    ("ice.theme", "# DZ6 Ice White\nname = ice\nmain_fg = #DEDEDE\nmain_bg = #152028\noffsets_fg = #A8D7F0\noffsets_bg = #152028\ndimmed_fg = #6D8796\ndimmed_bg = #152028\ndialog_fg = #DEDEDE\ndialog_bg = #24333F\nchanged_bytes_fg = #FFF27A\nchanged_bytes_bg = #152028\nhighlight_fg = #152028\nhighlight_bg = #DEDEDE\nbyte_highlight_fg = #FFFFFF\nbyte_highlight_bg = #58B4E8\ntopbar_fg = #DEDEDE\ntopbar_bg = #334552\nerror_fg = #FF6666\nerror_bg = #440000\nediting_fg = #152028\nediting_bg = #FFF27A\n"),
    ("matrix.theme", "# DZ6 Matrix Green Theme\nname = matrix\nmain_fg = #00FF66\nmain_bg = #05140A\noffsets_fg = #00CC44\noffsets_bg = #05140A\ndimmed_fg = #2D6639\ndimmed_bg = #05140A\ndialog_fg = #00FF66\ndialog_bg = #0A2914\nchanged_bytes_fg = #FFFF00\nchanged_bytes_bg = #05140A\nhighlight_fg = #05140A\nhighlight_bg = #00FF66\nbyte_highlight_fg = #FFFFFF\nbyte_highlight_bg = #009933\ntopbar_fg = #00FF66\ntopbar_bg = #143D1E\nerror_fg = #FF3333\nerror_bg = #330000\nediting_fg = #05140A\nediting_bg = #FFFF00\n"),
    ("mocha.theme", "# DZ6 Everforest Theme\nname = mocha\nmain_fg = #D3C6AA\nmain_bg = #2B3339\noffsets_fg = #859289\noffsets_bg = #2B3339\ndimmed_fg = #4A555B\ndimmed_bg = #2B3339\ndialog_fg = #DDC7A1\ndialog_bg = #343F44\nchanged_bytes_fg = #DBBC7F\nchanged_bytes_bg = #2B3339\nhighlight_fg = #2B3339\nhighlight_bg = #A7C080\nbyte_highlight_fg = #2B3339\nbyte_highlight_bg = #83C092\ntopbar_fg = #D3C6AA\ntopbar_bg = #3A464C\nerror_fg = #E67E80\nerror_bg = #3C2E2E\nediting_fg = #2B3339\nediting_bg = #DBBC7F\n"),
    ("paper.theme", "# DZ6 Gruvbox Theme\nname = paper\nmain_fg = #EBDBB2\nmain_bg = #282828\noffsets_fg = #A89984\noffsets_bg = #282828\ndimmed_fg = #665C54\ndimmed_bg = #282828\ndialog_fg = #FBF1C7\ndialog_bg = #3C3836\nchanged_bytes_fg = #D79921\nchanged_bytes_bg = #282828\nhighlight_fg = #282828\nhighlight_bg = #B8BB26\nbyte_highlight_fg = #282828\nbyte_highlight_bg = #689D6A\ntopbar_fg = #EBDBB2\ntopbar_bg = #3C3836\nerror_fg = #CC241D\nerror_bg = #3C2020\nediting_fg = #282828\nediting_bg = #D79921\n"),
    ("latte.theme", "# DZ6 Gruvbox Light Theme\nname = latte\nmain_fg = #3C3836\nmain_bg = #F7F1DF\noffsets_fg = #7C6F64\noffsets_bg = #F7F1DF\ndimmed_fg = #EEDFBB\ndimmed_bg = #F7F1DF\ndialog_fg = #282828\ndialog_bg = #EBDBB2\nchanged_bytes_fg = #D79921\nchanged_bytes_bg = #F7F1DF\nhighlight_fg = #282828\nhighlight_bg = #EBE2B8\nbyte_highlight_fg = #282828\nbyte_highlight_bg = #689D6A\ntopbar_fg = #3C3836\ntopbar_bg = #EBDBB2\nerror_fg = #CC241D\nerror_bg = #F2D9C4\nediting_fg = #282828\nediting_bg = #D79921\n"),
    ("punk.theme", "# DZ6 Cyberpunk Theme\nname = punk\nmain_fg = #00F0FF\nmain_bg = #0D0F18\noffsets_fg = #00F0FF\noffsets_bg = #0D0F18\ndimmed_fg = #708090\ndimmed_bg = #0D0F18\ndialog_fg = #00F0FF\ndialog_bg = #1A092B\nchanged_bytes_fg = #FFE600\nchanged_bytes_bg = #0D0F18\nhighlight_fg = #0D0F18\nhighlight_bg = #00F0FF\nbyte_highlight_fg = #FFFFFF\nbyte_highlight_bg = #FF007F\ntopbar_fg = #FFFFFF\ntopbar_bg = #2B0938\nerror_fg = #FF0055\nerror_bg = #3B0014\nediting_fg = #0D0F18\nediting_bg = #FFE600\n"),
    ("terracotta.theme", "# Terracotta Adobe Theme\nname = terracotta\nmain_fg = #4A2E22\nmain_bg = #EDD9B8\noffsets_fg = #8B5A2B\noffsets_bg = #EDD9B8\ndimmed_fg = #D9B98C\ndimmed_bg = #EDD9B8\ndialog_fg = #2E1B12\ndialog_bg = #D98452\nchanged_bytes_fg = #C1440E\nchanged_bytes_bg = #EDD9B8\nhighlight_fg = #2E1B12\nhighlight_bg = #E8B36B\nbyte_highlight_fg = #FFF3E0\nbyte_highlight_bg = #A63B1F\ntopbar_fg = #4A2E22\ntopbar_bg = #D98452\nerror_fg = #8B0000\nerror_bg = #F0C4A8\nediting_fg = #FFF3E0\nediting_bg = #C1440E\n"),
    ("cream.theme", "name = cream\nmain_fg = #2B2B26\nmain_bg = #FFFBF0\noffsets_fg = #3B4A5A\noffsets_bg = #EFE9D8\ndimmed_fg = #8C8775\ndimmed_bg = #FFFBF0\ndialog_fg = #262622\ndialog_bg = #FFFEF5\nchanged_bytes_fg = #A13A3A\nchanged_bytes_bg = #FFFBF0\nhighlight_fg = #2B2B26\nhighlight_bg = #CFC8B7\nbyte_highlight_fg = #FFFBF0\nbyte_highlight_bg = #C05A52\ntopbar_fg = #2A241C\ntopbar_bg = #D8CBA8\nerror_fg = #6E2A30\nerror_bg = #F5DCD0\nediting_fg = #FFFBF0\nediting_bg = #C05A52\n"),
    ("slate.theme", "# Dezes Theme File\nname = slate_mist\nmain_fg = #2C333A\nmain_bg = #DCE1E6\noffsets_fg = #41505E\noffsets_bg = #CCD4DC\ndimmed_fg = #77838F\ndimmed_bg = #E1E6EB\ndialog_fg = #22282E\ndialog_bg = #EAF0F5\nchanged_bytes_fg = #AC4444\nchanged_bytes_bg = #E1E6EB\nhighlight_fg = #2C333A\nhighlight_bg = #C2D1E0\nbyte_highlight_fg = #F5F7FA\nbyte_highlight_bg = #506D8A\ntopbar_fg = #22282E\ntopbar_bg = #BFCBD6\nerror_fg = #6E2A35\nerror_bg = #EED3D7\nediting_fg = #F5F7FA\nediting_bg = #506D8A\n"),
    ("sage.theme", "# Dezes Theme File\nname = sage_mist\nmain_fg = #2A362D\nmain_bg = #DCE4D6\noffsets_fg = #3E5444\noffsets_bg = #D3DDD0\ndimmed_fg = #788A7B\ndimmed_bg = #E6ECE3\ndialog_fg = #222C25\ndialog_bg = #EEF4EB\nchanged_bytes_fg = #A64B3B\nchanged_bytes_bg = #E6ECE3\nhighlight_fg = #2A362D\nhighlight_bg = #C4D9C8\nbyte_highlight_fg = #F5F9F4\nbyte_highlight_bg = #4F785C\ntopbar_fg = #222C25\ntopbar_bg = #CCD9C9\nerror_fg = #6B2929\nerror_bg = #EDD5D5\nediting_fg = #F5F9F4\nediting_bg = #4F785C\n"),
];

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

    for (fname, content) in EXTRA_BUILTIN_THEMES {
        let extra_path = dir.join(fname);
        if !extra_path.exists() {
            let _ = fs::write(&extra_path, content);
        }
    }

    let dark = Theme::load_from_file(&dark_path, &dark_def);
    let light = Theme::load_from_file(&light_path, &light_def);
    let gray = Theme::load_from_file(&gray_path, &gray_def);

    (dark, light, gray)
}

pub fn find_theme_path(name: &str) -> Option<std::path::PathBuf> {
    let clean_name = name.trim();
    let theme_files: Vec<String> = match clean_name {
        "slate" | "slate_mist" => vec!["slate.theme".to_string(), "slate_mist.theme".to_string()],
        "sage" | "sage_mist" => vec!["sage.theme".to_string(), "sage_mist.theme".to_string()],
        s if s.ends_with(".theme") => vec![s.to_string()],
        s => vec![format!("{}.theme", s)],
    };

    let mut candidates = Vec::new();

    // Executable directory first - that's where `ensure_and_load_themes` keeps
    // the install's theme set.
    let exe = crate::util::exe_dir();
    let startup = crate::util::startup_dir();

    for theme_file in &theme_files {
        // 1. <exe_dir>/themes/<theme_file>
        candidates.push(exe.join("themes").join(theme_file));
        // 2. <exe_dir>/<theme_file>
        candidates.push(exe.join(theme_file));

        if startup != exe {
            // 3. <startup_dir>/themes/<theme_file>
            candidates.push(startup.join("themes").join(theme_file));
            // 4. <startup_dir>/<theme_file>
            candidates.push(startup.join(theme_file));
        }
    }

    candidates.into_iter().find(|path| path.is_file())
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

pub fn theme_exists(name: &str) -> bool {
    let clean_name = name.trim();
    if clean_name.eq_ignore_ascii_case("dark")
        || clean_name.eq_ignore_ascii_case("light")
        || clean_name.eq_ignore_ascii_case("gray")
        || clean_name.eq_ignore_ascii_case("grey")
    {
        return true;
    }
    if find_theme_path(clean_name).is_some() {
        return true;
    }
    EXTRA_BUILTIN_THEMES.iter().any(|(fname, _)| {
        let stem = fname.strip_suffix(".theme").unwrap_or(fname);
        stem.eq_ignore_ascii_case(clean_name)
            || (stem == "slate" && clean_name.eq_ignore_ascii_case("slate_mist"))
            || (stem == "sage" && clean_name.eq_ignore_ascii_case("sage_mist"))
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
    } else if let Some((_, content)) = EXTRA_BUILTIN_THEMES.iter().find(|(fname, _)| {
        let stem = fname.strip_suffix(".theme").unwrap_or(fname);
        stem.eq_ignore_ascii_case(clean_name)
            || (stem == "slate" && clean_name.eq_ignore_ascii_case("slate_mist"))
            || (stem == "sage" && clean_name.eq_ignore_ascii_case("sage_mist"))
    }) {
        Theme::load_from_str(content, &fallback)
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

    #[test]
    fn new_builtin_themes_load_properly() {
        let cream = load_theme_or_fallback("cream");
        assert_eq!(cream.name, "cream");
        assert_eq!(Theme::color_to_hex(cream.main.bg.unwrap()), "#FFFBF0");

        let slate = load_theme_or_fallback("slate");
        assert_eq!(slate.name, "slate_mist");
        assert_eq!(Theme::color_to_hex(slate.main.bg.unwrap()), "#DCE1E6");

        let slate_mist = load_theme_or_fallback("slate_mist");
        assert_eq!(slate_mist.name, "slate_mist");
        assert_eq!(Theme::color_to_hex(slate_mist.main.bg.unwrap()), "#DCE1E6");

        let sage = load_theme_or_fallback("sage");
        assert_eq!(sage.name, "sage_mist");
        assert_eq!(Theme::color_to_hex(sage.main.bg.unwrap()), "#DCE4D6");

        let sage_mist = load_theme_or_fallback("sage_mist");
        assert_eq!(sage_mist.name, "sage_mist");
        assert_eq!(Theme::color_to_hex(sage_mist.main.bg.unwrap()), "#DCE4D6");
    }

    #[test]
    fn every_extra_builtin_theme_has_main_keys() {
        for (fname, content) in EXTRA_BUILTIN_THEMES {
            assert!(
                has_main_keys(content),
                "Built-in theme '{}' lacks main keys",
                fname
            );
        }
    }

    #[test]
    fn theme_exists_distinguishes_valid_and_invalid_names() {
        assert!(theme_exists("dark"));
        assert!(theme_exists("light"));
        assert!(theme_exists("gray"));
        assert!(theme_exists("cream"));
        assert!(theme_exists("slate"));
        assert!(theme_exists("slate_mist"));
        assert!(theme_exists("sage"));
        assert!(theme_exists("sage_mist"));
        assert!(!theme_exists("unknown_nonexistent_theme_xyz"));
        assert!(!theme_exists(""));
    }
}
