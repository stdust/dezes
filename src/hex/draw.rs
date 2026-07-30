use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Paragraph, Row, Table, TableState},
};

use crate::{app::App, editor::UIState};

// Left column with offsets
pub fn draw_hex_offsets(app: &mut App, frame: &mut Frame, area: Rect) {
    let show_va = app.hex_view.show_va;
    let is_64 = app.is_64();

    // Helper formatting function according to user specific digits spec
    let format_addr = |app: &App, ofs: usize| -> String {
        let addr = if show_va { app.get_va(ofs) } else { ofs as u64 };
        if is_64 {
            if show_va {
                let raw = format!("{:X}", addr);
                if raw.len() < 9 { format!("{:09X}", addr) } else { raw }
            } else {
                format!("{:09X}", addr) // 64bit Offset: 000000000 (9 digits padded)
            }
        } else {
            format!("{:08X}", addr) // 32bit VA & Offset: 00000000 (8 digits padded)
        }
    };

    // `max(1)` guards the division: bytes-per-line reaching 0 (`:set byteline 0`
    // or a .dz6init line) used to panic here on the very next frame.
    let bytes_per_line = app.config.hex_mode_bytes_per_line.max(1);

    // Offset lines
    let mut rows: Vec<Row> = Vec::with_capacity(app.reader.page_current_size / bytes_per_line + 1);
    let mut ofs = app.reader.page_start;
    let height = frame.area().height as usize;

    let col_width = app.get_addr_col_width();
    for _ in 0..height {
        let addr_text = format!("{:^width$}", format_addr(app, ofs), width = col_width);
        rows.push(Row::new([addr_text]));
        ofs += bytes_per_line;

        // Prevent further offsets to appear
        if ofs >= app.file_info.size {
            break;
        }
    }

    // Show filesize as last offset
    if app.file_info.size > 0 {
        let addr_text = format!("{:^width$}", format_addr(app, app.file_info.size), width = col_width);
        rows.push(Row::new([addr_text]));
    }

    app.hex_view
        .offset_state
        .select(Some(app.hex_view.cursor.y));

    let col_width = app.get_addr_col_width() as u16;
    let table = Table::new(rows, [Constraint::Length(col_width); 1]).style(app.config.theme.offsets);

    frame.render_stateful_widget(table, area, &mut app.hex_view.offset_state);
}

/// Flat `"00 01 02 ... FF "` table: three bytes per entry, the third being the
/// column separator.
///
/// Rendering a hex cell used to do `HEX_LOOKUP[b].to_string()`, i.e. one heap
/// allocation per byte per frame (~800 for a 16x50 page). Slicing this table
/// yields a `&'static str` instead, so the common case allocates nothing.
const fn build_hex3_table() -> [u8; 768] {
    let digits = *b"0123456789ABCDEF";
    let mut table = [b' '; 768];
    let mut i = 0;
    while i < 256 {
        table[i * 3] = digits[i >> 4];
        table[i * 3 + 1] = digits[i & 0x0F];
        i += 1;
    }
    table
}

static HEX3_TABLE: [u8; 768] = build_hex3_table();

/// Two-character hex representation of `b`, e.g. `"4D"`.
#[inline]
fn hex2(b: u8) -> &'static str {
    let i = b as usize * 3;
    // The table is ASCII by construction, so this never actually fails.
    std::str::from_utf8(&HEX3_TABLE[i..i + 2]).unwrap_or("??")
}

/// Hex representation of `b` followed by the column separator, e.g. `"4D "`.
#[inline]
fn hex3(b: u8) -> &'static str {
    let i = b as usize * 3;
    std::str::from_utf8(&HEX3_TABLE[i..i + 3]).unwrap_or("?? ")
}

/// Left-pads a single-digit stored edit ("A" -> "0A") and optionally appends the
/// column separator.
fn changed_byte_text(stored: &str, with_separator: bool) -> String {
    let mut out = String::with_capacity(3);
    if stored.len() == 1 {
        out.push('0');
    }
    out.push_str(stored);
    if with_separator {
        out.push(' ');
    }
    out
}

