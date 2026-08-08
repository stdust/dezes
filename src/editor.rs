#[derive(Debug, PartialEq, Copy, Clone)]
pub enum AppView {
    Text,
    Hex,
    Header,
    Disasm,
}

// View cycling lives in `App::switch_editor_view`, which is what Tab is bound to:
// it also has to remember the previous view and skip Disasm on a non-executable.
// Two earlier methods here (`next` and `next_valid`) duplicated parts of that and
// had no callers left.

#[derive(Debug, PartialEq, Copy, Clone, Default)]
pub enum EditingTarget {
    #[default]
    Hex,
    Enc1,
    Enc2,
}

impl EditingTarget {
    pub fn next(&self) -> Self {
        match self {
            EditingTarget::Hex => EditingTarget::Enc1,
            EditingTarget::Enc1 => EditingTarget::Enc2,
            EditingTarget::Enc2 => EditingTarget::Hex,
        }
    }
}

#[derive(PartialEq, Copy, Clone)]
pub enum UIState {
    Command,
    DialogAbout,
    DialogAssemble,
    DialogBase,
    DialogCalculator,
    DialogComment,
    DialogEditData,
    DialogEncoding,
    DialogEncoding2,
    DialogFindPattern,
    DialogGoto,
    DialogHelp,
    DialogSectionSize,
    DialogLog,
    /// The digital-rain easter egg. Not reachable from any documented key.
    Matrix,
    DialogModifyBlock,
    DialogNames,
    DialogNamesRegex,
    DialogReplacePattern,
    DialogSettings,
    DialogStrings,
    /// In-place replacement of the string selected in the F6 list.
    DialogStringEdit,
    DialogStringRef,
    DialogXref,
    DialogFileDialog,
    DialogDriveSelect,
    DialogHeaderEdit,
    Error,
    HexEditing,
    HexSelection,
    Normal,
}
