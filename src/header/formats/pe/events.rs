use std::io::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;
use crate::editor::{AppView, UIState};
use crate::header::header_view::HeaderPane;
use crate::header::formats::pe::fields::{self, KvField};
use crate::header::formats::pe::OptionalHeaderLayout;

/// Number of Data Directory entries a PE optional header can hold.
pub const DATA_DIRECTORY_COUNT: usize = 16;

/// Sidebar categories: DOS, COFF, Optional, Data Directories, Sections, Imports,
/// Section Tools.
pub const SIDEBAR_CATEGORIES: usize = 7;

/// Runs the Section Tools action on `row`.
///
/// Shared by Enter and by a mouse click, so the two cannot end up doing different
/// things.
pub fn run_section_tool(app: &mut App, row: usize) {
    match row {
        0 => crate::header::formats::pe::section_tools::align_offset_to_va(
            app,
            app.header_view.tools_section_index,
        ),
        1 => {
            if app.file_info.is_read_only {
                app.read_only_error(crate::i18n::M::RoSectionTools);
            } else {
                app.header_view.section_size_dialog.open("0x1000");
                app.state = UIState::DialogSectionSize;
                app.dialog_renderer =
                    Some(crate::header::formats::pe::section_tools::draw_section_size_dialog);
            }
        }
        _ => {}
    }
}

/// Highest selectable detail row, for the mouse handler.
pub fn max_detail_index_for_mouse(app: &App) -> usize {
    max_detail_index(app).min(4096)
}

/// The header field the detail cursor is on.
///
/// One resolver for the edit dialog and the `g`/`f` jump. The key-value tabs go
/// through [`fields::kv_fields`], the same table `draw.rs` renders, so the row
/// that is highlighted is the field that is edited; the cell-based tabs (Data
/// Directories, Sections) compute their offset from the column.
pub fn field_at_cursor(app: &App) -> Option<KvField> {
    let pe = app.header_view.pe.as_ref()?;
    let idx = app.header_view.detail_index;

    match app.header_view.sidebar_index {
        0..=2 => {
            let mut rows = fields::kv_fields(pe, app.header_view.sidebar_index);
            if idx >= rows.len() {
                return None;
            }
            Some(rows.swap_remove(idx))
        }
        3 => {
            // Clamped here as well as in the `Down` handler: this is the point
            // where the index becomes a file offset, so the bound belongs where
            // the arithmetic happens rather than only where the index is
            // incremented.
            let idx = idx.min(DATA_DIRECTORY_COUNT - 1);
            let dir_names = fields::DATA_DIRECTORY_NAMES;
            let name = dir_names.get(idx).copied().unwrap_or("Data Directory");
            // The array is not at a fixed +96: PE32+ pushes it 24 bytes further
            // out through the wider stack/heap fields.
            let dd_base = OptionalHeaderLayout::from_pe(pe).data_directory(idx);
            if app.header_view.detail_col_index == 0 {
                Some(KvField::cell(dd_base, 4, format!("{} RVA", name)))
            } else {
                Some(KvField::cell(dd_base + 4, 4, format!("{} Size", name)))
            }
        }
        4 => {
            let sec = pe.sections.get(idx)?;
            let sec_name = sec.name().unwrap_or("Section");
            let size_of_opt_hdr = pe.coff_header.size_of_optional_header as usize;
            let sec_base = pe.dos_header.pe_pointer as usize + 24 + size_of_opt_hdr + idx * 40;
            match app.header_view.detail_col_index {
                0 => Some(KvField::cell(sec_base, 8, format!("{}.Name", sec_name))),
                1 => Some(KvField::cell(sec_base + 8, 4, format!("{}.VirtualSize", sec_name))),
                2 => Some(KvField::cell(sec_base + 12, 4, format!("{}.VirtualAddress", sec_name))),
                3 => Some(KvField::cell(sec_base + 16, 4, format!("{}.SizeOfRawData", sec_name))),
                4 => Some(KvField::cell(sec_base + 20, 4, format!("{}.PointerToRawData", sec_name))),
                5 => Some(KvField::cell(sec_base + 36, 4, format!("{}.Characteristics", sec_name))),
                _ => None,
            }
        }
        _ => None,
    }
}


