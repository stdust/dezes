mod app;
mod commands;
mod config;
mod database;
mod disasm;
mod draw;
mod editor;
mod events;
mod file_dialog;
mod global;
mod goto_dialog;
mod header;
mod hex;
mod i18n;
mod initfile;
mod input_history;
mod reader;
mod ruler;
mod text;
mod text_field;
mod themes;
mod util;
mod widgets;

use std::io::{BufWriter, Stdout};
use std::time::Duration;

use clap::Parser;
use ratatui::crossterm::{event, terminal};

use app::App;

/// Rows reserved around the hex/disasm content: ruler + status bar + command bar.
const CHROME_ROWS: u16 = 3;

/// Console writes made since the counters were last read, and how long they took.
///
/// Read by the slow-frame log. Two chases after an intermittent stutter both ended
/// at "the terminal took 100 ms", which is not an answer: it could be the console
/// refusing output, or work inside `Terminal::draw` that is neither building widgets
/// nor writing. Splitting the write out settles it.
static WRITE_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WRITE_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Wraps the console handle to time every syscall made against it.
///
/// Sits *inside* the `BufWriter`, so what is measured is the real writes - one per
/// frame - rather than the buffered copies into memory.
struct TimedWriter<W> {
    inner: W,
}

impl<W: std::io::Write> std::io::Write for TimedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let started = std::time::Instant::now();
        let written = self.inner.write(buf);
        WRITE_NANOS.fetch_add(
            started.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        if let Ok(n) = written {
            WRITE_BYTES.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        }
        written
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let started = std::time::Instant::now();
        let result = self.inner.flush();
        WRITE_NANOS.fetch_add(
            started.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        result
    }
}

/// The terminal, writing through a buffer.
type Tui = ratatui::Terminal<
    ratatui::backend::CrosstermBackend<BufWriter<TimedWriter<Stdout>>>,
>;

/// Sets up the terminal with a *buffered* writer.
///
/// `ratatui::init()` hands `CrosstermBackend` a bare `Stdout`, and crossterm writes
/// each escape sequence to it as it goes: on Windows every one of those is a
/// separate console write. A frame that scrolled the Header view measured 2399
/// write calls for 4 KB of output, and a full repaint far more - which is where
/// the intermittent stutter came from. The `Slow frame` log put it beyond doubt by
/// splitting the frame in two: building the widgets took under a millisecond while
/// the terminal took 100 ms and occasionally a whole second.
///
/// A `BufWriter` turns those thousands of calls into one per frame, since
/// `CrosstermBackend::flush` (which `Terminal::draw` calls at the end of every
/// frame) flushes the writer.
fn init_terminal() -> std::io::Result<Tui> {
    set_panic_hook();
    terminal::enable_raw_mode()?;
    ratatui::crossterm::execute!(
        std::io::stdout(),
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture
    )?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(
        BufWriter::new(TimedWriter {
            inner: std::io::stdout(),
        }),
    ))?;
    terminal.clear()?;
    Ok(terminal)
}

/// Puts the console back the way it was found.
fn restore_terminal() {
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        event::DisableMouseCapture,
        terminal::LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show
    );
    let _ = terminal::disable_raw_mode();
}

/// Restores the console before the default panic message is printed.
///
/// `ratatui::init()` does this for us; hand-rolling the setup means hand-rolling
/// this too, or a panic leaves the terminal in raw mode on the alternate screen
/// with no cursor and the message nowhere to be seen.
fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        hook(info);
    }));
}

/// Fast terminal hex editor and x86 disassembler
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// File to open
    file: Option<String>,

    /// Initial cursor offset (hex default; `t` suffix = decimal)
    #[arg(short, long, default_value = "0")]
    offset: String,

    /// Set read-only mode
    #[arg(short, long)]
    readonly: bool,
}

