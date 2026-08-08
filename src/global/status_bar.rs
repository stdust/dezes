//! Bottom status bar.
//!
//! Layout and contents are the original ones: the file name (plus the
//! `Read Only` marker and search hit counter) on the left, and a `│`-separated block of
//! IME / encodings / mode / file type / selection / address / percentage on the
//! right.
//!
//! Three defects in that original implementation are fixed here:
//!
//! * The two halves were rendered into the *same* full-width rect, one
//!   left-aligned and one right-aligned. On a narrow terminal they overwrote
//!   each other. They now get a rect each, so the left half is clipped instead.
//! * The address was built with a space-padded width and then `.trim()`ed, so
//!   the whole right-hand block slid sideways as the cursor moved between
//!   address lengths. The width is now fixed.
//! * The percentage was rounded, which reported 100% before the end of the file
//!   was reached. It is floored.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};

use crate::{app::App, editor::UIState};

/// Width of the selection slot: `[` + four hex digits + `]`.
const SELECTION_WIDTH: usize = 6;

/// Width of the file type slot, sized for the longest label (`PE64`).
///
/// Reserved rather than conditional so that opening a file, which is when the
/// type first becomes known, doesn't shift the fields to its left.
const TYPE_WIDTH: usize = 4;

/// Width of the mode slot.
///
/// Ten columns covers the modes visible while a view is on screen - `NORMAL`,
/// `SELECT`, `DISASM`, `EDIT/HEX`, `EDIT/UTF-8` and so on. Longer names (a
/// dialog mode such as `REPLACE/PATTERN`, or `EDIT/` with a long encoding label)
/// simply extend the slot; those appear with a dialog over the view, where a
/// shift underneath isn't visible anyway. Padding beats truncating here because
/// the mode name is the field most often read.
const MODE_WIDTH: usize = 10;