/// Highest valid `detail_index` for the category currently selected.
///
/// `Down` used to be an unclamped `saturating_add(1)`, and the edit path derives
/// a file offset arithmetically from the index: `data_directory(idx)` is an
/// 8-bytes-per-entry stride, and a section entry is `idx * 40`. Only the *label*
/// was bounded (`dir_names.get(idx)`), so holding Down past the end of the table
/// walked the edit target through the section table and on into section data.
fn max_detail_index(app: &App) -> usize {
    match app.header_view.sidebar_index {
        // The key-value tabs are as long as the table that draws them, so End and
        // PageDown land on the last field rather than on a guess.
        0..=2 => app
            .header_view
            .pe
            .as_ref()
            .map(|pe| {
                fields::kv_fields(pe, app.header_view.sidebar_index)
                    .len()
                    .saturating_sub(1)
            })
            .unwrap_or(0),
        3 => DATA_DIRECTORY_COUNT - 1,
        4 => app
            .header_view
            .pe
            .as_ref()
            .map(|pe| pe.sections.len().saturating_sub(1))
            .unwrap_or(0),
        5 => app
            .header_view
            .pe
            .as_ref()
            .map(|pe| pe.imports.len().saturating_sub(1))
            .unwrap_or(0),
        // Section Tools: "Align Offset to VA" and "Add New Section".
        6 => 1,
        _ => usize::MAX,
    }
}

/// Rows a PageUp/PageDown moves by: what the table actually had room for in the
/// last frame, less one so the row at the edge stays visible as an anchor.
fn detail_page_step(app: &App) -> usize {
    app.header_view.last_detail_rows.saturating_sub(1).max(1)
}

/// Moves the detail cursor to `index`, clamped, keeping the Section Tools tab's
/// section choice in step.
fn set_detail_index(app: &mut App, index: usize) {
    app.header_view.detail_index = index.min(max_detail_index(app));
    if app.header_view.sidebar_index == 4 {
        app.header_view.tools_section_index = app.header_view.detail_index;
    }
}

