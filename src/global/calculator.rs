use crate::{app::App, util::center_widget};

use ratatui::{
    Frame,
    layout::Rect,
    symbols,
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use crate::editor::UIState;

use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers};
use std::{collections::HashSet, io::Result};
use tui_input::Input;

use evalexpr::*;

/// Rewrites bare number literals in `expr` from hexadecimal to decimal, so
/// `evalexpr` - which only understands decimal - evaluates what the user meant.
///
/// dz6 is a hex editor: every address and byte value on screen is hexadecimal, and
/// the command line, `:goto` and the assembler all already read `1d1` as 0x1D1 with
/// `t` for decimal. The calculator was the one place that read it as decimal, so
/// copying a value out of the hex view and adding to it gave a wrong answer with no
/// indication anything had been reinterpreted.
///
/// A token is a number when it is made only of hex digits, optionally `0x`-prefixed,
/// or ends in `t`/`T` for an explicit decimal. Anything else - `@B`, `i64::MAX`, a
/// comment name, a function - is left exactly as written. A token that *is* a
/// defined variable wins over the number reading, so naming a comment `ff` still
/// refers to that comment rather than to 255.
fn hex_literals_to_decimal(expr: &str, ctx: &HashMapContext) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut token = String::new();

    /// `@`, `_` and alphanumerics hold a token together; everything else - operators,
    /// brackets, `:`, `.`, whitespace - ends it.
    fn is_token_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '@'
    }

    let flush = |token: &mut String, out: &mut String| {
        if token.is_empty() {
            return;
        }
        match convert_literal(token, ctx) {
            Some(decimal) => out.push_str(&decimal),
            None => out.push_str(token),
        }
        token.clear();
    };

    for c in expr.chars() {
        if is_token_char(c) {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);

    out
}

/// The decimal spelling of `token`, or `None` when it is not a number literal.
fn convert_literal(token: &str, ctx: &HashMapContext) -> Option<String> {
    // An explicit decimal, the same `t` suffix the rest of dz6 uses.
    if let Some(digits) = token.strip_suffix('t').or_else(|| token.strip_suffix('T')) {
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            // Already decimal; hand it over without the suffix, which evalexpr would
            // otherwise read as an identifier.
            return Some(digits.to_string());
        }
        return None;
    }

    let hex = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
        .unwrap_or(token);
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }

    // A variable of that name is what the user meant, not the number.
    if !token.starts_with("0x") && !token.starts_with("0X") && ctx.get_value(token).is_some() {
        return None;
    }

    u64::from_str_radix(hex, 16).ok().map(|v| v.to_string())
}

/// `50332113` -> `50,332,113`.
fn with_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Signed decimal with separators, keeping the sign outside the grouping.
fn signed_with_thousands(value: i64) -> String {
    if value < 0 {
        format!("-{}", with_thousands(value.unsigned_abs()))
    } else {
        with_thousands(value as u64)
    }
}

/// `0000 0000  0000 0000  0000 0000  0000 0000` for one 32-bit half.
///
/// Grouped in nibbles with a wider gap between bytes, which is what makes a bit
/// position countable by eye rather than by squinting at 64 identical characters.
fn binary_row(half: u32) -> String {
    let mut groups = Vec::with_capacity(4);
    for byte_index in (0..4).rev() {
        let byte = (half >> (byte_index * 8)) as u8;
        groups.push(format!("{:04b} {:04b}", byte >> 4, byte & 0x0F));
    }
    groups.join("  ")
}

#[derive(Default)]
pub struct Calculator {
    pub input: Input,
    /// Character a Shift-selection started from, or `None`.
    pub anchor: Option<usize>,
    pub context: HashMapContext,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    // history_set is a HashSet to avoid duplicates, although users
    // can bypass that with something like "1+1" != "1 + 1"
    pub history_set: HashSet<String>,
    pub result: i64,
}

impl Calculator {
    pub fn push_history(&mut self, entry: String) {
        if !entry.trim().is_empty() && self.history_set.insert(entry.clone()) {
            self.history.push(entry);
        }
        self.history_index = None;
    }
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let len = self.history.len();

