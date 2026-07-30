use crate::themes::*;
use crate::disasm::theme::DisasmTheme;

// command input history size
pub const CMD_INPUT_HIST_SIZE: usize = 50;

pub struct Config {
    pub database: bool,
    pub dim_control_chars: bool,
    pub dim_zeroes: bool,
    pub hex_mode_bytes_per_line: usize,
    pub hex_mode_bytes_per_line_auto: bool,
    pub hex_mode_non_graphic_char: char,
    pub maximum_strings_to_show: usize,
    pub minimum_string_length: usize,
    pub search_wrap: bool,
    /// Interface language (`:set lang en|ko|zh`).
    ///
    /// Only labels move: key names, option names and the status-bar mode labels are
    /// identifiers shared with the documentation, so they stay as they are.
    pub lang: crate::i18n::Lang,
    /// Show the context hint line on the command-bar row (`:set hintbar off` to
    /// hide it).
    ///
    /// On by default: the shortcut set is large enough that a new user has no way
    /// in without it, and it costs no screen space - the row it uses is empty
    /// unless the command line or a message needs it.
    pub hint_bar: bool,
    /// Decoding width forced by the user: 16, 32 or 64. `None` means "take it from
    /// the header".
    ///
    /// The same bytes decode to different instructions per width - `48 89 E5` is one
    /// `mov rbp, rsp` at 64 bits but `dec eax` + `mov ebp, esp` at 32 - so getting
    /// this wrong desynchronises the whole listing. A header is not always right or
    /// even present: a PE's DOS stub is 16-bit real-mode code inside a 64-bit image,
    /// and a raw shellcode dump has nothing to declare a width at all.
    pub bitness_override: Option<u32>,
    pub syntax_highlight: bool,
    /// Show the IME conversion-mode indicator (`EN` / `Han`) in the status bar.
    ///
    /// Off by default, and deliberately absent from the `:set` table, the settings
    /// dialog and the help: the indicator only means anything to someone typing
    /// with a Korean, Japanese or Chinese IME, and reading it costs a window-message
    /// round-trip to the IME process. With this off, that query never happens and
    /// the poller thread is never started.
    pub show_ime: bool,
    pub theme: Theme,
    pub disasm_theme: DisasmTheme,
    // pub hex_mode_dword_separator: char,
    // pub text_mode_tab_spaces: usize,
}
