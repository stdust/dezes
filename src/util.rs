use std::num::ParseIntError;

use ratatui::layout::Rect;

/// This function is used returns the right offset
/// for goto(). Hexa is the default. Add 't' suffix for decimal
pub fn parse_offset(expr: &str) -> Result<usize, ParseIntError> {
    // `strip_suffix` is char-boundary safe, unlike `expr[0..expr.len() - 1]`,
    // which panics on input such as "가t".
    if let Some(decimal) = expr.strip_suffix('t') {
        decimal.parse()
    } else {
        usize::from_str_radix(expr, 16)
    }
}

pub fn center_widget(width: u16, height: u16, area: Rect) -> Rect {
    // Clamp the requested size to the available area *before* computing the
    // origin, otherwise an oversized dialog (e.g. the 68x8 calculator on a
    // narrow terminal) gets positioned partially off-screen.
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Directory the process was started in.
///
/// The file dialog calls `std::env::set_current_dir` on every directory it
/// navigates into, so anything resolving a CWD-relative config path
/// (`.dz6init`, `themes/`, `disasm.theme`, `<file>.dz6`) would otherwise
/// read - and *write* - inside whatever directory the user happened to browse
/// last. This is captured on first use, which happens during startup, before any
/// navigation is possible.
pub fn startup_dir() -> &'static std::path::Path {
    static STARTUP_DIR: std::sync::LazyLock<std::path::PathBuf> = std::sync::LazyLock::new(|| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });
    STARTUP_DIR.as_path()
}

/// Directory the dz6 executable itself lives in.
///
/// This is the anchor for the colour configs (`themes/`, `disasm.theme`): they
/// belong to the install, not to whatever directory the user happened to launch
/// dz6 from. Anchoring them to [`startup_dir`] instead meant that opening a file
/// from a new folder created a `themes/` there and filled it with the built-in
/// defaults, so the same file rendered in different colours depending on where
/// dz6 was started.
///
/// Falls back to the startup directory if the executable path can't be resolved.
pub fn exe_dir() -> &'static std::path::Path {
    static EXE_DIR: std::sync::LazyLock<std::path::PathBuf> = std::sync::LazyLock::new(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| startup_dir().to_path_buf())
    });
    EXE_DIR.as_path()
}

/// Largest bytes-per-line value that still fits in `terminal_width`.
///
/// The layout budget per byte is 3 columns in the hex dump plus 1 in the text
/// column, on top of the ~9 columns of address gutter and separators - hence
/// `(width - 9) / 4`. Both underflows of the old inline version are handled
/// here: a terminal narrower than 9 columns, and the `- 1` when the division
/// yields 0.
pub fn max_bytes_per_line(terminal_width: u16) -> usize {
    let slots = (terminal_width.saturating_sub(9) / 4) as usize;
    slots.saturating_sub(1).max(1)
}

/// Encodes `text` with `enc`, handling the UTF-16 variants itself.
///
/// `Encoding::encode` cannot produce UTF-16: encoding_rs maps UTF-16LE and
/// UTF-16BE to UTF-8 on the way out, so `UTF_16LE.encode("分析")` returns the
/// five UTF-8 bytes `E5 88 86 E6 9E 90` rather than `06 52 90 67` - silently, with
/// no error to notice. Anything that turns typed text into bytes has to come
/// through here or it searches for, and writes, the wrong bytes.
pub fn encode_text(text: &str, enc: &'static encoding_rs::Encoding) -> Vec<u8> {
    match enc.name() {
        "UTF-16LE" => text
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect(),
        "UTF-16BE" => text
            .encode_utf16()
            .flat_map(|unit| unit.to_be_bytes())
            .collect(),
        _ => {
            let (bytes, _, _) = enc.encode(text);
            bytes.into_owned()
        }
    }
}