fn main() {
    let args = Args::parse();
    let mut app = App::new();
    let cursor_offset = util::parse_offset(&args.offset).unwrap_or_default();

    // read init file ignoring errors
    let _ = app.read_initfile();

    if let Some(ref filename) = args.file {
        if let Err(e) = app.load_file(filename, cursor_offset, args.readonly) {
            App::log(&mut app, format!("Failed to open {}: {}", filename, e));
            app.open_file_dialog();
        }
    } else {
        app.open_file_dialog();
    }

    app.list_state.select_first();

    let mut terminal = match init_terminal() {
        Ok(terminal) => terminal,
        Err(e) => {
            eprintln!("dezes: unable to set up the terminal: {}", e);
            return;
        }
    };

    // Any fatal loop error is reported *after* the terminal has been restored,
    // otherwise a panic here leaves the console in raw mode with no cursor.
    let mut fatal: Option<String> = None;

    // Idle repaint interval. Nothing on screen changes without an event except the
    // IME indicator and the Ctrl/Alt hint page, both of which are polled rather than
    // delivered as events. 120 ms keeps holding Ctrl feeling instant while cutting
    // idle repaints by more than half.
    const IDLE_REDRAW: Duration = Duration::from_millis(120);
    // Frames slower than this go in the log with their cost, so an intermittent
    // stall leaves evidence behind instead of being unreproducible.
    const SLOW_FRAME: Duration = Duration::from_millis(50);
    let mut last_draw = std::time::Instant::now() - IDLE_REDRAW;

    while app.running {
        // 1. Block and handle first incoming event
        let had_event = match events::handle_events(&mut app) {
            Ok(handled) => handled,
            Err(e) => {
                fatal = Some(format!("unable to read events: {}", e));
                break;
            }
        };
        let mut dirty = had_event;

        // 2. Drain all pending batch events (e.g. Windows IME committed character + arrow key)
        while event::poll(Duration::ZERO).unwrap_or(false) {
            match events::handle_events(&mut app) {
                Ok(handled) => dirty |= handled,
                Err(e) => {
                    fatal = Some(format!("unable to read events: {}", e));
                    break;
                }
            }
            if !app.running {
                break;
            }
        }

        if fatal.is_some() || !app.running {
            break;
        }

        // Redrawing on every 50 ms poll timeout meant a full-screen diff and terminal
        // write 20 times a second even while idle. On a large window in a slow
        // terminal those writes queue up, and keystrokes then wait behind them - which
        // is what made scrolling stutter for no apparent reason. Frames are now drawn
        // when something actually happened, plus a slow idle tick for the polled
        // indicators.
        // The easter egg animates, so it needs frames of its own; everything else
        // is drawn on demand.
        let redraw_interval = if app.state == editor::UIState::Matrix {
            global::matrix::FRAME_INTERVAL
        } else {
            IDLE_REDRAW
        };
        if !dirty && last_draw.elapsed() < redraw_interval {
            continue;
        }
        let frame_started = std::time::Instant::now();
        // Timed separately from the frame as a whole: building the widgets is our
        // code, writing the result out is the terminal's. A slow frame with a fast
        // build is a console that cannot keep up with a full-screen repaint, and
        // no amount of optimizing the draw code would move it.
        let mut build_time = std::time::Duration::ZERO;
        WRITE_NANOS.store(0, std::sync::atomic::Ordering::Relaxed);
        WRITE_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);

        // 3. Render frame only after all pending events are processed into app state
        let draw_result = terminal
            .draw(|f| {
                // Page size is dynamically calculated as:
                // frame height - (command line + status line + header) * bytes per line

                // Prevent panic on underflow with small screen sizes
                // Currently, we can't have them because of widgets such as Calculator,
                // but we might add support for such small screen sizes in the future
                let bytes_per_line = app.config.hex_mode_bytes_per_line.max(1);
                let page_size =
                    (f.area().height.saturating_sub(CHROME_ROWS)).max(1) as usize * bytes_per_line;

                if page_size != app.reader.page_current_size {
                    app.reader.page_current_size = page_size;
                    // `wrapping_sub` on a zero page_size produced usize::MAX.
                    app.reader.page_end = app
                        .reader
                        .page_start
                        .saturating_add(page_size)
                        .saturating_sub(1);
                }
                app.screen = f.area();
                let build_started = std::time::Instant::now();
                draw::draw(f, &mut app);
                build_time = build_started.elapsed();
            });

        if let Err(e) = draw_result {
            fatal = Some(format!("failed to draw frame: {}", e));
            break;
        }

        last_draw = std::time::Instant::now();
        let frame_time = frame_started.elapsed();
        if frame_time > SLOW_FRAME {
            let view = app.editor_view;
            let write_time = std::time::Duration::from_nanos(
                WRITE_NANOS.load(std::sync::atomic::Ordering::Relaxed),
            );
            let write_bytes = WRITE_BYTES.load(std::sync::atomic::Ordering::Relaxed);
            App::log(
                &mut app,
                format!(
                    "Slow frame: {:?} (build {:?}, write {:?} for {} bytes, rest {:?}) (view {:?})",
                    frame_time,
                    build_time,
                    write_time,
                    write_bytes,
                    frame_time
                        .saturating_sub(build_time)
                        .saturating_sub(write_time),
                    view
                ),
            );
        }
    }

    // Annotations are flushed here rather than in each quit handler, so every way
    // out of the loop covers them: `:q`, `:wq`, F12, Esc in the file dialog, and
    // a fatal error. Saving used to hang off successful *file writes* only, which
    // meant `:q` threw the session's comments away and a read-only file could
    // never keep any.
    let annotations_error = app.persist_annotations();

    // Flushed and dropped before the console is restored, so nothing buffered ends
    // up printed over the shell after the alternate screen is gone.
    let _ = terminal.flush();
    drop(terminal);
    restore_terminal();

    if let Some(message) = annotations_error {
        eprintln!("dezes: {}", message);
    }

    if let Some(message) = fatal {
        eprintln!("dezes: {}", message);
        std::process::exit(1);
    }
}

#[macro_export]
macro_rules! beep {
    () => {
        print!("\x07")
    };
}

#[cfg(test)]
mod terminal_tests {
    /// The terminal must keep writing through a buffer.
    ///
    /// Compile-time only: the coercion below fails if `Tui` stops being a
    /// `BufWriter`. Handing `CrosstermBackend` a bare `Stdout` costs a console
    /// write per escape sequence - measured at 2740 of them for one Hex frame -
    /// and that is what the intermittent scroll stutter was.
    #[test]
    fn the_writer_is_buffered() {
        fn only_accepts_buffered(
            _: ratatui::Terminal<
                ratatui::backend::CrosstermBackend<
                    std::io::BufWriter<super::TimedWriter<std::io::Stdout>>,
                >,
            >,
        ) {
        }
        let _: fn(super::Tui) = only_accepts_buffered;
    }
}