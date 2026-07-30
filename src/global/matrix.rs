//! Digital rain. An easter egg, reached by a command that is in no help text.
//!
//! Deliberately self-contained: it owns a `UIState`, draws over the whole frame
//! and returns to Normal on any key. Nothing here touches the file, the cursor or
//! any setting, so the worst it can do is waste a few frames of someone's time.
//!
//! The glyphs are the bytes of the file that is open. A hex editor raining its own
//! contents is the version of this joke worth writing, and it makes every file look
//! a little different - a text file rains words, an executable rains the printable
//! debris between its opcodes.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::{Frame, crossterm::event::KeyEvent};
use std::io::Result;

use crate::{app::App, editor::UIState};

/// Frame interval while the rain is running.
///
/// Deliberately shorter than the 50 ms event poll: the loop wakes on that poll, and
/// an interval longer than it would mean a frame every *other* wake-up - 10 frames
/// a second, which reads as a stutter. Below the poll, every wake-up draws, so the
/// rain runs at about 20 frames a second and costs nothing while it is not on
/// screen.
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);

/// Glyphs used when the file has nothing printable to offer (or none is open).
///
/// Hex digits, because that is what this program is for.
const FALLBACK_GLYPHS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
];

/// The backdrop: true black, not the terminal's palette black.
///
/// `Color::Black` is whatever the terminal calls black - usually a dark grey
/// (Windows Terminal ships 0x0C0C0C), which left the picture looking washed out
/// next to the film.
const BACKDROP: Color = Color::Rgb(0, 0, 0);

/// Dimmest green a glyph is drawn in.
///
/// The fade used to run all the way to zero, which was survivable against a grey
/// backdrop and invisible against a black one - the end of every trail simply
/// disappeared. The tail is faint now, not absent.
const TAIL_LEVEL: f32 = 70.0;

/// Brightest green, for the glyph just behind the white head.
const HEAD_LEVEL: f32 = 255.0;

/// Half-width katakana: the film's alphabet, for terminals whose font has it.
///
/// Half-width (U+FF66..U+FF9D) rather than the usual full-width forms, because a
/// full-width glyph takes two cells and a column of them would be twice as wide as
/// the grid it is falling through. Opt-in - `:matrix kana` - since a font without
/// these draws a row of replacement boxes instead.
fn katakana_glyphs() -> Vec<char> {
    let mut glyphs: Vec<char> = ('\u{FF66}'..='\u{FF9D}').collect();
    // A few digits mixed in, as in the film.
    glyphs.extend('0'..='9');
    glyphs
}

/// Where the falling glyphs come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphSource {
    /// The printable bytes of the open file. A hex editor raining its own contents
    /// is the version of the joke worth writing, and every file looks different.
    #[default]
    File,
    /// Half-width katakana, for a terminal whose font can draw them.
    Kana,
    /// Hex digits only.
    Hex,
}

impl GlyphSource {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value.map(str::trim) {
            None | Some("") | Some("file") => Some(GlyphSource::File),
            Some("kana") | Some("katakana") => Some(GlyphSource::Kana),
            Some("hex") => Some(GlyphSource::Hex),
            _ => None,
        }
    }
}

/// Fraction of columns that are falling at any one time.
///
/// Every column used to hold a drop, which filled the screen from edge to edge with
/// no gaps in it - denser than the film, where the rain is sparse enough to see
/// black between the streaks. A column that finishes now waits before it starts
/// again, so the gaps move around instead of sitting in fixed stripes.
///
/// Tuned by eye against the film, in two passes: every column, then 70%, then this.
const DUTY: f32 = 0.55;

/// Multiplier on the fall speed.
///
/// The first pass fell fast enough that a trail crossed the screen in a couple of
/// seconds, which reads as static rather than as rain. Cut to 65% of that, then by
/// a further quarter.
const SPEED_SCALE: f32 = 0.49;