        let new_index = match self.history_index {
            None => len - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };

        self.history_index = Some(new_index);
        self.input = Input::new(self.history[new_index].clone());
    }
    pub fn history_down(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let len = self.history.len();

        let new_index = match self.history_index {
            None => 0,
            Some(i) => (i + 1).min(len - 1),
        };

        self.history_index = Some(new_index);
        self.input = Input::new(self.history[new_index].clone());
    }
}

/// Width the layout is designed for.
///
/// The widest row is a 32-bit binary half at 42 characters; the rest is slack so a
/// long DEC row - a 64-bit value printed twice with separators runs to about 50
/// characters - is not clipped.
const CALC_WIDTH: u16 = 62;
/// Border, input, rule, three radix rows, rule, heading, two binary rows, border.
const CALC_HEIGHT: u16 = 11;

pub fn dialog_calculator_draw(app: &mut App, frame: &mut Frame) {
    // Lifted off dead centre, like the Goto, Comment and Image Base boxes: the row
    // being worked on is usually mid-screen, and the calculator is often opened to
    // work out an address from bytes that are visible behind it.
    let mut area = center_widget(CALC_WIDTH, CALC_HEIGHT, frame.area());
    area.y = area.y.saturating_sub(3);

    let result = app.calculator.result;
    let unsigned = result as u64;

    // No zero padding: the leading zeroes carry no information and make the one
    // number the user is most likely to copy out harder to read. Width is what the
    // BINARY rows below are for.
    let hex = format!("0x{:X}", unsigned);

    let dec = if result < 0 {
        // Signed and unsigned differ only for negatives, and then both readings are
        // worth having: the same bits are a small negative or a huge positive.
        format!(
            "{}  (Unsigned: {})",
            signed_with_thousands(result),
            with_thousands(unsigned)
        )
    } else {
        format!("{}  (Unsigned: {})", with_thousands(unsigned), with_thousands(unsigned))
    };

    let rows = [
        String::new(), // the expression, drawn as spans below
        String::new(), // rule, drawn over below
        format!("HEX      {}", hex),
        format!("DEC      {}", dec),
        format!("OCT      0o{:o}", unsigned),
        String::new(), // rule
        "BINARY (64-bit)".to_string(),
        binary_row((unsigned >> 32) as u32),
        binary_row(unsigned as u32),
    ];

    let block = Block::new()
        .title(crate::i18n::M::CalculatorTitle.tr(app.config.lang))
        .borders(Borders::ALL)
        .border_set(symbols::border::DOUBLE)
        .style(app.config.theme.dialog)
        .padding(Padding::horizontal(1));

    // The first row is built from spans so a Shift-selection can be highlighted;
    // the rest is plain text.
    let mut lines: Vec<ratatui::text::Line> = Vec::with_capacity(rows.len());
    let mut first = vec![ratatui::text::Span::styled("> ", app.config.theme.dialog)];
    first.extend(
        crate::text_field::render_line(
            &app.calculator.input,
            app.calculator.anchor,
            app.config.theme.dialog,
            app.config.theme.highlight,
        )
        .spans,
    );
    lines.push(ratatui::text::Line::from(first));
    for row in rows.iter().skip(1) {
        lines.push(ratatui::text::Line::raw(row.clone()));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);

    // The two horizontal rules are drawn over the border afterwards so their ends
    // join it. A Paragraph inside the block cannot reach the border columns.
    let rule: String = format!(
        "{}{}{}",
        symbols::line::DOUBLE_VERTICAL_RIGHT,
        symbols::line::HORIZONTAL.repeat(area.width.saturating_sub(2) as usize),
        symbols::line::DOUBLE_VERTICAL_LEFT
    );
    for row_offset in [2u16, 6] {
        if row_offset >= area.height.saturating_sub(1) {
            continue;
        }
        let rule_area = Rect {
            x: area.x,
            y: area.y + row_offset,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(rule.clone()).style(app.config.theme.dialog),
            rule_area,
        );
    }

    // Cursor sits after the "> " prompt.
    let cursor_x = area.x + 4 + app.calculator.input.visual_cursor() as u16;
    if cursor_x < area.x + area.width.saturating_sub(1) {
        frame.set_cursor_position((cursor_x, area.y + 1));
    }
}