pub fn view_header_pe_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    let is_sidebar = app.header_view.active_pane == HeaderPane::Sidebar;

    match key.code {
        // Tab / BackTab key in Header View: Switch Pane (Sidebar <-> Detail)
        KeyCode::Tab | KeyCode::BackTab => {
            if is_sidebar {
                app.header_view.active_pane = HeaderPane::Detail;
                app.header_view.detail_col_index = 0;
            } else {
                app.header_view.active_pane = HeaderPane::Sidebar;
            }
        }

        // Left / Right / h / l Pane or Column Navigation
        KeyCode::Right if is_sidebar => {
            app.header_view.active_pane = HeaderPane::Detail;
            app.header_view.detail_col_index = 0;
        }
        KeyCode::Left if !is_sidebar => {
            if app.header_view.detail_col_index > 0 {
                app.header_view.detail_col_index -= 1;
            } else {
                app.header_view.active_pane = HeaderPane::Sidebar;
            }
        }
        KeyCode::Right if !is_sidebar => {
            let max_cols = match app.header_view.sidebar_index {
                3 => 2, // Data Directories: 2 editable cols (RVA, Size)
                4 => 6, // Section Headers: 6 editable cols (Name, VSize, VAddr, RSize, RAddr, Flags)
                5 => 4, // Import Directory: 4 cols
                _ => 1,
            };
            if app.header_view.detail_col_index + 1 < max_cols {
                app.header_view.detail_col_index += 1;
            }
        }

        // Sidebar Navigation
        KeyCode::Down if is_sidebar => {
            let max_cat = SIDEBAR_CATEGORIES;
            if app.header_view.sidebar_index < max_cat - 1 {
                app.header_view.sidebar_index += 1;
                app.header_view.detail_index = 0;
                app.header_view.detail_col_index = 0;
            }
        }
        KeyCode::Up if is_sidebar => {
            if app.header_view.sidebar_index > 0 {
                app.header_view.sidebar_index -= 1;
                app.header_view.detail_index = 0;
                app.header_view.detail_col_index = 0;
            }
        }

        // Enter on Sidebar: switch to Detail pane
        KeyCode::Enter if is_sidebar => {
            app.header_view.active_pane = HeaderPane::Detail;
            app.header_view.detail_col_index = 0;
        }

        // Detail Inspector Navigation
        //
        // The page keys and Home/End were missing entirely, which the Import
        // Directory made obvious: 289 entries reachable only one Down at a time.
        KeyCode::Down if !is_sidebar => {
            set_detail_index(app, app.header_view.detail_index.saturating_add(1));
        }
        KeyCode::Up if !is_sidebar => {
            set_detail_index(app, app.header_view.detail_index.saturating_sub(1));
        }
        KeyCode::PageDown if !is_sidebar => {
            let step = detail_page_step(app);
            set_detail_index(app, app.header_view.detail_index.saturating_add(step));
        }
        KeyCode::PageUp if !is_sidebar => {
            let step = detail_page_step(app);
            set_detail_index(app, app.header_view.detail_index.saturating_sub(step));
        }
        KeyCode::Home if !is_sidebar => {
            set_detail_index(app, 0);
        }
        KeyCode::End if !is_sidebar => {
            set_detail_index(app, max_detail_index(app));
        }

        // Enter on the Section Tools tab: run the selected action.
        KeyCode::Enter if !is_sidebar && app.header_view.sidebar_index == 6 => {
            run_section_tool(app, app.header_view.detail_index);
        }

        // Enter on Detail Pane: Open Edit Dialog for the Selected Cell Value
        KeyCode::Enter if !is_sidebar => {
            let Some(field) = field_at_cursor(app) else {
                crate::beep!();
                return Ok(false);
            };

            // Text and blob rows are listed but cannot be typed over as a
            // little-endian integer: Enter on `PE_Signature` would have written
            // four bytes of number where "PE\0\0" is.
            if !field.editable {
                let lang = app.config.lang;
                let message = crate::i18n::fill(
                    crate::i18n::M::ErrFieldNotNumeric.tr(lang),
                    &[&field.name],
                );
                app.error(message);
                return Ok(false);
            }

            let ofs = field.offset;
            app.header_view.edit_offset = ofs;
            app.header_view.edit_size = field.size;
            app.header_view.edit_name = field.name.clone();

            // Prefilled from the *staged* bytes, not the file on disk.
            //
            // These used to be `read_u8`/`read_u32`, which go straight to the
            // mapping and ignore `changed_bytes`, so reopening a field that had
            // just been edited showed the old value - and pressing Enter on it
            // wrote that old value straight back over the edit.
            if field.name.ends_with(".Name") {
                let mut name_bytes = Vec::new();
                for i in 0..8 {
                    let b = crate::hex::edit::displayed_byte(app, ofs + i);
                    if b == 0 {
                        break;
                    }
                    name_bytes.push(b);
                }
                let sec_str = String::from_utf8_lossy(&name_bytes).to_string();
                app.goto_input = tui_input::Input::new(sec_str);
            } else {
                let mut cur_val: u64 = 0;
                for i in 0..field.size.min(8) {
                    cur_val |= (crate::hex::edit::displayed_byte(app, ofs + i) as u64) << (8 * i);
                }
                app.goto_input = tui_input::Input::new(format!("0x{:X}", cur_val));
            }

            // Whole value pre-selected with the cursor at the end, so the
            // user can either type a replacement straight away or press
            // Enter to keep the current value. `Input::new` already parks
            // the cursor at the end.
            app.goto_selection_all = true;
            app.goto_selection_anchor = None;
            app.state = UIState::DialogHeaderEdit;
        }
        // 'g' or 'f' on Detail Pane: Jump to offset in Hex View
        KeyCode::Char('g') | KeyCode::Char('f') if !is_sidebar => {
            let pe_ref = match &app.header_view.pe {
                Some(pe) => pe,
                None => return Ok(false),
            };

            let mut target_offset: Option<usize> = None;

            match app.header_view.sidebar_index {
                // The key-value tabs jump to the field the cursor is on, resolved
                // through the same table that draws it. Three hand-maintained
                // offset lists used to live here and none of them agreed with the
                // rows on screen.
                0..=2 => {
                    target_offset = field_at_cursor(app).map(|f| f.offset);
                }
                3 => {
                    if let Some(opt) = &pe_ref.optional_header {
                        let data_dirs = &opt.data_directories.data_directories;
                        let idx = app.header_view.detail_index;
                        if let Some(Some((_, dd))) = data_dirs.get(idx) {
                            if dd.virtual_address > 0 {
                                if let Some(ofs) = app.va_to_offset(dd.virtual_address as u64) {
                                    target_offset = Some(ofs);
                                }
                            }
                        }
                    }
                }
                4 => {
                    let idx = app.header_view.detail_index;
                    if let Some(sec) = pe_ref.sections.get(idx) {
                        target_offset = Some(sec.pointer_to_raw_data as usize);
                    }
                }
                5 => {
                    let idx = app.header_view.detail_index;
                    if let Some(imp) = pe_ref.imports.get(idx) {
                        target_offset = Some(imp.offset);
                    }
                }
                _ => {}
            }

            if let Some(ofs) = target_offset {
                app.goto(ofs);
                app.editor_view = AppView::Hex;
            }
        }

        // Esc / q: Exit Header View
        KeyCode::Esc | KeyCode::Char('q') => {
            app.editor_view = AppView::Hex;
        }

        _ => {}
    }

    Ok(false)
}