/// Background override for `offset`, or `None` when it is not inside a block.
///
/// `blocks` is kept sorted by `start` (see `hex/selection.rs`), so the scan can
/// stop as soon as a block starts past the offset instead of walking the whole
/// list for every byte of every frame.
#[inline]
fn block_bg(blocks: &[crate::hex::blocks::ColoredBlock], offset: usize) -> Option<u32> {
    let mut found = None;
    for b in blocks {
        if b.start > offset {
            break;
        }
        if offset <= b.end {
            found = Some(b.bg_color);
        }
    }
    found
}

/// Final style the hex cell at `offset` is drawn with.
///
/// Extracted so the same answer can be asked for the *next* byte: whether the
/// column separator after a cell is painted depends on whether its neighbour
/// ends up the same colour. Computing that twice inline would let the two copies
/// drift.
fn cell_style(app: &App, offset: usize, byte: u8, selection_active: bool, main_style: Style) -> Style {
    let in_selection = app.hex_view.selection.contains(offset);

    let mut style = if selection_active && in_selection {
        app.config.theme.highlight
    } else if crate::hex::search::in_match(app, offset) {
        // Every hit of the current search pattern, not just the one the cursor is
        // on. With 12,000 matches in a file, "3 of 12578" in the status line said
        // nothing about which bytes a Replace All would touch; this shows them.
        app.config.theme.byte_highlight
    } else if byte == b'\0' && app.config.dim_zeroes {
        app.config.theme.dimmed
    } else if !byte.is_ascii_graphic() && app.config.dim_control_chars {
        app.config.theme.dimmed
    } else {
        main_style
    };

    if let Some(bg) = block_bg(&app.hex_view.blocks, offset) {
        style = style.bg(Color::from_u32(bg));
    }

    if app.hex_view.changed_bytes.contains_key(&offset) && !in_selection {
        style = app.config.theme.changed_bytes;
    }

    if app.hex_view.highlights.contains(&byte) {
        style = app.config.theme.byte_highlight;
    }

    let is_cursor = offset == app.hex_view.offset;
    if is_cursor {
        style = if app.state == UIState::HexEditing && app.hex_view.editing_hex {
            app.config.theme.editing
        } else {
            app.config.theme.highlight
        };
    }

    style
}