pub fn status_bar_draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let enc1_name = app.text_view.table.name();
    let encoding_info = if let Some(enc2_table) = app.hex_view.enc2_table {
        format!("{}│{}", enc1_name, enc2_table.name())
    } else {
        enc1_name.to_string()
    };

    use std::borrow::Cow;
    use std::fmt::Write;

    let mode_str: Cow<'static, str> = match app.state {
        UIState::Normal | UIState::DialogEditData => {
            match app.editor_view {
                crate::editor::AppView::Disasm => Cow::Borrowed("DISASM"),
                crate::editor::AppView::Header => Cow::Borrowed("HEADER"),
                crate::editor::AppView::Text => Cow::Borrowed("TEXT"),
                // The Hex view names itself like the other three. It used to read
                // `NORMAL` - vi's normal mode - which was both a leftover from the
                // vi-style bindings and the one entry in this field that described a
                // mode rather than what is on screen.
                _ => Cow::Borrowed("HEX"),
            }
        }
        UIState::HexEditing => {
            use crate::editor::EditingTarget;
            match app.hex_view.editing_target {
                EditingTarget::Hex => Cow::Borrowed("EDIT/HEX"),
                EditingTarget::Enc1 => Cow::Owned(format!("EDIT/{}", enc1_name)),
                EditingTarget::Enc2 => Cow::Owned(format!("EDIT/{}", app.hex_view.get_enc2_table().name())),
            }
        }
        UIState::HexSelection => Cow::Borrowed("SELECT"),
        UIState::DialogAbout => Cow::Borrowed("ABOUT"),
        UIState::DialogCalculator => Cow::Borrowed("CALC"),
        UIState::DialogBase => Cow::Borrowed("BASE"),
        UIState::DialogComment => Cow::Borrowed("COMMENT"),
        UIState::DialogEncoding => Cow::Borrowed("ENCODING1"),
        UIState::DialogEncoding2 => Cow::Borrowed("ENCODING2"),
        UIState::DialogGoto => Cow::Borrowed("GOTO"),
        UIState::DialogHeaderEdit => Cow::Borrowed("EDIT/HEADER"),
        UIState::DialogHelp => Cow::Borrowed("HELP"),
        UIState::DialogLog => Cow::Borrowed("LOG"),
        UIState::Matrix => Cow::Borrowed("MATRIX"),
        UIState::DialogSettings => Cow::Borrowed("SETTINGS"),
        UIState::DialogNames | UIState::DialogNamesRegex => Cow::Borrowed("NAMES"),
        UIState::DialogStrings | UIState::DialogStringEdit => Cow::Borrowed("STRINGS"),
        UIState::DialogModifyBlock => Cow::Borrowed("MODIFY"),
        UIState::DialogReplacePattern => Cow::Borrowed("REPLACE/PATTERN"),
        UIState::DialogFindPattern => Cow::Borrowed("FIND/PATTERN"),
        UIState::DialogXref => Cow::Borrowed("XREF"),
        UIState::DialogStringRef => Cow::Borrowed("STR REFS"),
        UIState::DialogAssemble => Cow::Borrowed("ASSEMBLE"),
        UIState::DialogSectionSize => Cow::Borrowed("SECTION"),
        UIState::DialogFileDialog => Cow::Borrowed("FILE"),
        UIState::DialogDriveSelect => Cow::Borrowed("DRIVE"),
        UIState::Command => Cow::Borrowed("COMMAND"),
        UIState::Error => Cow::Borrowed("ERROR"),
    };

    let match_count_str = if !app.hex_view.search.matches.is_empty() {
        if let Some(idx) = app.hex_view.search.match_index {
            format!(" ({}/{})", idx + 1, app.hex_view.search.matches.len())
        } else {
            format!(" (0/{})", app.hex_view.search.matches.len())
        }
    } else {
        String::new()
    };

    let filename = &app.file_info.name;

    // Floored, not rounded: rounding showed 100% while there was still up to
    // half a percent of the file left below the cursor.
    let percent = if app.file_info.size == 0 {
        0
    } else {
        (app.hex_view.offset * 100 / app.file_info.size).min(100)
    };

    // Bracketed so it reads as a badge on the filename rather than as part of it:
    // `sample.bin [Read Only]`, not `sample.bin Read Only`.
    let read_only = if app.file_info.is_read_only {
        format!(" [{}]", crate::i18n::M::ReadOnly.tr(app.config.lang))
    } else {
        String::new()
    };

    let left_text = format!("{}{}{}", filename, read_only, match_count_str);

    // Shown for any live selection, not just while in 'v' selection mode: a
    // Shift+arrow selection leaves the state as Normal, so keying off the
    // state alone meant the size never appeared for it.
    //
    // The slot is always occupied - blanks when there is no selection - because
    // the block is right-aligned, so a field that appears and disappears pushes
    // everything to its left sideways. This one sits fifth, so marking a block
    // used to shift the IME, encoding, mode and file type fields all at once.
    let sel_len = app
        .hex_view
        .selection
        .end
        .saturating_sub(app.hex_view.selection.start);
    let selected_str = if app.state == UIState::HexSelection || sel_len > 0 {
        format!("[{:>4X}]", sel_len)
    } else {
        " ".repeat(SELECTION_WIDTH)
    };

    // Read only when the slot is going to be drawn: the query is a window-message
    // round-trip, and `:set han` is off by default.
    let ime_mode = if app.config.show_ime {
        crate::util::get_ime_language_mode()
    } else {
        ""
    };

    let show_va = app.hex_view.show_va;
    let is_64 = app.is_64();
    let cur_addr = if show_va { app.get_va(app.hex_view.offset) } else { app.hex_view.offset as u64 };
    // Right-aligned in a constant-width field. The original built the same
    // padding and then `.trim()`ed it away, which is what let this field - and
    // with it everything to its left in the right-hand block - move every time
    // the address gained or lost a digit. Keeping the pad preserves the original
    // digits (no added leading zeros) while pinning the width.
    let addr_fmt = if is_64 {
        format!("{:>9X}", cur_addr)
    } else {
        format!("{:>8X}", cur_addr)
    };

    // Pre-allocated single string buffer optimization (Allocation reduced from 8 to 1)
    // Every slot below has a fixed width, so no field ever moves: the block is
    // right-aligned, and anything that grows, shrinks or appears drags the
    // fields to its left along with it.
    //
    // The encoding label is the one exception, since the supported names run from
    // `GBK` to `windows-1252` and reserving for the longest would cost 25 columns
    // permanently. It sits second, so only the IME indicator is to its left.
    let mut right_text = String::with_capacity(128);
    right_text.push('│');
    if app.config.show_ime {
        let _ = write!(right_text, " {} │", ime_mode);
    }
    let _ = write!(right_text, " {} │", encoding_info);
    // Centred in its fixed-width slot: the labels run from `HEX` to `EDIT/UTF-8`,
    // and left-aligning short ones left a ragged gap before the `│` separator.
    let _ = write!(right_text, " {:^width$} │", mode_str, width = MODE_WIDTH);
    let _ = write!(
        right_text,
        " {:<width$} │",
        app.file_info.r#type,
        width = TYPE_WIDTH
    );
    let _ = write!(right_text, " {} │", selected_str);
    let _ = write!(right_text, " {} │", addr_fmt);
    let _ = write!(right_text, " {:3}% │", percent);

    let style = app.config.theme.topbar;

    // Paint the strip first so the gap between the two halves carries the bar's
    // own background rather than whatever the view drew underneath.
    frame.render_widget(Paragraph::new("").style(style), area);

    // One rect each. Rendering both halves into `area` with opposite alignments
    // made them overwrite each other once they no longer fit side by side; the
    // right-hand block is the one that must stay readable, so it keeps its width
    // and the file name gets clipped.
    let right_w = (right_text.chars().count() as u16).min(area.width);
    let zones = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_w)])
        .split(area);

    frame.render_widget(
        Paragraph::new(left_text)
            .style(style)
            .alignment(Alignment::Left),
        zones[0],
    );
    frame.render_widget(
        Paragraph::new(right_text)
            .style(style)
            .alignment(Alignment::Right),
        zones[1],
    );
}

