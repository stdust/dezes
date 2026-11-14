use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::Paragraph,
};

use crate::app::App;
use crate::editor::AppView;

/// Calculate perceptual brightness (luminance 0.0 - 255.0) of a Color.
#[allow(dead_code)]
pub fn color_luminance(c: Color) -> f32 {
    match c {
        Color::Rgb(r, g, b) => 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32,
        Color::Indexed(idx) => idx as f32,
        Color::Black => 0.0,
        Color::White => 255.0,
        Color::Red | Color::LightRed => 76.0,
        Color::Green | Color::LightGreen => 150.0,
        Color::Yellow | Color::LightYellow => 226.0,
        Color::Blue | Color::LightBlue => 29.0,
        Color::Magenta | Color::LightMagenta => 105.0,
        Color::Cyan | Color::LightCyan => 179.0,
        Color::Gray => 128.0,
        Color::DarkGray => 64.0,
        Color::Reset => 0.0,
    }
}

/// Soften/lighten scrollbar track color mathematically by moving 25% (1/4) toward white:
/// channel' = channel + (255 - channel + 2) / 4
/// E.g. #ABB0A9 (171, 176, 169) transforms into #C0C4BF (192, 196, 191).
pub fn soften_track_color(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let lift = |ch: u8| -> u8 {
                let diff = 255u16.saturating_sub(ch as u16);
                ch.saturating_add(((diff + 2) / 4) as u8)
            };
            Color::Rgb(lift(r), lift(g), lift(b))
        }
        Color::Indexed(idx) => match idx {
            0 => Color::Rgb(64, 64, 64),
            8 => Color::Rgb(140, 140, 140),
            7 => Color::Rgb(200, 200, 200),
            _ => Color::Indexed(idx),
        },
        Color::DarkGray => Color::Rgb(112, 112, 112),
        Color::Gray => Color::Rgb(160, 160, 160),
        Color::Black => Color::Rgb(64, 64, 64),
        other => other,
    }
}

/// Compute (thumb_top, thumb_height) within a vertical track of `track_height` rows.
///
/// Guarantees a minimum thumb height of 3 rows (or `track_height` if smaller)
/// so the slider never shrinks to an invisible speck even in huge files.
pub fn compute_thumb_position(
    total: usize,
    visible: usize,
    current: usize,
    track_height: usize,
) -> (usize, usize) {
    if track_height == 0 {
        return (0, 0);
    }
    if total == 0 || total <= visible {
        return (0, track_height);
    }

    let min_thumb = 2.min(track_height);
    let raw_thumb = (visible * track_height) / total.max(1);
    // Reduce thumb height by 40% (60% of proportional height)
    let thumb_height = ((raw_thumb * 60 + 50) / 100).max(min_thumb).min(track_height);

    let max_top = track_height.saturating_sub(thumb_height);
    let max_scroll = total.saturating_sub(visible);

    let thumb_top = if max_scroll > 0 {
        let cur = current.min(max_scroll) as u64;
        let m_top = max_top as u64;
        let m_sc = max_scroll as u64;
        ((cur * m_top + (m_sc / 2)) / m_sc) as usize
    } else {
        0
    };

    (thumb_top.min(max_top), thumb_height)
}

/// Draw a prominent 2-column wide vertical scrollbar without arrows.
/// Colors dynamically follow the theme's offsets background (Plan A) to avoid clumping with header.
pub fn draw_scrollbar(app: &App, frame: &mut Frame, area: Rect) {
    if area.width < 2 || area.height == 0 || !app.config.scrollbar {
        return;
    }

    let track_height = area.height as usize;
    let bpl = app.config.hex_mode_bytes_per_line.max(1);

    let (total, visible, current) = match app.editor_view {
        AppView::Hex => {
            let total = app.file_info.size;
            let visible = (track_height * bpl).min(total);
            let current = app.reader.page_start.min(total);
            (total, visible, current)
        }
        AppView::Disasm => {
            let total = app.file_info.size;
            let visible = (track_height * 4).min(total);
            let current = app.reader.page_start.min(total);
            (total, visible, current)
        }
        AppView::Text => {
            let total = app.file_info.size;
            let visible = (track_height * 40).min(total);
            let current = app.reader.page_start.min(total);
            (total, visible, current)
        }
        AppView::Header => {
            let total = app.file_info.size;
            let visible = (track_height * 16).min(total);
            let current = app.reader.page_start.min(total);
            (total, visible, current)
        }
    };

    let (thumb_top, thumb_height) =
        compute_thumb_position(total, visible, current, track_height);

    // Derive thumb color from theme's offsets background (Plan A) with fallback:
    let main_bg = app.config.theme.main.bg.unwrap_or(Color::Reset);
    let thumb_color = match app.config.theme.offsets.bg {
        Some(c) if Some(c) != app.config.theme.main.bg => c,
        _ => app.config.theme.topbar.bg.unwrap_or(Color::Rgb(194, 194, 190)),
    };
    // Mathematically lighten track line (e.g. #ABB0A9 -> #C7CAC6) across all themes:
    let raw_track_color = app.config.theme.topbar.bg.unwrap_or(Color::Rgb(171, 176, 169));
    let track_color = soften_track_color(raw_track_color);

    let thumb_style = Style::default().fg(thumb_color).bg(main_bg);
    let track_style = Style::default().fg(track_color).bg(main_bg);

    let mut text_lines: Vec<ratatui::text::Line> = Vec::with_capacity(track_height);
    for row in 0..track_height {
        let is_thumb = row >= thumb_top && row < thumb_top + thumb_height;
        if is_thumb {
            text_lines.push(ratatui::text::Line::from(vec![Span::styled(
                "██",
                thumb_style,
            )]));
        } else {
            text_lines.push(ratatui::text::Line::from(vec![Span::styled(
                "░░",
                track_style,
            )]));
        }
    }

    let para = Paragraph::new(text_lines);
    frame.render_widget(para, area);
}