// Middle area with the actual hex dump
pub fn draw_hex_contents(app: &mut App, frame: &mut Frame, area: Rect) {
    let page_start = app.reader.page_start;
    let bpl = app.config.hex_mode_bytes_per_line.max(1);

    // Borrowed page slice: the old code copied every visible byte into a fresh
    // Vec on each frame.
    let buffer = app.file_info.get_buffer_ref();
    let page_end = page_start
        .saturating_add(app.reader.page_current_size)
        .min(buffer.len());
    let bytes: &[u8] = if page_start < buffer.len() {
        &buffer[page_start..page_end]
    } else {
        &[]
    };

    let rows_capacity = bytes.len() / bpl + 1;
    let mut rows1: Vec<Row> = Vec::with_capacity(rows_capacity);
    let mut rows2: Vec<Row> = Vec::with_capacity(rows_capacity);
    let mut byte_row1: Vec<Cell> = Vec::with_capacity(8);
    let mut byte_row2: Vec<Cell> = Vec::with_capacity(bpl.saturating_sub(8).max(1));

    let main_style = app.config.theme.main;
    let selection_active =
        app.state == UIState::HexSelection || app.hex_view.selection.start != app.hex_view.selection.end;

    for (i, byte) in bytes.iter().enumerate() {
        let offset = page_start + i;

        // Single map probe: this used to be `contains_key` followed by `[&offset]`.
        let changed = app.hex_view.changed_bytes.get(&offset);

        let idx_in_line = i % bpl;
        let is_last_in_group = idx_in_line == 7 || idx_in_line == bpl - 1;

        let hl_style = cell_style(app, offset, *byte, selection_active, main_style);

        // A style that paints its own background must not spill into the column
        // separator *unless* the next byte is painted the same colour.
        //
        // Both halves of that rule are load-bearing. Without the first, a single
        // highlighted byte (Alt+H) or the cursor covers three columns and reads as
        // a fat marker misaligned with the grid. Without the second, a selection
        // or a run of changed bytes is chopped into one stripe per byte with gaps
        // between them, instead of one continuous band.
        let paints_background = hl_style.bg.is_some() && hl_style.bg != main_style.bg;

        let separator_style = if is_last_in_group {
            main_style
        } else {
            match bytes.get(i + 1) {
                Some(next) => {
                    let next_style = cell_style(app, offset + 1, *next, selection_active, main_style);
                    if next_style.bg == hl_style.bg {
                        hl_style
                    } else {
                        main_style
                    }
                }
                None => main_style,
            }
        };

        let cell = if paints_background && !is_last_in_group {
            let content: std::borrow::Cow<'static, str> = match changed {
                Some(s) => std::borrow::Cow::Owned(changed_byte_text(s, false)),
                None => std::borrow::Cow::Borrowed(hex2(*byte)),
            };
            Cell::new(Line::from(vec![
                Span::styled(content, hl_style),
                Span::styled(" ", separator_style),
            ]))
        } else {
            let content: std::borrow::Cow<'static, str> = match changed {
                Some(s) => std::borrow::Cow::Owned(changed_byte_text(s, !is_last_in_group)),
                None if is_last_in_group => std::borrow::Cow::Borrowed(hex2(*byte)),
                None => std::borrow::Cow::Borrowed(hex3(*byte)),
            };
            // `hl_style`, not the pre-cursor style: on the last byte of a group
            // this is the only branch taken, so using the plain byte style left
            // the cursor invisible there.
            Cell::new(Span::raw(content)).style(hl_style)
        };

        if idx_in_line < 8 {
            byte_row1.push(cell);
        } else {
            byte_row2.push(cell);
        }

        if (i + 1) % bpl == 0 {
            // `take` reuses the buffers instead of cloning both Vec<Cell> per row.
            rows1.push(Row::new(std::mem::take(&mut byte_row1)));
            rows2.push(Row::new(std::mem::take(&mut byte_row2)));
        }
    }

    if !byte_row1.is_empty() || !byte_row2.is_empty() {
        rows1.push(Row::new(byte_row1));
        rows2.push(Row::new(byte_row2));
    }

    // Column widths derived from bytes-per-line instead of hardcoded to 23,
    // which only happened to be right for the default bpl of 16.
    // Saturating conversions: `bpl` comes from `:set byteline`, so a large value
    // would silently wrap a plain `as u16` into a tiny width and scramble the
    // layout instead of simply being clamped to the terminal.
    let group1_len = bpl.min(8);
    let group1_width = u16::try_from(group1_len * 3 - 1).unwrap_or(u16::MAX);
    let group2_width = if bpl > 8 {
        u16::try_from((bpl - 8) * 3 - 1).unwrap_or(u16::MAX)
    } else {
        0
    };

    let hex_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Length(group1_width),
            Constraint::Length(1),
            Constraint::Length(group2_width),
            Constraint::Min(0),
        ])
        .split(area);

    let mut constraints1 = Vec::with_capacity(group1_len);
    for idx in 0..group1_len {
        if idx == 7 || idx == bpl - 1 {
            constraints1.push(Constraint::Length(2));
        } else {
            constraints1.push(Constraint::Length(3));
        }
    }

    let mut constraints2 = Vec::with_capacity(bpl.saturating_sub(8));
    if bpl > 8 {
        for idx in 8..bpl {
            if idx == bpl - 1 {
                constraints2.push(Constraint::Length(2));
            } else {
                constraints2.push(Constraint::Length(3));
            }
        }
    }

    let table1 = Table::new(rows1, constraints1)
        .column_spacing(0)
        .style(app.config.theme.main);

    let table2 = Table::new(rows2, constraints2)
        .column_spacing(0)
        .style(app.config.theme.main);

    let main_bg = app.config.theme.main.bg.unwrap_or(Color::Reset);
    let mid_sep_style = Style::default()
        .fg(Color::Rgb(0x5A, 0x64, 0x73))
        .bg(main_bg);
    // `area.height - 1` underflows on a zero-height area; build the string by
    // repetition and trim the trailing newline instead.
    let mut v_sep_str = "│\n".repeat(area.height as usize);
    if v_sep_str.ends_with('\n') {
        v_sep_str.pop();
    }
    let mid_sep = Paragraph::new(v_sep_str).style(mid_sep_style);

    frame.render_widget(Clear, area);

    let cursor_x = app.hex_view.cursor.x;
    let cursor_y = app.hex_view.cursor.y;

    let mut state1 = TableState::default();
    let mut state2 = TableState::default();
    state1.select(Some(cursor_y));
    state2.select(Some(cursor_y));

    if cursor_x < 8 {
        state1.select_column(Some(cursor_x));
        state2.select_column(None);
    } else {
        state1.select_column(None);
        state2.select_column(Some(cursor_x - 8));
    }

    frame.render_stateful_widget(table1, hex_layout[0], &mut state1);
    if bpl >= 16 {
        frame.render_widget(mid_sep, hex_layout[1]);
        frame.render_stateful_widget(table2, hex_layout[2], &mut state2);
    }
}

