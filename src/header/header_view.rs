use std::collections::HashMap;

use ratatui::widgets::TableState;

#[derive(Debug, Clone)]
pub struct PEImport {
    pub dll: String,
    pub name: String,
    pub offset: usize,
    /// Underscored like `_size`: parsed from the import table and kept because it
    /// is part of what an import entry is, but nothing displays it yet.
    pub _ordinal: u16,
    pub rva: usize,
    pub _size: usize,
}

#[derive(Debug, Clone)]
pub struct Pe {
    pub dos_header: goblin::pe::header::DosHeader,
    pub coff_header: goblin::pe::header::CoffHeader,
    pub optional_header: Option<goblin::pe::optional_header::OptionalHeader>,
    pub sections: Vec<goblin::pe::section_table::SectionTable>,
    pub imports: Vec<PEImport>,
}

#[derive(Debug, Clone)]
pub struct Elf {
    pub header: goblin::elf::Header,
    pub phdrs: goblin::elf::ProgramHeaders,
    pub sections: goblin::elf::SectionHeaders,
    pub symtab: Vec<goblin::elf::Sym>,
    pub strtab: HashMap<usize, String>,
}

// The PE side of the header view has no `TableState`s: it tracks its own
// selection with `tab_index` / `sidebar_index` / `detail_index` instead. A
// `PeState` struct holding five unused `TableState`s used to sit here.

#[derive(Debug, Default)]
pub struct ElfState {
    pub elf_header_table_state: TableState,
    pub program_header_table_state: TableState,
    pub sections_table_state: TableState,
    pub symbols_table_state: TableState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderPane {
    Sidebar,
    Detail,
}

impl Default for HeaderPane {
    fn default() -> Self {
        HeaderPane::Sidebar
    }
}

/// Single-line, "select all on open" numeric input used by the "Add New
/// Section" size prompt. Mirrors the goto-dialog's selection model (whole
/// text selected so typing replaces it outright, or Left/Right to edit in
/// place) without dragging in goto's address-specific parsing.
#[derive(Debug, Default)]
pub struct SectionSizeDialog {
    pub input: tui_input::Input,
    pub selection_all: bool,
    pub selection_anchor: Option<usize>,
    pub error_message: Option<String>,
}

impl SectionSizeDialog {
    /// Opens the dialog pre-filled with `text`, fully selected, cursor at the
    /// end - so the user can either just start typing to replace it, or press
    /// Enter to accept the default outright.
    pub fn open(&mut self, text: &str) {
        self.input = tui_input::Input::new(text.to_string());
        self.selection_all = true;
        self.selection_anchor = None;
        self.error_message = None;
    }
}

#[derive(Default, Debug)]
pub struct HeaderView {
    pub pe: Option<Pe>,
    pub elf: Option<Elf>,
    pub elf_state: ElfState,
    pub tab_index: usize,
    pub sidebar_index: usize,
    pub detail_index: usize,
    pub detail_col_index: usize,
    /// Data rows the detail table had room for in the last frame.
    ///
    /// PageUp/PageDown move by whatever is actually on screen rather than a fixed
    /// guess, and the row count is only known while drawing.
    pub last_detail_rows: usize,
    pub active_pane: HeaderPane,
    pub edit_offset: usize,
    pub edit_size: usize,
    pub edit_name: String,
    pub section_size_dialog: SectionSizeDialog,
    /// Which section "Align Offset to VA" (in the Section Tools tab) targets.
    /// Kept separate from `detail_index` because that resets to 0 whenever
    /// the sidebar tab changes, which would make picking a section on the
    /// Section tab and then switching to Section Tools forget the choice.
    pub tools_section_index: usize,
    /// Result of the last Section Tools action ("Align Offset to VA" or "Add
    /// New Section"), shown at the bottom of that tab so running an action
    /// gives visible confirmation instead of only being recorded in the
    /// (not-immediately-visible) log.
    pub tools_last_message: Option<String>,
}