/// The calculator's expression box and its selection anchor.
fn calculator_field(app: &mut App) -> (&mut Input, &mut Option<usize>) {
    (&mut app.calculator.input, &mut app.calculator.anchor)
}

/// Populates the calculator context with the built-in variables.
///
/// Every `set_value` used to be `.unwrap()`ed. `evalexpr` rejects anything that
/// is not a valid identifier, and the comment loop below feeds it *user-typed
/// comment text* - so a comment containing a space or an operator crashed the
/// whole editor the moment the calculator was opened. Failures are now skipped.
fn load_variables(app: &mut App) {
    /// Offsets are `usize`; saturate rather than wrapping into a negative i64.
    fn as_int(value: usize) -> Value {
        Value::from_int(value.min(i64::MAX as usize) as i64)
    }

    let ctx = &mut app.calculator.context;

    // constants
    let _ = ctx.set_value("i64::MAX".to_string(), Value::from_int(i64::MAX));
    let _ = ctx.set_value("i64::MIN".to_string(), Value::from_int(i64::MIN));

    // comments (identifiers here come from user input and are often invalid)
    for cmt in &app.hex_view.comment_name_list {
        let _ = app
            .calculator
            .context
            .set_value(cmt.comment.clone(), as_int(cmt.offset));
    }

    let offset = app.hex_view.offset;
    let last_visited = app.hex_view.last_visited_offset;

    // @B/@b -> unsigned/signed byte, @W/@w word, @D/@d dword, @Q/@q qword
    let byte_vars: [(&str, Option<i64>); 8] = [
        ("@B", app.read_u8(offset).map(i64::from)),
        ("@b", app.read_i8(offset).map(i64::from)),
        ("@W", app.read_u16(offset).map(i64::from)),
        ("@w", app.read_i16(offset).map(i64::from)),
        ("@D", app.read_u32(offset).map(i64::from)),
        ("@d", app.read_i32(offset).map(i64::from)),
        ("@Q", app.read_u64(offset).map(|v| v as i64)),
        ("@q", app.read_i64(offset)),
    ];

    for (name, value) in byte_vars {
        if let Some(v) = value {
            let _ = app
                .calculator
                .context
                .set_value(name.to_string(), Value::from_int(v));
        }
    }

    let ctx = &mut app.calculator.context;

    // @o -> current offset, @O -> previous offset
    let _ = ctx.set_value("@o".to_string(), as_int(offset));
    let _ = ctx.set_value("@O".to_string(), as_int(last_visited));

    let _ = ctx.set_function(
        String::from("fu8"),
        Function::new(|arg| {
            if let Ok(ofs) = arg.as_int() {
                Ok(Value::Int(ofs + 10))
            } else if let Ok(float) = arg.as_number() {
                Ok(Value::Float(float / 2.0))
            } else {
                Err(EvalexprError::expected_number(arg.clone()))
            }
        }),
    );
}