/// How far behind the head a glyph may be redrawn as a different character.
///
/// In the film the trail flickers: characters keep changing after they have fallen.
/// Only the first few are churned, so the tail stays readable as a streak rather
/// than boiling all over.
const CHURN_DEPTH: usize = 6;

/// One falling column.
struct Drop {
    /// Row of the leading glyph, fractional so columns can fall at different
    /// speeds without snapping to whole rows.
    head: f32,
    /// Rows per frame.
    speed: f32,
    /// Length of the trail behind the head.
    len: u16,
    /// The glyphs currently in this column, head first.
    glyphs: Vec<char>,
    /// Frames to wait before this column starts falling. Zero while it falls, which
    /// is what [`DUTY`] thins out.
    cooldown: u16,
}

pub struct Matrix {
    columns: Vec<Drop>,
    /// Glyphs this run is raining.
    pool: Vec<char>,
    /// Screen size the columns were built for; rebuilt when the terminal resizes.
    size: (u16, u16),
    rng: u64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            pool: Vec::new(),
            size: (0, 0),
            rng: 0,
        }
    }
}

impl Matrix {
    /// xorshift64*, so the animation needs no dependency and no allocation per
    /// frame. Nothing here is cryptography; it only has to look unpatterned.
    fn next_u64(&mut self) -> u64 {
        // Seeded lazily: a zero state would stay zero forever.
        if self.rng == 0 {
            self.rng = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545F4914F6CDD1D)
                | 1;
        }
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            return 0;
        }
        (self.next_u64() % limit as u64) as usize
    }

    fn glyph(&mut self) -> char {
        if self.pool.is_empty() {
            return FALLBACK_GLYPHS[self.below(FALLBACK_GLYPHS.len())];
        }
        let idx = self.below(self.pool.len());
        self.pool[idx]
    }


    fn rebuild(&mut self, width: u16, height: u16) {
        self.size = (width, height);
        self.columns = Vec::with_capacity(width as usize);
        for _ in 0..width {
            let drop = self.spawn(height, true);
            self.columns.push(drop);
        }
    }

    /// A column starting above the top of the screen, so it falls in rather than
    /// appearing all at once.
    ///
    /// `scattered` spreads the first generation over the height of the screen;
    /// respawns always come from above.
    fn spawn(&mut self, height: u16, scattered: bool) -> Drop {
        let len = 6 + self.below(height.max(8) as usize / 2) as u16;
        let head = if scattered {
            self.below(height as usize) as f32
        } else {
            // Just above the top, by up to its own length. This used to start as far
            // as a whole screen above, which desynchronised the columns - the job the
            // cooldown does now - while making the time a column spends alive but
            // invisible longer than the idle accounting below assumed. The result was
            // a screen a good deal busier than `DUTY` asked for.
            -(self.below(len.max(1) as usize) as f32)
        };
        let speed = (0.25 + (self.below(100) as f32) / 100.0 * 0.75) * SPEED_SCALE;
        let glyphs = (0..len).map(|_| self.glyph()).collect();

        // Idle time before falling, sized against how long this drop is on screen: a
        // column that spends `1 - DUTY` of its cycle empty makes the rain that much
        // sparser without any column going permanently dark.
        //
        // Measured from `head`, not from row zero, so the frames a drop spends
        // falling in from above the top are part of its life rather than free.
        let travel = height as f32 + len as f32 - head;
        let fall_frames = (travel / speed.max(0.01)) as usize;
        let idle_frames = (fall_frames as f32 * (1.0 - DUTY) / DUTY * 2.0) as usize;
        let cooldown = if scattered {
            // The first generation is already spread down the screen; thin it by
            // simply leaving some columns out to begin with.
            if self.below(100) < ((1.0 - DUTY) * 100.0) as usize {
                self.below(idle_frames.max(1)) as u16
            } else {
                0
            }
        } else {
            self.below(idle_frames.max(1)) as u16
        };

        Drop {
            head,
            speed,
            len,
            glyphs,
            cooldown,
        }
    }

    fn step(&mut self, width: u16, height: u16) {
        if self.size != (width, height) || self.columns.is_empty() {
            self.rebuild(width, height);
            return;
        }

        for idx in 0..self.columns.len() {
            if self.columns[idx].cooldown > 0 {
                self.columns[idx].cooldown -= 1;
                continue;
            }

            let (head, len, speed) = {
                let column = &self.columns[idx];
                (column.head, column.len, column.speed)
            };

            // Off the bottom, trail and all: start again from above.
            if head - len as f32 > height as f32 {
                self.columns[idx] = self.spawn(height, false);
                continue;
            }

            self.columns[idx].head = head + speed;

            // The trail flickers near the head.
            let depth = CHURN_DEPTH.min(len as usize);
            if depth > 0 {
                let at = self.below(depth);
                let glyph = self.glyph();
                self.columns[idx].glyphs[at] = glyph;
            }
        }
    }
}