pub fn encode_char(c: char, enc: &'static encoding_rs::Encoding) -> Vec<u8> {
    match enc.name() {
        "UTF-16LE" => {
            let mut buf = [0u16; 2];
            let u16_slice = c.encode_utf16(&mut buf);
            let mut bytes = Vec::new();
            for &val in u16_slice.iter() {
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            bytes
        }
        "UTF-16BE" => {
            let mut buf = [0u16; 2];
            let u16_slice = c.encode_utf16(&mut buf);
            let mut bytes = Vec::new();
            for &val in u16_slice.iter() {
                bytes.extend_from_slice(&val.to_be_bytes());
            }
            bytes
        }
        _ => {
            let c_str = c.to_string();
            let (encoded_bytes, _, has_unmappable) = enc.encode(&c_str);
            if has_unmappable {
                vec![b'?']
            } else {
                encoded_bytes.into_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_expr_test() {
        assert_eq!(Ok(255), parse_offset("ff"));
        assert_eq!(Ok(16), parse_offset("10"));
        assert_eq!(Ok(255), parse_offset("ff"));
        assert_eq!(Ok(255), parse_offset("255t"));
        // Errors
        assert!(parse_offset("255th").is_err());
        assert!(parse_offset("255ht").is_err());
        assert!(parse_offset("ht").is_err());
        assert!(parse_offset("h3").is_err());
        assert!(parse_offset("-5").is_err());
        assert!(parse_offset("4h4h").is_err());
    }
}

/// How often the poller thread asks the IME what mode it is in.
#[cfg(target_os = "windows")]
const IME_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

/// Last known IME conversion mode: true for native (Han), false for alphanumeric.
#[cfg(target_os = "windows")]
static IME_NATIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The indicator for the status bar, as a plain atomic read.
///
/// The query itself does a blocking window-message round-trip with a 20 ms
/// timeout, and it used to happen inside the frame - throttled to once every
/// 400 ms, but on the render thread all the same. The `Slow frame` log showed it:
/// frames alternated between a 0.6 ms build and a 20-35 ms one, in step with the
/// throttle. A poller thread owns the round-trip now, so a stuck IME costs the
/// frame nothing and the indicator is at most one interval stale.
#[cfg(target_os = "windows")]
pub fn get_ime_language_mode() -> &'static str {
    use std::sync::Once;
    use std::sync::atomic::Ordering;

    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::Builder::new()
            .name("ime-poll".to_string())
            .spawn(|| {
                loop {
                    let native = query_ime_language_mode() == "Han";
                    IME_NATIVE.store(native, Ordering::Relaxed);
                    std::thread::sleep(IME_POLL_INTERVAL);
                }
            })
            .ok();
    });

    if IME_NATIVE.load(Ordering::Relaxed) {
        "Han"
    } else {
        "EN"
    }
}

#[cfg(target_os = "windows")]
fn query_ime_language_mode() -> &'static str {
    #[link(name = "imm32")]
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut std::ffi::c_void;
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
        fn ImmGetDefaultIMEWnd(hWnd: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn SendMessageTimeoutA(
            hWnd: *mut std::ffi::c_void,
            Msg: u32,
            wParam: usize,
            lParam: isize,
            fuFlags: u32,
            uTimeout: u32,
            lpdwResult: *mut usize,
        ) -> isize;
    }

    const WM_IME_CONTROL: u32 = 0x0283;
    const IMC_GETCONVERSIONMODE: usize = 0x0001;
    const IME_CMODE_NATIVE: usize = 0x0001;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    const SMTO_BLOCK: u32 = 0x0001;
    /// Long enough for a responsive IME, short enough not to be felt as a stutter.
    const TIMEOUT_MS: u32 = 20;

    unsafe {
        // Foreground window first, console window as the fallback.
        //
        // It was the other way round to keep a synchronous round-trip off the render
        // thread, but a poller thread owns that call now, so there is nothing left to
        // protect - and under Windows Terminal the console window's default IME
        // window does not carry the conversion mode, so the indicator was stuck on
        // `EN` however the keyboard was switched.
        let mut hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            hwnd = GetConsoleWindow();
        }
        if hwnd.is_null() {
            return "EN";
        }
        let def_ime = ImmGetDefaultIMEWnd(hwnd);
        if def_ime.is_null() {
            return "EN";
        }

        // `SendMessageTimeoutA`, not `SendMessageA`: an IME that is mid-switch or a
        // window whose message loop is stuck would otherwise hold the frame for as
        // long as it liked. On timeout the indicator simply keeps its last value.
        let mut result: usize = 0;
        let ok = SendMessageTimeoutA(
            def_ime,
            WM_IME_CONTROL,
            IMC_GETCONVERSIONMODE,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            TIMEOUT_MS,
            &mut result,
        );
        if ok != 0 && (result & IME_CMODE_NATIVE) != 0 {
            return "Han";
        }
    }
    "EN"
}

#[cfg(not(target_os = "windows"))]
pub fn get_ime_language_mode() -> &'static str {
    "EN"
}

/// True when `re` matches at least one *non-empty* stretch of `text`.
///
/// `Regex::is_match` counts a zero-length match, and that is not what a filter
/// means. `([一-龥]*?)` is the case that made this obvious: `*?` is "zero or more,
/// lazily", so the shortest answer at every position is zero characters - measured,
/// it never matches a single hanzi even in `反编译失败`, yet `is_match` returns true
/// for every row including the English ones. Every engine behaves that way
/// (Python's `re.search`, JS `.test()`, PCRE all return a zero-length match at
/// position 0); what differs is whether the host program counts it. A find or
/// highlight tool discards empty matches because there is nothing to point at, and
/// a list filter is the same kind of question.
///
/// The visible consequences: `[a-z]*` now behaves like `[a-z]+`, and a pattern that
/// can only ever match nothing selects nothing instead of everything.
pub fn has_nonempty_match(re: &regex::Regex, text: &str) -> bool {
    re.find_iter(text).any(|m| !m.is_empty())
}