#[cfg(test)]
mod status_bar_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).expect("terminal");
        terminal
            .draw(|f| {
                let area = Rect::new(0, 1, width, 1);
                app.screen = f.area();
                status_bar_draw(app, f, area);
            })
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..width).map(|x| buf[(x, 1)].symbol().to_string()).collect()
    }

    fn loaded_app() -> App {
        let mut app = App::new();
        let exe = std::env::current_exe().expect("exe path");
        app.load_file(exe.to_str().expect("utf-8 path"), 0x1a2f, true)
            .expect("load the test binary");
        app.file_info.name = "a_fairly_long_binary_name.exe".to_string();
        app
    }

    /// The two halves must not overwrite each other at any width.
    ///
    /// The right-hand block ends with the percentage, so finding that intact
    /// shows the block was laid out beside the file name rather than on top of
    /// it. With the old code the name ran straight through this area.
    #[test]
    fn halves_never_collide() {
        let mut app = loaded_app();
        for width in [200u16, 140, 120, 100, 80, 70, 60, 50, 40] {
            let line = render(&mut app, width);
            assert_eq!(
                line.chars().count(),
                width as usize,
                "line must fill exactly the bar width at {}",
                width
            );
            if width >= 70 {
                assert!(
                    line.trim_end().ends_with("% │"),
                    "right block was overwritten at width {}: {}",
                    width,
                    line
                );
            }
        }
    }

    /// Moving the cursor must not shift the right-hand block sideways.
    #[test]
    fn right_block_does_not_move_with_the_cursor() {
        let mut app = loaded_app();
        let mut first: Option<usize> = None;

        for ofs in [0x0usize, 0xff, 0x1a2f, 0x10000, 0x100000] {
            app.hex_view.offset = ofs;
            let line = render(&mut app, 140);
            // Position of the block's leading separator.
            let pos = line.find('│').expect("separator must be present");
            match first {
                None => first = Some(pos),
                Some(expected) => assert_eq!(
                    pos, expected,
                    "right block moved when the cursor did (offset {:X}): {}",
                    ofs, line
                ),
            }
        }
    }

    /// 100% must mean the end of the file, not "close enough to round up".
    #[test]
    fn percent_is_floored() {
        let mut app = loaded_app();
        app.file_info.size = 1000;

        app.hex_view.offset = 999;
        assert!(
            render(&mut app, 140).contains(" 99% │"),
            "999/1000 must read 99%: {}",
            render(&mut app, 140)
        );

        app.hex_view.offset = 1000;
        assert!(
            render(&mut app, 140).contains("100% │"),
            "1000/1000 must read 100%: {}",
            render(&mut app, 140)
        );
    }
}

