use ratatui::{
    Frame,
    crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::{
    fs,
    io::Result,
    path::{Path, PathBuf},
    time::SystemTime,
};
use crate::{app::App, editor::UIState, util::center_widget};

#[cfg(windows)]
unsafe extern "system" {
    fn GetVolumeInformationW(
        lpRootPathName: *const u16,
        lpVolumeNameBuffer: *mut u16,
        nVolumeNameSize: u32,
        lpVolumeSerialNumber: *mut u32,
        lpMaximumComponentLength: *mut u32,
        lpFileSystemFlags: *mut u32,
        lpFileSystemNameBuffer: *mut u16,
        nFileSystemNameSize: u32,
    ) -> i32;
}

#[cfg(windows)]
fn get_volume_label(drive_root: &str) -> String {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let wide_path: Vec<u16> = OsStr::new(drive_root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut volume_name_buf = [0u16; 261];
    unsafe {
        if GetVolumeInformationW(
            wide_path.as_ptr(),
            volume_name_buf.as_mut_ptr(),
            volume_name_buf.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        ) != 0 {
            let len = volume_name_buf.iter().position(|&c| c == 0).unwrap_or(volume_name_buf.len());
            let name = String::from_utf16_lossy(&volume_name_buf[..len]);
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "Local Disk".to_string()
}

#[derive(Clone, Debug)]
pub struct FileDialogItem {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_str: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct FileDialogState {
    pub current_dir: PathBuf,
    pub items: Vec<FileDialogItem>,
    pub selected_index: usize,
    pub scroll_offset: usize,
}

#[derive(Clone, Debug)]
pub struct DriveItem {
    pub letter: char,
    pub path: PathBuf,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct DriveSelectState {
    pub drives: Vec<DriveItem>,
    pub selected_index: usize,
}

impl FileDialogState {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        let canonical_dir = dir.as_ref().canonicalize().unwrap_or_else(|_| dir.as_ref().to_path_buf());
        let mut state = FileDialogState {
            current_dir: canonical_dir,
            items: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
        };
        state.refresh();
        state
    }

    pub fn refresh(&mut self) {
        self.items.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;

        // Parent directory .. if parent exists
        if let Some(parent) = self.current_dir.parent() {
            self.items.push(FileDialogItem {
                name: "..".to_string(),
                is_dir: true,
                size: 0,
                modified_str: "".to_string(),
                path: parent.to_path_buf(),
            });
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();

                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified_str = metadata
                    .and_then(|m| m.modified().ok())
                    .map(format_system_time)
                    .unwrap_or_default();

                let item = FileDialogItem {
                    name,
                    is_dir,
                    size,
                    modified_str,
                    path,
                };

                if is_dir {
                    dirs.push(item);
                } else {
                    files.push(item);
                }
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.items.extend(dirs);
        self.items.extend(files);
    }
}

impl DriveSelectState {
    pub fn new() -> Self {
        let mut drives = Vec::new();

        #[cfg(windows)]
        {
            for c in b'A'..=b'Z' {
                let drive_char = c as char;
                let drive_path_str = format!("{}:\\", drive_char);
                let path = PathBuf::from(&drive_path_str);
                if path.exists() {
                    let vol_label = get_volume_label(&drive_path_str);
                    let label = format!("{} ({}:)", vol_label, drive_char);
                    drives.push(DriveItem {
                        letter: drive_char,
                        path,
                        label,
                    });
                }
            }
        }

        #[cfg(not(windows))]
        {
            drives.push(DriveItem {
                letter: '/',
                path: PathBuf::from("/"),
                label: "Root (/)".to_string(),
            });
            if let Ok(home) = std::env::var("HOME") {
                drives.push(DriveItem {
                    letter: '~',
                    path: PathBuf::from(home),
                    label: "Home (~)".to_string(),
                });
            }
        }

        DriveSelectState {
            drives,
            selected_index: 0,
        }
    }
}

fn format_system_time(time: SystemTime) -> String {
    if let Ok(duration) = time.duration_since(SystemTime::UNIX_EPOCH) {
        let secs = duration.as_secs();
        let days = secs / 86400;
        let day_secs = secs % 86400;
        let hours = day_secs / 3600;
        let minutes = (day_secs % 3600) / 60;
        let (y, m, d) = days_to_date(days);
        format!("{:02}-{:02}-{:04} {:02}:{:02}", d, m, y, hours, minutes)
    } else {
        "".to_string()
    }
}

fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    days += 719468;
    let era = days / 146097;
    let doe = days % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn derive_dialog_bg(base_color: Color) -> Color {
    if let Color::Rgb(r, g, b) = base_color {
        let luminance = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u32;
        if luminance < 128 {
            // Dark themes: Proportional 1.45x scaling preserving hue and saturation (with minimum boost for very dark values)
            let r_out = ((r as f32 * 1.45).max(r as f32 + 16.0)).min(255.0) as u8;
            let g_out = ((g as f32 * 1.45).max(g as f32 + 16.0)).min(255.0) as u8;
            let b_out = ((b as f32 * 1.45).max(b as f32 + 16.0)).min(255.0) as u8;
            Color::Rgb(r_out, g_out, b_out)
        } else {
            // Light themes: Proportional 0.80x scaling
            let r_out = (r as f32 * 0.80) as u8;
            let g_out = (g as f32 * 0.80) as u8;
            let b_out = (b as f32 * 0.80) as u8;
            Color::Rgb(r_out, g_out, b_out)
        }
    } else {
        base_color
    }
}

pub fn draw_file_dialog(app: &mut App, frame: &mut Frame) {
    let theme = &app.config.theme;
    let base_bg = theme.dialog.bg.unwrap_or_else(|| {
        theme.main.bg.unwrap_or(Color::Rgb(25, 30, 40))
    });
    let dialog_bg = derive_dialog_bg(base_bg);
    let dialog_style = theme.dialog.bg(dialog_bg);

    let width = (frame.area().width as u16).saturating_sub(6).clamp(60, 84);
    let height = (frame.area().height as u16).saturating_sub(4).clamp(16, 26);
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);

    let cur_dir_str = app.file_dialog.current_dir.to_string_lossy();
    let clean_dir_str = cur_dir_str.strip_prefix(r"\\?\").unwrap_or(&cur_dir_str);
    let title = format!(
        " {}: {} ",
        crate::i18n::M::OpenFileTitle.tr(app.config.lang),
        clean_dir_str
    );

    let block = Block::bordered()
        .title(title)
        .title_alignment(Alignment::Left)
        .style(dialog_style);

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let list_area = chunks[0];
    let footer_area = chunks[1];

    let visible_rows = list_area.height as usize;
    if visible_rows > 0 {
        if app.file_dialog.selected_index < app.file_dialog.scroll_offset {
            app.file_dialog.scroll_offset = app.file_dialog.selected_index;
        } else if app.file_dialog.selected_index >= app.file_dialog.scroll_offset + visible_rows {
            app.file_dialog.scroll_offset = app.file_dialog.selected_index.saturating_sub(visible_rows - 1);
        }
    }

    let items_len = app.file_dialog.items.len();
    let end_idx = (app.file_dialog.scroll_offset + visible_rows).min(items_len);

    let mut lines = Vec::new();

    for i in app.file_dialog.scroll_offset..end_idx {
        let item = &app.file_dialog.items[i];
        let is_selected = i == app.file_dialog.selected_index;

        let name_col_width = (list_area.width as usize).saturating_sub(32).max(18);

        let formatted_name = if item.is_dir {
            format!("{:<width$}", format!("[{}]", item.name), width = name_col_width)
        } else {
            format!("{:<width$}", item.name, width = name_col_width)
        };

        let type_or_size = if item.is_dir {
            format!("{:>12}", crate::i18n::M::LblSubDir.tr(app.config.lang))
        } else {
            format!("{:>12}", format_bytes(item.size))
        };

        let date_str = format!("{:>18}", item.modified_str);

        let line_text = format!("{} {} {}", formatted_name, type_or_size, date_str);

        let style = if is_selected {
            theme.highlight.add_modifier(Modifier::BOLD)
        } else if item.is_dir {
            theme.offsets.bg(dialog_bg)
        } else {
            dialog_style
        };

        lines.push(Line::from(Span::styled(line_text, style)));
    }

    let list_para = Paragraph::new(lines).style(dialog_style);
    frame.render_widget(list_para, list_area);

    let footer_text = " [Enter] Open  [Ctrl+PgUp] Parent  [Alt+F1] Drives  [Esc] Cancel ";
    let footer_para = Paragraph::new(footer_text)
        .alignment(Alignment::Center)
        .style(theme.topbar);

    frame.render_widget(footer_para, footer_area);
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn draw_drive_dialog(app: &mut App, frame: &mut Frame) {
    let theme = &app.config.theme;
    let base_bg = theme.dialog.bg.unwrap_or_else(|| {
        theme.main.bg.unwrap_or(Color::Rgb(25, 30, 40))
    });
    let dialog_bg = derive_dialog_bg(base_bg);
    let dialog_style = theme.dialog.bg(dialog_bg);

    let drive_cnt = app.drive_dialog.drives.len();
    let width = 46.min(frame.area().width.saturating_sub(4)).max(32);
    let height = ((drive_cnt + 4) as u16).min(frame.area().height.saturating_sub(4)).max(6);
    let dialog_area = center_widget(width, height, frame.area());

    frame.render_widget(Clear, dialog_area);

    let block = Block::bordered()
        .title(crate::i18n::M::SelectDriveTitle.tr(app.config.lang))
        .title_alignment(Alignment::Center)
        .style(dialog_style);

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let mut lines = Vec::new();

    for (idx, drive) in app.drive_dialog.drives.iter().enumerate() {
        let is_selected = idx == app.drive_dialog.selected_index;
        let line_text = format!("  [{}:]  {}", drive.letter, drive.label);

        let style = if is_selected {
            theme.highlight.add_modifier(Modifier::BOLD)
        } else {
            dialog_style
        };

        lines.push(Line::from(Span::styled(line_text, style)));
    }

    let para = Paragraph::new(lines).style(dialog_style);
    frame.render_widget(para, inner);
}

pub fn dialog_file_events(app: &mut App, event: &Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        // Alt+F1: Open drive dialog
        if key.code == KeyCode::F(1) && key.modifiers.contains(KeyModifiers::ALT) {
            app.open_drive_dialog();
            return Ok(false);
        }

        // Ctrl+PageUp or Backspace: Go to parent directory (one level up)
        if (key.code == KeyCode::PageUp && key.modifiers.contains(KeyModifiers::CONTROL))
            || key.code == KeyCode::Backspace
        {
            if let Some(parent) = app.file_dialog.current_dir.parent().map(|p| p.to_path_buf()) {
                let _ = std::env::set_current_dir(&parent);
                app.file_dialog = FileDialogState::new(parent);
            }
            return Ok(false);
        }

        match key.code {
            KeyCode::Esc => {
                if !app.file_info.path.is_empty() {
                    app.state = UIState::Normal;
                    app.dialog_renderer = None;
                } else {
                    app.running = false;
                }
            }
            KeyCode::Up => {
                if app.file_dialog.selected_index > 0 {
                    app.file_dialog.selected_index -= 1;
                }
            }
            KeyCode::Down => {
                if !app.file_dialog.items.is_empty()
                    && app.file_dialog.selected_index < app.file_dialog.items.len() - 1
                {
                    app.file_dialog.selected_index += 1;
                }
            }
            KeyCode::PageUp => {
                app.file_dialog.selected_index = app.file_dialog.selected_index.saturating_sub(10);
            }
            KeyCode::PageDown => {
                if !app.file_dialog.items.is_empty() {
                    app.file_dialog.selected_index =
                        (app.file_dialog.selected_index + 10).min(app.file_dialog.items.len() - 1);
                }
            }
            KeyCode::Home => {
                app.file_dialog.selected_index = 0;
            }
            KeyCode::End => {
                if !app.file_dialog.items.is_empty() {
                    app.file_dialog.selected_index = app.file_dialog.items.len() - 1;
                }
            }
            KeyCode::Enter => {
                if let Some(item) = app.file_dialog.items.get(app.file_dialog.selected_index).cloned() {
                    if item.is_dir {
                        let new_dir = item.path;
                        let _ = std::env::set_current_dir(&new_dir);
                        app.file_dialog = FileDialogState::new(new_dir);
                    } else {
                        let path_str = item.path.to_string_lossy().to_string();
                        match app.load_file(&path_str, 0, false) {
                            Ok(_) => {
                                app.state = UIState::Normal;
                                app.dialog_renderer = None;
                                App::log(app, format!("Opened file '{}'", item.name));
                            }
                            Err(e) => {
                                App::log(app, format!("Error opening '{}': {}", item.name, e));
                            }
                        }
                    }
                }
            }
            KeyCode::Char(c) if c.is_alphanumeric() => {
                let lower_c = c.to_ascii_lowercase();
                if let Some(idx) = app.file_dialog.items.iter().position(|item| {
                    item.name.to_ascii_lowercase().starts_with(lower_c)
                }) {
                    app.file_dialog.selected_index = idx;
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

pub fn dialog_drive_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.dialog_2nd_renderer = None;
            if app.dialog_renderer.is_some() {
                app.state = UIState::DialogFileDialog;
            } else {
                app.state = UIState::Normal;
            }
        }
        KeyCode::Up => {
            if app.drive_dialog.selected_index > 0 {
                app.drive_dialog.selected_index -= 1;
            }
        }
        KeyCode::Down => {
            if !app.drive_dialog.drives.is_empty()
                && app.drive_dialog.selected_index < app.drive_dialog.drives.len() - 1
            {
                app.drive_dialog.selected_index += 1;
            }
        }
        KeyCode::Enter => {
            confirm_drive_selection(app);
        }
        KeyCode::Char(c) if c.is_alphanumeric() => {
            let upper_c = c.to_ascii_uppercase();
            if let Some(idx) = app.drive_dialog.drives.iter().position(|d| d.letter.to_ascii_uppercase() == upper_c) {
                app.drive_dialog.selected_index = idx;
                confirm_drive_selection(app);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn confirm_drive_selection(app: &mut App) {
    if let Some(drive) = app.drive_dialog.drives.get(app.drive_dialog.selected_index).cloned() {
        let _ = std::env::set_current_dir(&drive.path);
        app.file_dialog = FileDialogState::new(&drive.path);
        app.state = UIState::DialogFileDialog;
        app.dialog_renderer = Some(draw_file_dialog);
        app.dialog_2nd_renderer = None;
        App::log(app, format!("Switched drive to {}", drive.path.display()));
    }
}