pub fn dialog_calculator_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Esc => {
                app.dialog_renderer = None;
                app.state = UIState::Normal;
            }
            // Ctrl+L clears the expression and the result. Not a bare 'c': every
            // letter a-f is a hex digit now, so the obvious "clear" key would make
            // values like `c000` untypeable.
            KeyCode::Char('l') | KeyCode::Char('L')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.calculator.input = Input::default();
                app.calculator.result = 0;
                app.calculator.history_index = None;
            }
            KeyCode::Enter => {
                let input_expr = app.calculator.input.value().to_string();

                load_variables(app);

                // Numbers are hexadecimal here, as everywhere else in dz6; evalexpr
                // only reads decimal, so the literals are rewritten first.
                let evaluated =
                    hex_literals_to_decimal(&input_expr, &app.calculator.context);
                let result = eval_with_context_mut(&evaluated, &mut app.calculator.context);

                app.calculator.push_history(input_expr);

                match result {
                    Ok(v) => {
                        if let Ok(a) = v.as_int() {
                            app.calculator.result = a;
                        }
                    }
                    Err(_e) => {
                        // app.calculator.history.push(format!("Error: {}", e));
                    }
                }
            }
            KeyCode::Up => {
                app.calculator.history_up();
            }
            KeyCode::Down => {
                app.calculator.history_down();
            }
            // Shift+arrows, Shift+Home/End and Ctrl+C/X/V over the block.
            _ => {
                crate::text_field::handle_key(app, calculator_field, event);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HashMapContext {
        HashMapContext::new()
    }

    /// Numbers are hexadecimal, matching the rest of dz6.
    ///
    /// The calculator used to read them as decimal, so a value copied out of the hex
    /// view was silently reinterpreted: `1d1+3000000` answered as if both operands
    /// were decimal.
    #[test]
    fn bare_numbers_are_hexadecimal() {
        let c = ctx();
        assert_eq!(hex_literals_to_decimal("1d1", &c), "465");
        assert_eq!(hex_literals_to_decimal("ff", &c), "255");
        assert_eq!(hex_literals_to_decimal("c000", &c), "49152");
        // The reported expression: 0x1D1 + 0x3000000 = 0x30001D1 = 50,332,113.
        assert_eq!(
            hex_literals_to_decimal("1d1+3000000", &c),
            format!("{}+{}", 0x1d1, 0x3000000)
        );
    }

    /// `t` is the explicit decimal marker, as in `:goto` and the assembler.
    #[test]
    fn the_t_suffix_means_decimal() {
        let c = ctx();
        assert_eq!(hex_literals_to_decimal("300t", &c), "300");
        assert_eq!(hex_literals_to_decimal("300T", &c), "300");
        assert_ne!(
            hex_literals_to_decimal("300t", &c),
            hex_literals_to_decimal("300", &c),
            "'300' is hex 0x300, '300t' is three hundred"
        );
        // The suffix must not survive into the expression, or evalexpr reads it as an
        // identifier and the whole thing fails.
        assert!(!hex_literals_to_decimal("300t+1", &c).contains('t'));
    }

    /// An explicit `0x` prefix is accepted and stripped.
    #[test]
    fn the_0x_prefix_is_accepted() {
        let c = ctx();
        assert_eq!(hex_literals_to_decimal("0x1f", &c), "31");
        assert_eq!(hex_literals_to_decimal("0X1F", &c), "31");
    }

    /// Operators, brackets and spacing survive untouched.
    #[test]
    fn structure_is_preserved() {
        let c = ctx();
        assert_eq!(hex_literals_to_decimal("(10 + 20) * 2", &c), "(16 + 32) * 2");
        assert_eq!(hex_literals_to_decimal("10 << 2", &c), "16 << 2");
    }

    /// Identifiers must not be mistaken for numbers.
    ///
    /// `@B`, `i64::MAX` and the function names are not hex-only, so the token rule
    /// leaves them alone.
    #[test]
    fn identifiers_are_left_alone() {
        let c = ctx();
        for text in ["@B", "@o", "@Q", "i64::MAX", "i64::MIN", "fu8(2)"] {
            let rewritten = hex_literals_to_decimal(text, &c);
            assert!(
                rewritten.starts_with(text.trim_end_matches("(2)")),
                "'{text}' was rewritten to '{rewritten}'"
            );
        }
    }

    /// A variable whose name happens to be hex digits still refers to the variable.
    ///
    /// Comment names come from user text, so `ff` is a plausible one; resolving it to
    /// 255 instead would silently break a lookup the user set up deliberately.
    #[test]
    fn a_defined_variable_wins_over_the_number_reading() {
        let mut c = ctx();
        c.set_value("ff".to_string(), Value::from_int(0x1234)).expect("set");

        assert_eq!(
            hex_literals_to_decimal("ff", &c),
            "ff",
            "a defined name must stay a name"
        );
        // An explicit 0x is unambiguous and still means the number.
        assert_eq!(hex_literals_to_decimal("0xff", &c), "255");
        // An undefined one is a number.
        assert_eq!(hex_literals_to_decimal("ee", &c), "238");
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(with_thousands(0), "0");
        assert_eq!(with_thousands(999), "999");
        assert_eq!(with_thousands(1000), "1,000");
        assert_eq!(with_thousands(50_332_113), "50,332,113");
        assert_eq!(signed_with_thousands(-1_234_567), "-1,234,567");
        // `i64::MIN` has no positive counterpart; `unsigned_abs` is why this does not
        // overflow.
        assert_eq!(signed_with_thousands(i64::MIN), "-9,223,372,036,854,775,808");
    }

    /// Nibble groups, wider gap between bytes, most significant bit first.
    #[test]
    fn binary_rows_are_grouped_for_counting() {
        assert_eq!(binary_row(0), "0000 0000  0000 0000  0000 0000  0000 0000");
        assert_eq!(
            binary_row(0x002D_C6CB),
            "0000 0000  0010 1101  1100 0110  1100 1011"
        );
        assert_eq!(binary_row(u32::MAX).chars().count(), 42);
        assert!(
            (binary_row(u32::MAX).chars().count() as u16) <= CALC_WIDTH - 4,
            "the widest row must fit inside the border and padding"
        );
    }

    /// The dialog renders and shows every radix row.
    #[test]
    fn the_dialog_renders_every_row() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = crate::app::App::new();
        app.config.database = false;
        app.calculator.result = 0x002D_C6CB;
        app.calculator.input = Input::new("1d1+3000000".to_string());

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                dialog_calculator_draw(&mut app, f);
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let text: String = (0..24u16)
            .map(|y| {
                let row: String = (0..80u16).map(|x| buf[(x, y)].symbol().to_string()).collect();
                format!("{}\n", row.trim_end())
            })
            .collect();

        for expected in [
            "Calculator",
            "1d1+3000000",
            "HEX      0x2DC6CB",
            "DEC      3,000,011",
            "OCT      0o13343313",
            "BINARY (64-bit)",
            "0000 0000  0010 1101  1100 0110  1100 1011",
        ] {
            assert!(
                text.contains(expected),
                "expected '{expected}' in the dialog, got:\n{text}"
            );
        }
    }

    /// The box sits above centre, so the bytes it was opened to work on stay visible.
    #[test]
    fn the_dialog_sits_above_centre() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = crate::app::App::new();
        app.config.database = false;

        let height = 24u16;
        let mut terminal = Terminal::new(TestBackend::new(80, height)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                dialog_calculator_draw(&mut app, f);
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let title_row = (0..height)
            .find(|&y| {
                (0..80u16)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .contains("Calculator")
            })
            .expect("the titled border must be drawn");

        let centred = (height - CALC_HEIGHT) / 2;
        assert!(
            title_row < centred,
            "expected the box above row {centred}, found its title on row {title_row}"
        );
    }

    /// Ctrl+L empties the expression and the result.
    #[test]
    fn ctrl_l_clears_everything() {
        use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

        let mut app = crate::app::App::new();
        app.config.database = false;
        app.state = crate::editor::UIState::DialogCalculator;
        app.calculator.input = Input::new("dead+beef".to_string());
        app.calculator.result = 0x1234;

        let event = Event::Key(KeyEvent {
            code: KeyCode::Char('l'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        dialog_calculator_events(&mut app, &event).expect("handled");

        assert_eq!(app.calculator.input.value(), "");
        assert_eq!(app.calculator.result, 0);
        assert!(
            app.state == crate::editor::UIState::DialogCalculator,
            "clearing must not close the dialog"
        );
    }

    /// A bare 'l' is still typed, since it is only a letter - and every letter a-f
    /// must remain typeable because they are hex digits.
    #[test]
    fn a_bare_letter_is_still_typed() {
        use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

        let mut app = crate::app::App::new();
        app.config.database = false;

        for c in ['c', 'l', 'd', 'e', 'a', 'f', 'b'] {
            let event = Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            });
            dialog_calculator_events(&mut app, &event).expect("handled");
        }

        assert_eq!(
            app.calculator.input.value(),
            "cldeafb",
            "hex digits must be typeable; clearing is Ctrl+L, not a bare letter"
        );
    }
}