/// Handle mouse clicks and drags on the scrollbar track to jump/scroll proportionally.
pub fn handle_mouse_click(app: &mut App, mouse_row: u16) {
    let area = app.scrollbar_area;
    if area.height == 0 || app.file_info.size == 0 {
        return;
    }

    let rel_row = (mouse_row.saturating_sub(area.y)) as usize;
    let track_height = area.height as usize;
    let rel_row = rel_row.min(track_height.saturating_sub(1));

    let fraction = rel_row as f64 / (track_height.saturating_sub(1)).max(1) as f64;

    match app.editor_view {
        AppView::Hex => {
            let bpl = app.config.hex_mode_bytes_per_line.max(1);
            let total = app.file_info.size;
            let visible = (track_height * bpl).min(total);
            let max_scroll = total.saturating_sub(visible);
            let target = (fraction * max_scroll as f64) as usize;
            let aligned = (target / bpl) * bpl;

            app.reader.page_start = aligned;
            app.hex_view.offset = aligned;
            app.hex_view.cursor.y = 0;
            app.hex_view.cursor.x = 0;
            app.hex_view.selection.start = 0;
            app.hex_view.selection.end = 0;
            app.state = crate::editor::UIState::Normal;
        }
        AppView::Disasm => {
            let total = app.file_info.size;
            let visible = (track_height * 4).min(total);
            let max_scroll = total.saturating_sub(visible);
            let target = (fraction * max_scroll as f64) as usize;

            app.reader.page_start = target;
            app.hex_view.offset = target;
            app.disasm_selection_anchor = None;
            app.align_page_for_view();
        }
        AppView::Text => {
            let total = app.file_info.size;
            let visible = (track_height * 40).min(total);
            let max_scroll = total.saturating_sub(visible);
            let target = (fraction * max_scroll as f64) as usize;
            app.reader.page_start = target;
            app.text_view.cursor = (0, 0);
            app.hex_view.offset = target;
        }
        AppView::Header => {}
    }
}

#[cfg(test)]
mod scrollbar_tests {
    use super::*;

    #[test]
    fn test_compute_thumb_position_empty_file() {
        let (top, height) = compute_thumb_position(0, 100, 0, 25);
        assert_eq!(top, 0);
        assert_eq!(height, 25);
    }

    #[test]
    fn test_compute_thumb_position_fits_in_screen() {
        let (top, height) = compute_thumb_position(200, 500, 0, 25);
        assert_eq!(top, 0);
        assert_eq!(height, 25);
    }

    #[test]
    fn test_compute_thumb_position_huge_file_minimum_height() {
        // 100MB file, 25 rows: thumb maintains minimum height of 2 rows
        let (top, height) = compute_thumb_position(100_000_000, 400, 0, 25);
        assert_eq!(top, 0);
        assert_eq!(height, 2, "thumb must maintain minimum height of 2 rows");
    }

    #[test]
    fn test_compute_thumb_position_40_percent_reduction() {
        // total = 100, visible = 50, track = 20
        // raw_thumb = (50 * 20) / 100 = 10
        // reduced by 40% -> 6
        let (top, height) = compute_thumb_position(100, 50, 0, 20);
        assert_eq!(top, 0);
        assert_eq!(height, 6);
    }

    #[test]
    fn test_compute_thumb_position_end_of_file() {
        let total = 10_000;
        let visible = 1_000;
        let max_scroll = total - visible;
        let track_height = 20;

        let (top, height) = compute_thumb_position(total, visible, max_scroll, track_height);
        assert_eq!(top + height, track_height, "thumb must reach the bottom at max scroll");
    }

    #[test]
    fn test_color_luminance() {
        assert!(color_luminance(Color::White) > 250.0);
        assert!(color_luminance(Color::Black) < 1.0);
        assert!(color_luminance(Color::Rgb(255, 255, 255)) > 250.0);
        assert!(color_luminance(Color::Rgb(0, 0, 0)) < 1.0);
    }

    #[test]
    fn test_soften_track_color_mathematical_precision() {
        // 25% lift toward white: #ABB0A9 -> #C0C4BF
        assert_eq!(
            soften_track_color(Color::Rgb(0xAB, 0xB0, 0xA9)),
            Color::Rgb(0xC0, 0xC4, 0xBF)
        );
    }
}