/// Green at `fade` of the way from the tail level to the head level.
///
/// Never darker than [`TAIL_LEVEL`]: on a true-black backdrop a glyph that fades to
/// zero is a glyph that is not there.
fn green(fade: f32) -> Color {
    let fade = fade.clamp(0.0, 1.0);
    let level = (TAIL_LEVEL + (HEAD_LEVEL - TAIL_LEVEL) * fade) as u8;
    // A little blue keeps the brightest part from looking like pure primary green,
    // which reads as a terminal default rather than as light.
    Color::Rgb(0, level, (level / 5).min(60))
}

pub fn draw(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let (width, height) = (area.width, area.height);
    app.matrix.step(width, height);

    // True black, not the theme background: the joke is the film's screen, and a
    // light theme would leave green on white.
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new("").style(Style::new().bg(BACKDROP)), area);

    // Built per row, since that is how a Paragraph is drawn: for each cell, work
    // out whether some column's trail covers it.
    let mut rows: Vec<Line> = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut spans: Vec<Span> = Vec::with_capacity(width as usize);
        for x in 0..width {
            let column = &app.matrix.columns[x as usize];
            let head = column.head;
            let distance = head - y as f32;

            // Waiting to start, or not in this column's trail.
            if column.cooldown > 0 || distance < 0.0 || distance >= column.len as f32 {
                spans.push(Span::raw(" "));
                continue;
            }

            let depth = distance as usize;
            let glyph = column.glyphs[depth.min(column.glyphs.len() - 1)];
            let style = if depth == 0 {
                // The leading glyph is almost white, which is what gives the rain
                // its sense of falling rather than just fading.
                Style::new()
                    .fg(Color::Rgb(210, 255, 210))
                    .bg(BACKDROP)
                    .add_modifier(Modifier::BOLD)
            } else {
                let fade = 1.0 - (depth as f32 / column.len as f32);
                let style = Style::new().fg(green(fade * fade)).bg(BACKDROP);
                if depth < 3 {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }
            };
            spans.push(Span::styled(glyph.to_string(), style));
        }
        rows.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(rows), area);

    // One line of instruction, bottom right, dim enough not to spoil the picture.
    let hint = " any key to wake up ";
    let hint_width = hint.chars().count() as u16;
    if width > hint_width && height > 1 {
        let hint_area = Rect::new(width - hint_width, height - 1, hint_width, 1);
        frame.render_widget(
            Paragraph::new(hint).style(Style::new().fg(Color::Rgb(0, 90, 20)).bg(BACKDROP)),
            hint_area,
        );
    }
}