#[cfg(test)]
mod detail_index_bounds_tests {
    use super::*;
    use crate::header::header_view::HeaderPane;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn loaded_app() -> Option<App> {
        let mut app = App::new();
        let exe = std::env::current_exe().ok()?.to_str()?.to_string();
        app.load_file(&exe, 0, true).ok()?;
        app.header_view.pe.as_ref()?; // only meaningful for a real PE
        app.header_view.active_pane = HeaderPane::Detail;
        Some(app)
    }

    fn press_down(app: &mut App) {
        let key = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = view_header_pe_events(app, key);
    }

    /// Holding Down in the Data Directory table must stop at the last entry.
    ///
    /// The index is turned into a file offset with an 8-byte stride, so an
    /// unbounded index walked the edit target out of the directory array, through
    /// the section table and into section data. Only the row *label* was bounded.
    #[test]
    fn data_directory_index_is_capped() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.sidebar_index = 3;
        app.header_view.detail_index = 0;

        for _ in 0..100 {
            press_down(&mut app);
        }

        assert_eq!(
            app.header_view.detail_index,
            DATA_DIRECTORY_COUNT - 1,
            "Data Directory index ran past the 16-entry array"
        );
    }

    /// Same for the section table, which uses a 40-byte stride.
    #[test]
    fn section_index_is_capped_to_the_real_section_count() {
        let Some(mut app) = loaded_app() else { return };
        let sections = app
            .header_view
            .pe
            .as_ref()
            .map(|pe| pe.sections.len())
            .unwrap_or(0);
        if sections == 0 {
            return;
        }
        app.header_view.sidebar_index = 4;
        app.header_view.detail_index = 0;

        for _ in 0..200 {
            press_down(&mut app);
        }

        assert_eq!(app.header_view.detail_index, sections - 1);
        // The Section Tools tab tracks the same choice and must stay in range.
        assert_eq!(app.header_view.tools_section_index, sections - 1);
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let _ = view_header_pe_events(app, key);
    }

    /// The offsets of a handful of fields, pinned against the PE specification
    /// rather than against our own table.
    ///
    /// The alignment test in `fields.rs` compares the two consumers of the table,
    /// which cannot fail if both read the same (wrong) list. This is the other
    /// half: it fails if the table itself moves.
    #[test]
    fn known_fields_sit_where_the_specification_says() {
        let Some(app) = loaded_app() else { return };
        let pe_ptr = app
            .header_view
            .pe
            .as_ref()
            .map(|pe| pe.dos_header.pe_pointer as usize)
            .unwrap();
        let pe = app.header_view.pe.as_ref().unwrap();

        let coff = fields::kv_fields(pe, 1);
        // The COFF header starts with the 4-byte "PE\0\0" signature; Machine is
        // the first field after it. This is the row the edit path used to skip,
        // which shifted every COFF edit down by one field.
        assert_eq!((coff[0].name.as_str(), coff[0].offset), ("PE_Signature", pe_ptr));
        assert_eq!((coff[1].name.as_str(), coff[1].offset), ("Machine", pe_ptr + 4));
        assert_eq!(
            (coff[4].name.as_str(), coff[4].offset),
            ("PointerToSymbolTable", pe_ptr + 12)
        );

        let dos = fields::kv_fields(pe, 0);
        assert_eq!((dos[0].name.as_str(), dos[0].offset), ("DOS_Signature", 0));
        assert_eq!(
            (dos[14].name.as_str(), dos[14].offset),
            ("PE_Header_Offset", 0x3C)
        );

        let opt = fields::kv_fields(pe, 2);
        let opt_off = pe_ptr + 24;
        assert_eq!((opt[0].name.as_str(), opt[0].offset), ("Magic", opt_off));
        assert_eq!((opt[3].name.as_str(), opt[3].offset), ("SizeOfCode", opt_off + 4));
        assert_eq!(
            (opt[9].name.as_str(), opt[9].offset),
            ("SectionAlignment", opt_off + 32)
        );
    }

    /// The edit dialog has to open on the staged value, not the one on disk.
    ///
    /// It read the field with `read_u32`, which goes straight to the mapping and
    /// ignores `changed_bytes`. So a field that had just been edited reopened
    /// showing its old value - and Enter on that value wrote it back over the
    /// edit, which is what "my header edits are not applied" looked like.
    #[test]
    fn the_dialog_opens_on_the_staged_value() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.sidebar_index = 1; // COFF
        app.header_view.detail_index = 3; // TimeDateStamp, 4 bytes

        let field = field_at_cursor(&app).expect("a field under the cursor");
        assert_eq!(field.name, "TimeDateStamp");

        // Stage 0x11223344 over it, the way the edit dialog does.
        for (i, b) in 0x1122_3344u32.to_le_bytes().iter().enumerate() {
            crate::hex::edit::record_edit(&mut app, field.offset + i, *b);
        }

        press(&mut app, KeyCode::Enter);

        assert!(app.state == UIState::DialogHeaderEdit);
        assert_eq!(
            app.goto_input.value(),
            "0x11223344",
            "the dialog reopened on the byte on disk instead of the pending edit"
        );
    }

    /// A text row is listed but must refuse to open as a number.
    #[test]
    fn the_pe_signature_row_refuses_to_be_edited() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.sidebar_index = 1;
        app.header_view.detail_index = 0; // PE_Signature

        press(&mut app, KeyCode::Enter);

        assert!(
            app.state != UIState::DialogHeaderEdit,
            "the signature opened as a numeric field"
        );
        assert!(app.status_error.is_some(), "the refusal has to be reported");
    }

    /// The fixed-layout tables are left alone, so this change can't have broken
    /// navigation in them.
    #[test]
    fn fixed_tables_are_not_capped() {
        let Some(mut app) = loaded_app() else { return };
        app.header_view.sidebar_index = 0; // DOS header
        app.header_view.detail_index = 0;
        press_down(&mut app);
        assert_eq!(app.header_view.detail_index, 1);
    }
}