/// Essa função desenha o ASCII dump em modo hexa. Ela tabmém permite a edição,
/// de modo que aceita texto normal do teclado. A função precisa:
///
/// 1. Criar uma Cell com cada char (porque precisa estilizá-la individualmente)
/// 2. Se estiver editando, estilizar o highlight (pode ser fora do loop)
/// 3. Se estiver editando E os bytes forem alterados, aplicar os estilos individualmente
/// 4. Se chegar em 16 bytes, pushar no vetor de Rows
///
/// OBS.: Table é criada a partir de Row, que são conjuntos de Cell
pub fn draw_hex_ascii(app: &mut App, frame: &mut Frame, area: Rect, is_enc2: bool) {
    use crate::editor::EditingTarget;
    let mut lines: Vec<Line> = Vec::new();
    let char_style = app.config.theme.main;

    let target_encoding = if is_enc2 {
        app.hex_view.get_enc2_table()
    } else {
        app.text_view.table
    };

    let is_current_editing = app.state == UIState::HexEditing && (
        (!is_enc2 && app.hex_view.editing_target == EditingTarget::Enc1) ||
        (is_enc2 && app.hex_view.editing_target == EditingTarget::Enc2)
    );

    let cell_hl_style = if is_current_editing {
        app.config.theme.editing
    } else {
        app.config.theme.highlight
    };

    let bytes_per_line = app.config.hex_mode_bytes_per_line.max(1);
    let page_start = app.reader.page_start;
    let page_current_size = app.reader.page_current_size;
    let buffer = app.file_info.get_buffer_ref();
    // Bound by the live mapping, not by `file_info.size`: the two can disagree
    // and every index below would then be out of bounds.
    let page_bytes_len = buffer
        .len()
        .saturating_sub(page_start)
        .min(page_current_size);

    if page_bytes_len == 0 {
        let paragraph = Paragraph::new(lines).style(app.config.theme.main);
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
        return;
    }

    // Step 1: Build the entire page bytes (applying changed_bytes)
    let mut page_bytes = buffer[page_start..page_start + page_bytes_len].to_vec();
    if !app.hex_view.changed_bytes.is_empty() {
        for (i, b) in page_bytes.iter_mut().enumerate() {
            if let Some(s) = app.hex_view.changed_bytes.get(&(page_start + i))
                && let Ok(parsed) = u8::from_str_radix(s, 16)
            {
                *b = parsed;
            }
        }
    }

    // Step 2: Decode the entire page to find character boundaries
    let is_utf16_le = target_encoding == encoding_rs::UTF_16LE;
    let is_utf16_be = target_encoding == encoding_rs::UTF_16BE;
    let is_utf16 = is_utf16_le || is_utf16_be;
    // `char` is Copy, so this is a single flat allocation. The previous
    // `Vec<(String, usize)>` allocated one String per visible byte, per encoding
    // column, on every frame.
    let non_graphic = app.config.hex_mode_non_graphic_char;
    let mut char_cells: Vec<(char, usize)> = vec![(non_graphic, 1usize); page_bytes_len];

    let mut idx = 0;
    while idx < page_bytes_len {
        if char_cells[idx].1 == 0 {
            idx += 1;
            continue;
        }

        let mut found = false;

        if is_utf16 {
            // UTF-16: manually read 2-byte code units (no encoding_rs)
            if idx + 2 <= page_bytes_len {
                let code_unit = if is_utf16_le {
                    u16::from_le_bytes([page_bytes[idx], page_bytes[idx + 1]])
                } else {
                    u16::from_be_bytes([page_bytes[idx], page_bytes[idx + 1]])
                };

                // Check for high surrogate (U+D800..U+DBFF)
                if (0xD800..=0xDBFF).contains(&code_unit) {
                    if idx + 4 <= page_bytes_len {
                        let low_unit = if is_utf16_le {
                            u16::from_le_bytes([page_bytes[idx + 2], page_bytes[idx + 3]])
                        } else {
                            u16::from_be_bytes([page_bytes[idx + 2], page_bytes[idx + 3]])
                        };
                        if (0xDC00..=0xDFFF).contains(&low_unit) {
                            let code_point = 0x10000
                                + ((code_unit as u32 - 0xD800) << 10)
                                + (low_unit as u32 - 0xDC00);
                            if let Some(c) = char::from_u32(code_point) {
                                if !c.is_control() && !c.is_whitespace() {
                                    let cell_char = if c.is_ascii() {
                                        if c.is_ascii_graphic() { c } else { app.config.hex_mode_non_graphic_char }
                                    } else {
                                        c
                                    };
                                    let char_width = if cell_char.is_ascii() { 1 } else { 2 };
                                    char_cells[idx] = (cell_char, char_width);
                                    for j in 1..4 {
                                        if idx + j < page_bytes_len {
                                            if j < char_width {
                                                char_cells[idx + j] = ('\0', 0);
                                            } else {
                                                char_cells[idx + j] = (' ', 1);
                                            }
                                        }
                                    }
                                    found = true;
                                    idx += 4;
                                }
                            }
                        }
                    }
                    if !found {
                        // Lone high surrogate or invalid pair: only consume 2 bytes
                        char_cells[idx] = (non_graphic, 1);
                        char_cells[idx + 1] = (non_graphic, 1);
                        idx += 2;
                    }
                } else if !(0xDC00..=0xDFFF).contains(&code_unit) {
                    // Normal BMP character (not a surrogate)
                    if let Some(c) = char::from_u32(code_unit as u32) {
                        if !c.is_control() {
                            let cell_char = if c.is_ascii() {
                                if c.is_ascii_graphic() {
                                    c
                                } else {
                                    non_graphic
                                }
                            } else if !c.is_whitespace() {
                                c
                            } else {
                                non_graphic
                            };

                            let char_width = if cell_char.is_ascii() { 1 } else { 2 };
                            char_cells[idx] = (cell_char, char_width);
                            if idx + 1 < page_bytes_len {
                                if char_width > 1 {
                                    char_cells[idx + 1] = ('\0', 0);
                                } else {
                                    char_cells[idx + 1] = (' ', 1);
                                }
                            }
                            found = true;
                        }
                    }

                    if !found {
                        // Control char or invalid: mark both bytes as non-graphic
                        char_cells[idx] = (non_graphic, 1);
                        char_cells[idx + 1] = (non_graphic, 1);
                    }
                    idx += 2;
                } else {
                    // Lone low surrogate (U+DC00..U+DFFF): mark as non-graphic
                    char_cells[idx] = (non_graphic, 1);
                    char_cells[idx + 1] = (non_graphic, 1);
                    idx += 2;
                }
            } else {
                // Lone trailing byte
                char_cells[idx] = (non_graphic, 1);
                idx += 1;
            }
        } else {
            if page_bytes[idx] == 0x0A {
                char_cells[idx] = ('◙', 1);
                idx += 1;
                continue;
            }

            // UTF-8, CP949, CP936, ISO-8859-x, etc.: try lengths 1..=4
            for len in 1..=4 {
                if idx + len > page_bytes_len {
                    continue;
                }
                let slice = &page_bytes[idx..idx + len];
                let (decoded_str, _, had_errors) = target_encoding.decode(slice);

                if !had_errors && decoded_str.chars().count() == 1 {
                    let c = decoded_str.chars().next().unwrap();
                    if c != '\u{FFFD}' && !c.is_control() {
                        let cell_char = if c.is_ascii() {
                            if c.is_ascii_graphic() {
                                c
                            } else {
                                non_graphic
                            }
                        } else if !c.is_whitespace() {
                            c
                        } else {
                            non_graphic
                        };

                        let char_width = if cell_char.is_ascii() { 1 } else { 2 };
                        char_cells[idx] = (cell_char, char_width);
                        for j in 1..len {
                            if idx + j < page_bytes_len {
                                if j < char_width {
                                    char_cells[idx + j] = ('\0', 0);
                                } else {
                                    char_cells[idx + j] = (' ', 1);
                                }
                            }
                        }
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                char_cells[idx] = (non_graphic, 1);
            }

            idx += 1;
        }
    }

    // Step 3: Split into rows and build styled lines
    let mut row_start = 0;
    while row_start < page_bytes_len {
        let row_end = (row_start + bytes_per_line).min(page_bytes_len);

        let mut spans: Vec<Span> = Vec::with_capacity(bytes_per_line);
        let mut col_idx = row_start;
        while col_idx < row_end {
            let (cell_char, byte_len) = char_cells[col_idx];

            if byte_len == 0 {
                col_idx += 1;
                continue;
            }

            let offset = page_start + col_idx;
            let local_col = col_idx - row_start;

            let is_cursor_on_char = app.hex_view.cursor.y == (row_start / bytes_per_line)
                && app.hex_view.cursor.x >= local_col
                && app.hex_view.cursor.x < local_col + byte_len;

            let mut span_style = char_style;

            if is_cursor_on_char {
                span_style = cell_hl_style;
            } else if (app.state == UIState::HexSelection || app.hex_view.selection.start != app.hex_view.selection.end) && app.hex_view.selection.contains(offset) {
                span_style = app.config.theme.highlight;
            } else if app.hex_view.changed_bytes.contains_key(&offset)
                && !app.hex_view.selection.contains(offset)
            {
                span_style = app.config.theme.changed_bytes;
            } else if crate::hex::search::in_match(app, offset) {
                // Search hits are marked in the text columns too, so a match is
                // visible whichever column the eye is on.
                span_style = app.config.theme.byte_highlight;
            }

            if let Some(bg) = block_bg(&app.hex_view.blocks, offset) {
                span_style = span_style.bg(Color::from_u32(bg));
            }

            // If this multi-byte char spans across the row boundary,
            // still render the character at the end of this row (like WinHex)
            let overflows_row = col_idx + byte_len > row_end;
            spans.push(Span::styled(cell_char.to_string(), span_style));
            if overflows_row {
                break;
            }

            col_idx += byte_len;
        }
        lines.push(Line::from(spans));
        row_start += bytes_per_line;
    }

    let enc_name_len = (target_encoding.name().len() + 2) as u16;
    let text_dump_width = (bytes_per_line as u16).max(enc_name_len);
    if is_enc2 {
        app.hex_view.last_enc2_width = text_dump_width;
    } else {
        app.hex_view.last_ascii_width = text_dump_width;
    }

    let paragraph = Paragraph::new(lines).style(app.config.theme.main);
    let bg_fill = Paragraph::new(" ".repeat(area.width as usize)).style(app.config.theme.main);

    frame.render_widget(Clear, area);
    frame.render_widget(bg_fill, area);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod highlight_width_tests {
    use crate::app::App;
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fixtures need unique names: the reader keeps each file mmap'd, so Windows
    /// refuses to overwrite one that an earlier case in the same run opened.
    static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Loads a 0x40-byte fixture whose bytes are `00..3F`.
    fn fixture_app() -> App {
        let dir = std::env::temp_dir().join("dz6_hl_width");
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let n = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("hl_{n}.bin"));
        let bytes: Vec<u8> = (0..0x40u8).collect();
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, true).expect("open fixture");
        app.reader.page_start = 0;
        app.reader.page_current_size = 0x40;
        app.config.hex_mode_bytes_per_line = 16;
        app
    }

    /// Renders the first row and returns each cell's background and symbol.
    fn render_first_row(app: &mut App) -> (Vec<ratatui::style::Color>, Vec<String>) {
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                super::draw_hex_contents(app, f, f.area());
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let bgs = (0..24u16).map(|x| buf[(x, 0)].bg).collect();
        let syms = (0..24u16).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        (bgs, syms)
    }

    /// A multi-byte selection must render as one continuous band.
    ///
    /// Making background-painting styles stop at the two hex digits fixed the
    /// three-column Alt+H marker, but applied unconditionally it also cut the
    /// separator out of every selected byte, so a block came out as a row of
    /// detached stripes with gaps between them.
    #[test]
    fn a_selection_is_a_continuous_band() {
        let mut app = fixture_app();
        // Bytes 1..=3, with the cursor elsewhere so it cannot supply the colour.
        app.hex_view.selection.start = 1;
        app.hex_view.selection.end = 3;
        app.hex_view.offset = 0x20;

        let (bgs, syms) = render_first_row(&mut app);
        let sel_bg = app
            .config
            .theme
            .highlight
            .bg
            .expect("highlight defines a background");
        let main_bg = app.config.theme.main.bg.expect("main defines a background");
        assert_ne!(sel_bg, main_bg, "test needs a distinguishable selection colour");

        // Byte n occupies columns 3n..3n+2, the third being the separator.
        // Columns 3..=11 are bytes 1,2,3 plus the two separators between them.
        let painted: Vec<u16> = (0..24u16).filter(|&x| bgs[x as usize] == sel_bg).collect();
        assert_eq!(
            painted,
            (3..=10).collect::<Vec<u16>>(),
            "selection must be unbroken from the first digit to the last; row: {}",
            syms.join("")
        );
        // The separator *after* the last selected byte stays unpainted, so the
        // band ends where the selection ends.
        assert_eq!(bgs[11], main_bg);
        assert_eq!(bgs[2], main_bg, "the byte before the selection is untouched");
    }

    /// Renders a page whose byte `0x05` is Alt+H highlighted and returns the
    /// background colour of every cell on the first row.
    fn first_row_backgrounds() -> (Vec<ratatui::style::Color>, Vec<String>, App) {
        let dir = std::env::temp_dir().join("dz6_hl_width");
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let n = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("hl_{n}.bin"));
        let bytes: Vec<u8> = (0..0x40u8).collect();
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().unwrap(), 0, true).expect("open fixture");

        // The cursor sits on another row so its own highlight cannot be mistaken
        // for the Alt+H one being measured.
        app.hex_view.offset = 0x30;
        app.hex_view.highlights.insert(0x05);
        app.reader.page_start = 0;
        app.reader.page_current_size = 0x40;
        app.config.hex_mode_bytes_per_line = 16;

        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
        terminal
            .draw(|f| {
                app.screen = f.area();
                super::draw_hex_contents(&mut app, f, f.area());
            })
            .expect("draw");

        let buf = terminal.backend().buffer().clone();
        let bgs = (0..24u16).map(|x| buf[(x, 0)].bg).collect();
        let syms = (0..24u16).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        (bgs, syms, app)
    }

    /// Alt+H highlights a byte *value*, so the block must cover the two hex
    /// digits and stop there. It used to bleed into the column separator, making
    /// the marker three columns wide and visibly misaligned with the grid.
    #[test]
    fn byte_highlight_covers_exactly_two_columns() {
        let (bgs, syms, app) = first_row_backgrounds();
        let hl_bg = app
            .config
            .theme
            .byte_highlight
            .bg
            .expect("byte_highlight defines a background");
        let main_bg = app.config.theme.main.bg;

        assert_ne!(Some(hl_bg), main_bg, "test needs a distinguishable highlight");

        let painted: Vec<u16> = (0..24u16).filter(|&x| bgs[x as usize] == hl_bg).collect();

        // Byte 0x05 is the 6th byte of the row; each byte occupies 3 columns.
        assert_eq!(
            painted,
            vec![15, 16],
            "row: {:?}",
            syms.join("")
        );
        assert_eq!(syms[15], "0");
        assert_eq!(syms[16], "5");
        assert_eq!(
            bgs[17], main_bg.expect("main defines a background"),
            "the separator after a highlighted byte must keep the normal background"
        );
    }
}
