//! Tests for the forced decoding width (Alt+F7 / `:set bitness`).
//!
//! The feature itself is three lines in `App::bitness`/`is_64`; what needs holding
//! down is that the choice actually reaches the decoders, the row cache and the
//! address column, and that it is dropped when another file is opened.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// `48 89 E5` - one instruction at 64 bits, two at 32.
    ///
    /// The clearest demonstration that the width is not cosmetic: `0x48` is a REX.W
    /// prefix in long mode but a standalone `dec eax` in 32-bit mode, so every
    /// instruction boundary after it shifts.
    const CODE: &[u8] = &[0x48, 0x89, 0xE5];

    fn app_with_code(bytes: &[u8]) -> App {
        let dir = std::env::temp_dir().join("dz6_bitness");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("code_{n}.bin"));
        let mut blob = bytes.to_vec();
        blob.resize(0x200, 0x90);
        std::fs::write(&path, &blob).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        app
    }

    fn first_instruction(app: &App, offset: usize) -> (String, usize) {
        let buffer = app.file_info.get_buffer_ref();
        let end = (offset + 16).min(buffer.len());
        let decoder = Decoder::with_ip(
            app.bitness(),
            &buffer[offset..end],
            app.get_va(offset),
            DecoderOptions::NONE,
        );
        let instr = decoder.into_iter().next().expect("decodes");
        let mut text = String::new();
        IntelFormatter::new().format(&instr, &mut text);
        (text, instr.len())
    }

    /// The cycle order is auto -> 16 -> 32 -> 64 -> auto.
    #[test]
    fn the_cycle_visits_every_width_and_returns_to_auto() {
        let mut app = app_with_code(CODE);
        assert!(app.config.bitness_override.is_none(), "starts on auto");

        app.cycle_bitness();
        assert_eq!(app.config.bitness_override, Some(16));
        app.cycle_bitness();
        assert_eq!(app.config.bitness_override, Some(32));
        app.cycle_bitness();
        assert_eq!(app.config.bitness_override, Some(64));
        app.cycle_bitness();
        assert!(
            app.config.bitness_override.is_none(),
            "the cycle must come back to auto, or there is no way to undo it"
        );
    }

    /// The forced width has to reach the decoder, not just be stored.
    #[test]
    fn the_forced_width_changes_the_decoding() {
        let mut app = app_with_code(CODE);

        app.config.bitness_override = Some(64);
        let (at_64, len_64) = first_instruction(&app, 0);
        assert_eq!(len_64, 3, "48 89 E5 is one 3-byte instruction at 64 bits");
        assert!(
            at_64.contains("rbp") && at_64.contains("rsp"),
            "expected a 64-bit register form, got '{at_64}'"
        );

        app.config.bitness_override = Some(32);
        let (at_32, len_32) = first_instruction(&app, 0);
        assert_eq!(len_32, 1, "0x48 is a standalone `dec eax` at 32 bits");
        assert!(
            at_32.contains("eax"),
            "expected `dec eax`, got '{at_32}'"
        );

        app.config.bitness_override = Some(16);
        let (at_16, _) = first_instruction(&app, 0);
        assert_ne!(at_16, at_32, "16-bit decoding must differ from 32-bit");
    }

    /// `bitness()` must report the real width, and `is_64()` follow it.
    ///
    /// `bitness()` used to be derived from `is_64()`, which cannot express 16 - both
    /// 16 and 32 are "not 64".
    #[test]
    fn bitness_reports_sixteen_and_is_64_agrees() {
        let mut app = app_with_code(CODE);

        for bits in App::BITNESS_CHOICES {
            app.config.bitness_override = Some(bits);
            assert_eq!(app.bitness(), bits, "bitness() must report the forced width");
            assert_eq!(
                app.is_64(),
                bits == 64,
                "is_64() drives the address column width and must agree"
            );
        }
    }

    /// The address column narrows with the width, since it also formats the VA.
    #[test]
    fn the_address_column_follows_the_width() {
        let mut app = app_with_code(CODE);

        app.config.bitness_override = Some(64);
        let wide = app.get_addr_col_width();
        app.config.bitness_override = Some(16);
        let narrow = app.get_addr_col_width();

        assert!(
            narrow < wide,
            "a 16-bit view must not reserve room for 64-bit addresses ({narrow} vs {wide})"
        );
    }

    /// A forced width belongs to the image it was chosen for.
    #[test]
    fn opening_another_file_returns_to_auto() {
        let mut app = app_with_code(CODE);
        app.config.bitness_override = Some(16);

        let other = app_with_code(CODE);
        let path = other.file_info.path.clone();
        drop(other);
        app.load_file(&path, 0, true).expect("open another");

        assert!(
            app.config.bitness_override.is_none(),
            "carrying the width over would decode the next file wrongly with no visible cause"
        );
    }

    /// Alt+F7 is what drives the cycle, and bare F7 must still switch to Text view.
    ///
    /// The bare `KeyCode::F(7)` arm matched regardless of modifiers, so it swallowed
    /// Alt+F7 and the cycle was unreachable - the compiler called the new arm
    /// unreachable, which is how this was caught.
    #[test]
    fn alt_f7_cycles_and_bare_f7_still_switches_view() {
        use ratatui::crossterm::event::{
            KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };

        let press = |app: &mut App, modifiers: KeyModifiers| {
            let key = KeyEvent {
                code: KeyCode::F(7),
                modifiers,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            let _ = crate::global::events::handle_global_events(app, key);
        };

        let mut app = app_with_code(CODE);
        let view_before = app.editor_view;

        press(&mut app, KeyModifiers::ALT);
        assert_eq!(
            app.config.bitness_override,
            Some(16),
            "Alt+F7 must cycle the decoding width"
        );
        assert_eq!(
            app.editor_view, view_before,
            "Alt+F7 must not also switch to the Text view"
        );

        press(&mut app, KeyModifiers::NONE);
        assert_eq!(
            app.editor_view,
            crate::editor::AppView::Text,
            "bare F7 must still switch to the Text view"
        );
        assert_eq!(
            app.config.bitness_override,
            Some(16),
            "bare F7 must not touch the width"
        );
    }

    /// `:set bitness` accepts the three widths and `auto`, and refuses anything else.
    #[test]
    fn the_set_command_validates_the_width() {
        let mut app = app_with_code(CODE);

        crate::commands::parse_command(&mut app, "set bitness 16");
        assert_eq!(app.config.bitness_override, Some(16));

        crate::commands::parse_command(&mut app, "set bitness auto");
        assert!(app.config.bitness_override.is_none());

        crate::commands::parse_command(&mut app, "set bitness 64");
        assert_eq!(app.config.bitness_override, Some(64));

        // 48 is not a thing; the current setting must survive a bad value.
        crate::commands::parse_command(&mut app, "set bitness 48");
        assert_eq!(
            app.config.bitness_override,
            Some(64),
            "an invalid width must leave the current one alone"
        );
    }
}