/// The glyph pool for `source`.
///
/// The file's bytes are sampled with a stride rather than read from the front: the
/// first kilobyte of an executable is a DOS stub and a run of zeros, which would
/// rain nothing but the fallback.
///
/// A free function rather than a method, because it reads the file through `app`
/// while `app.matrix` is being written to.
fn pool_from(app: &App, source: GlyphSource) -> Vec<char> {
    const WANTED: usize = 4096;

    match source {
        GlyphSource::Kana => katakana_glyphs(),
        GlyphSource::Hex => FALLBACK_GLYPHS.to_vec(),
        GlyphSource::File => {
            let buffer = app.file_info.get_buffer_ref();
            if buffer.is_empty() {
                return Vec::new();
            }
            let stride = (buffer.len() / WANTED).max(1);
            buffer
                .iter()
                .step_by(stride)
                .filter(|b| b.is_ascii_graphic())
                .map(|b| *b as char)
                .take(WANTED)
                .collect()
        }
    }
}

/// Opens the rain.
pub fn open(app: &mut App, source: GlyphSource) {
    let pool = pool_from(app, source);
    app.matrix = Matrix {
        pool,
        ..Default::default()
    };
    app.state = UIState::Matrix;
    app.dialog_renderer = None;
}

pub fn events(app: &mut App, _key: KeyEvent) -> Result<bool> {
    // Any key at all. Waiting for a specific one would be a puzzle, and this is a
    // joke, not a puzzle.
    app.state = UIState::Normal;
    app.matrix = Matrix::default();
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(bytes: &[u8]) -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dezes_matrix");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("m_{}_{}.bin", std::process::id(), id));
        std::fs::write(&path, bytes).unwrap();
        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, true).unwrap();
        app
    }

    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(app, f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    /// The rain is made of the file's own bytes.
    #[test]
    fn the_glyphs_come_from_the_file() {
        let mut app = app_with(b"ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ");
        super::open(&mut app, GlyphSource::File);

        assert!(!app.matrix.pool.is_empty(), "no glyphs were taken from the file");
        assert!(
            app.matrix.pool.iter().all(|c| *c == 'Z'),
            "the pool is not the file's bytes: {:?}",
            app.matrix.pool
        );

        let screen = render(&mut app, 40, 12).join("");
        assert!(screen.contains('Z'), "none of the file made it on screen");
    }

    /// A file with nothing printable in it still rains something.
    #[test]
    fn an_unprintable_file_falls_back_to_hex_digits() {
        let mut app = app_with(&[0u8; 512]);
        super::open(&mut app, GlyphSource::File);
        assert!(app.matrix.pool.is_empty());

        let screen = render(&mut app, 40, 12).join("");
        assert!(
            screen.chars().any(|c| FALLBACK_GLYPHS.contains(&c)),
            "an empty pool drew an empty screen"
        );
    }

    /// It has to move, and it has to keep moving without running off the bottom.
    #[test]
    fn the_rain_falls_and_never_runs_out() {
        let mut app = app_with(b"dezes matrix rain 0123456789");
        super::open(&mut app, GlyphSource::File);

        let first = render(&mut app, 60, 20);
        let mut changed = false;
        for _ in 0..40 {
            let next = render(&mut app, 60, 20);
            if next != first {
                changed = true;
            }
            assert!(
                next.iter().any(|row| row.trim() != ""),
                "the screen went empty part way through"
            );
        }
        assert!(changed, "the rain never moved");
    }

    /// `:matrix` is what opens it, and it is not in any help text.
    #[test]
    fn the_command_opens_it_and_stays_undocumented() {
        let mut app = app_with(b"neo trinity morpheus 0123456789");
        crate::commands::parse_command(&mut app, "matrix");
        assert!(app.state == UIState::Matrix, "':matrix' did not open the rain");

        for text in [
            crate::hex::help::HELP_EN,
            crate::hex::help::HELP_KO,
            crate::hex::help::HELP_ZH,
        ] {
            assert!(
                !text.to_lowercase().contains("matrix"),
                "the easter egg is documented in the help"
            );
        }
        assert!(
            !crate::global::settings::OPTION_NAMES.contains(&"matrix"),
            "the easter egg is in the option list"
        );
    }

    /// A quick look at what it actually draws, so a change that empties the screen
    /// or fills it solid is visible in the test output rather than only in person.
    #[test]
    fn it_looks_like_rain() {
        let mut app = app_with(b"0123456789ABCDEF dezes matrix");
        super::open(&mut app, GlyphSource::File);

        // Let it fall in for a while, then take a frame.
        for _ in 0..25 {
            let _ = render(&mut app, 64, 16);
        }
        let frame = render(&mut app, 64, 16);
        let filled: usize = frame
            .iter()
            .map(|row| row.chars().filter(|c| *c != ' ').count())
            .sum();
        let total = 64 * 16;

        println!("---- matrix frame ----");
        for row in &frame {
            println!("{}", row);
        }
        println!("---- {} of {} cells filled ----", filled, total);

        // Rain, not a blank screen and not a wall of text.
        assert!(filled > total / 20, "only {} cells have glyphs", filled);
        assert!(filled < total * 4 / 5, "{} cells filled - that is a wall", filled);
    }

    /// The tuning holds in the steady state.
    ///
    /// Measured after letting it run, not on a fresh screen: the first generation is
    /// scattered down the screen and is denser than what it settles at, which is
    /// exactly the trap that made an earlier eyeball check read the density as
    /// having gone *up* when the columns were thinned.
    #[test]
    fn the_rain_is_tuned_as_intended() {
        let mut app = app_with(b"0123456789ABCDEF dezes matrix rain");
        super::open(&mut app, GlyphSource::File);

        const W: u16 = 64;
        const H: u16 = 20;

        // Let it reach a steady state: the first generation is scattered down the
        // screen and is not what it settles at.
        for _ in 0..400 {
            let _ = render(&mut app, W, H);
        }

        let mut active_sum = 0usize;
        let mut fill_sum = 0usize;
        const SAMPLES: usize = 60;
        for _ in 0..SAMPLES {
            let frame = render(&mut app, W, H);
            // Columns with a glyph actually on screen, which is what "busy" means -
            // a column can be past its cooldown and still be falling in from above.
            active_sum += (0..W as usize)
                .filter(|x| frame.iter().any(|row| row.chars().nth(*x) != Some(' ')))
                .count();
            fill_sum += frame
                .iter()
                .map(|row| row.chars().filter(|c| *c != ' ').count())
                .sum::<usize>();
        }

        let active = active_sum as f32 / SAMPLES as f32 / W as f32;
        let fill = fill_sum as f32 / SAMPLES as f32 / (W as usize * H as usize) as f32;
        let speeds: Vec<f32> = app.matrix.columns.iter().map(|c| c.speed).collect();
        let min = speeds.iter().cloned().fold(f32::MAX, f32::min);
        let max = speeds.iter().cloned().fold(0.0, f32::max);
        println!(
            "steady state: {:.0}% of columns falling, {:.0}% of cells lit, speed {:.2}..{:.2} rows/frame",
            active * 100.0,
            fill * 100.0,
            min,
            max
        );

        // Tolerance sized from the measurement, not from taste: a column stays
        // active for many frames in a row, so successive samples are correlated and
        // 60 of them still land anywhere in 54..66%. A tighter bound here failed
        // roughly one run in six on a seed that came from the clock.
        assert!(
            (active - DUTY).abs() < 0.15,
            "{:.0}% of columns are falling, not the {:.0}% DUTY asks for",
            active * 100.0,
            DUTY * 100.0
        );
        // The speed range is 0.25..1.0 before scaling.
        assert!(
            min >= 0.25 * SPEED_SCALE - 0.01 && max <= 1.0 * SPEED_SCALE + 0.01,
            "speeds run {:.2}..{:.2}, outside the scaled range",
            min,
            max
        );
        // Rain, not a drizzle and not a wall.
        assert!(
            (0.06..0.32).contains(&fill),
            "{:.0}% of cells are lit",
            fill * 100.0
        );
    }

    /// Katakana is opt-in, and every glyph in it must be one cell wide.
    ///
    /// A full-width glyph would take two cells, so a column of them would be twice
    /// as wide as the grid it falls through and the whole picture would shear.
    #[test]
    fn katakana_is_half_width() {
        use unicode_width::UnicodeWidthChar;

        let mut app = app_with(b"anything");
        super::open(&mut app, GlyphSource::Kana);

        assert!(!app.matrix.pool.is_empty());
        for glyph in &app.matrix.pool {
            assert_eq!(
                glyph.width(),
                Some(1),
                "{:?} (U+{:04X}) is {} cells wide",
                glyph,
                *glyph as u32,
                glyph.width().unwrap_or(0)
            );
        }
        // It really is katakana, not the file.
        assert!(
            app.matrix.pool.iter().any(|c| ('\u{FF66}'..='\u{FF9D}').contains(c)),
            "no katakana in the pool"
        );
    }

    /// The glyph source is chosen on the command line, and a typo is reported.
    #[test]
    fn the_glyph_source_is_parsed() {
        assert_eq!(GlyphSource::parse(None), Some(GlyphSource::File));
        assert_eq!(GlyphSource::parse(Some("")), Some(GlyphSource::File));
        assert_eq!(GlyphSource::parse(Some("file")), Some(GlyphSource::File));
        assert_eq!(GlyphSource::parse(Some("kana")), Some(GlyphSource::Kana));
        assert_eq!(GlyphSource::parse(Some("katakana")), Some(GlyphSource::Kana));
        assert_eq!(GlyphSource::parse(Some("hex")), Some(GlyphSource::Hex));
        assert_eq!(GlyphSource::parse(Some("kanaa")), None);

        let mut app = app_with(b"hello");
        crate::commands::parse_command(&mut app, "matrix kanaa");
        assert!(
            app.last_error.message.contains("kana"),
            "a typo has to say what the choices are, got: {}",
            app.last_error.message
        );
        assert!(app.state != UIState::Matrix);

        crate::commands::parse_command(&mut app, "matrix kana");
        assert!(app.state == UIState::Matrix);
    }

    /// No glyph is drawn dark enough to vanish into the backdrop.
    ///
    /// The backdrop is true black now, so a trail that faded to zero simply ended
    /// in nothing. Every glyph keeps at least `TAIL_LEVEL` of green.
    #[test]
    fn the_tail_stays_visible_against_black() {
        assert_eq!(BACKDROP, Color::Rgb(0, 0, 0), "the backdrop is not true black");

        for step in 0..=20 {
            let fade = step as f32 / 20.0;
            let Color::Rgb(r, g, b) = green(fade) else {
                panic!("the rain has to use rgb, or the theme decides its colours");
            };
            assert_eq!(r, 0);
            assert!(
                g as f32 >= TAIL_LEVEL - 1.0,
                "fade {} gives green {}, which is invisible on black",
                fade,
                g
            );
            assert!(g > b, "the rain went blue");
        }
    }

    /// Resizing rebuilds the columns instead of indexing past the old width.
    #[test]
    fn a_resize_is_survivable() {
        let mut app = app_with(b"abcdefghijklmnopqrstuvwxyz");
        super::open(&mut app, GlyphSource::File);

        let _ = render(&mut app, 80, 24);
        let _ = render(&mut app, 40, 12);
        let _ = render(&mut app, 120, 40);
        assert_eq!(app.matrix.columns.len(), 120);
    }

    /// Any key wakes up, and nothing about the file changed.
    #[test]
    fn any_key_returns_to_normal() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let mut app = app_with(b"hello");
        super::open(&mut app, GlyphSource::File);
        assert!(app.state == UIState::Matrix);

        let _ = super::events(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        assert!(app.state == UIState::Normal);
        assert!(app.hex_view.changed_bytes.is_empty(), "the joke edited the file");
        assert_eq!(app.hex_view.offset, 0);
    }
}