#[cfg(test)]
mod slot_width_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).expect("terminal");
        terminal
            .draw(|f| {
                let area = Rect::new(0, 1, width, 1);
                app.screen = f.area();
                status_bar_draw(app, f, area);
            })
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..width).map(|x| buf[(x, 1)].symbol().to_string()).collect()
    }

    fn loaded_app() -> App {
        let mut app = App::new();
        let exe = std::env::current_exe().expect("exe path");
        app.load_file(exe.to_str().expect("utf-8 path"), 0x1a2f, true)
            .expect("load the test binary");
        app.file_info.name = "lovestring_ori.exe".to_string();
        app.file_info.is_read_only = false;
        app
    }

    /// Column positions of the block's separators. Every field boundary in one
    /// list, so any field that changes width shows up as a different vector.
    fn separator_columns(line: &str) -> Vec<usize> {
        line.chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| i)
            .collect()
    }

    /// Marking, resizing and clearing a block must not move any field.
    ///
    /// The selection readout is the fifth field in a right-aligned block, so when
    /// it was only emitted while a selection existed, marking a block shifted the
    /// IME, encoding, mode and file type fields all at once.
    #[test]
    fn selection_does_not_shift_the_other_fields() {
        let mut app = loaded_app();
        let baseline = separator_columns(&render(&mut app, 120));

        // A large block.
        app.hex_view.selection.start = 0x1a2f;
        app.hex_view.selection.end = 0x1acf;
        assert_eq!(
            separator_columns(&render(&mut app, 120)),
            baseline,
            "marking a block moved the other fields"
        );

        // A one-byte block: fewer digits, same slot.
        app.hex_view.selection.end = 0x1a30;
        assert_eq!(
            separator_columns(&render(&mut app, 120)),
            baseline,
            "a shorter selection length moved the other fields"
        );

        // Selection mode with no length yet.
        app.state = UIState::HexSelection;
        assert_eq!(
            separator_columns(&render(&mut app, 120)),
            baseline,
            "entering selection mode moved the other fields"
        );

        // Cleared again.
        app.state = UIState::Normal;
        app.hex_view.selection.end = app.hex_view.selection.start;
        assert_eq!(
            separator_columns(&render(&mut app, 120)),
            baseline,
            "clearing the selection moved the other fields"
        );
    }

    /// The mode names seen while a view is on screen all fit their slot, so
    /// switching view or entering edit mode doesn't move anything either.
    #[test]
    fn view_modes_fit_the_mode_slot() {
        let mut app = loaded_app();
        let baseline = separator_columns(&render(&mut app, 120));

        for state in [UIState::HexSelection, UIState::HexEditing] {
            app.state = state;
            assert_eq!(
                separator_columns(&render(&mut app, 120)),
                baseline,
                "a mode change moved the other fields"
            );
        }

        app.state = UIState::Normal;
        for view in [
            crate::editor::AppView::Disasm,
            crate::editor::AppView::Header,
            crate::editor::AppView::Text,
            crate::editor::AppView::Hex,
        ] {
            app.editor_view = view;
            assert_eq!(
                separator_columns(&render(&mut app, 120)),
                baseline,
                "switching to {:?} moved the other fields",
                view
            );
        }
    }

    /// The IME slot is absent unless `:set han` asks for it.
    ///
    /// Two columns of `EN` mean nothing to someone who does not type through an
    /// IME, and reading the mode costs a window-message round-trip to the IME
    /// process - so the default is no slot and no query.
    #[test]
    fn the_ime_slot_is_off_by_default() {
        let mut app = loaded_app();
        assert!(!app.config.show_ime);

        let without = render(&mut app, 120);
        assert!(
            !without.contains("EN") && !without.contains("Han"),
            "the indicator is on screen with the option off: {:?}",
            without
        );

        app.config.show_ime = true;
        let with = render(&mut app, 120);
        assert!(
            with.contains("EN") || with.contains("Han"),
            "':set han on' did not bring the indicator back: {:?}",
            with
        );
        // The slot takes room from the left half rather than pushing anything off
        // the right edge.
        assert_eq!(with.chars().count(), without.chars().count());
    }

    /// The reserved widths must actually be wide enough for what goes in them.
    #[test]
    fn slots_fit_their_contents() {
        assert_eq!(format!("[{:>4X}]", 0xFFFFu32).chars().count(), SELECTION_WIDTH);
        for label in ["PE", "PE64", "ELF", "RAW"] {
            assert!(label.chars().count() <= TYPE_WIDTH, "'{}' exceeds slot", label);
        }
        for label in ["HEX", "SELECT", "DISASM", "HEADER", "TEXT", "EDIT/HEX"] {
            assert!(label.chars().count() <= MODE_WIDTH, "'{}' exceeds slot", label);
        }
    }
}
