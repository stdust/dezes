//! Interface language: English, Korean and Chinese.
//!
//! Only the words are translated - key names (`F1`, `Ctrl+C`), option names
//! (`byteline`), status-bar mode labels (`HEX`, `DISASM`) and everything a user
//! types stay as they are. Those are identifiers, and translating them would mean
//! the documentation, the `:set` command and the screen no longer agree.
//!
//! The table is an exhaustive `match` rather than an indexed array on purpose: add
//! a message and the compiler names every language that still has to be filled in.
//! An array indexed by an enum discriminant compiles happily when the two drift
//! apart, and the bug shows up as the wrong word on screen.
//!
//! CJK text is double-width in a terminal, so every caller that measures a
//! translated string has to use `unicode_width`, not `chars().count()`. The hint
//! bar and the `:set` table do.
//!
//! # Korean terminology
//!
//! Reverse-engineering jargon is transliterated, not translated: `offset` is
//! `옵셋`, `disassemble` is `디스어셈블`, `import table` is `임포트 테이블`,
//! `base` is `베이스`, `hex` is `헥스`. A Korean reader of this program already
//! knows the English terms, and a native coinage (`상쇄`, `역어셈블`) reads as a
//! different concept. Acronyms stay as they are - `Xref`, `VA`, `NOP`, `PE`.
//! Words that have a settled Korean form (`주석`, `문자열`, `블록`, `인코딩`) keep
//! it. Chinese follows its own conventions, where translated forms such as `偏移`
//! and `反汇编` are the standard ones.

/// Interface language.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    #[default]
    En,
    Ko,
    Zh,
}

impl Lang {
    /// Canonical name, as `:set lang` takes it and `.dz6init` stores it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
            Lang::Zh => "zh",
        }
    }

    /// Name in the language itself, for the settings table.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "en (English)",
            Lang::Ko => "ko (한국어)",
            Lang::Zh => "zh (中文)",
        }
    }

    /// Accepts the canonical names plus the spellings people actually type.
    pub fn from_name(name: &str) -> Option<Lang> {
        match name.trim().to_ascii_lowercase().as_str() {
            "en" | "eng" | "english" => Some(Lang::En),
            "ko" | "kr" | "kor" | "korean" | "한국어" => Some(Lang::Ko),
            "zh" | "cn" | "chs" | "chinese" | "中文" => Some(Lang::Zh),
            _ => None,
        }
    }

    /// Every language, for error messages and the settings table.
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ko, Lang::Zh];
}

/// A translatable message.
///
/// Named after what it says, not where it appears, so the same words are not
/// translated twice.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum M {
    // Hint bar - function keys
    Help,
    Edit,
    HeaderView,
    Refs,
    Strings,
    TextView,
    About,
    Open,
    Save,
    SaveQuit,
    HexView,
    // Hint bar - edit mode
    Type,
    Column,
    Case,
    Select,
    Done,
    // Hint bar - selection
    Copy,
    Modify,
    Color,
    Clear,
    // Hint bar - Ctrl page
    Data,
    Goto,
    Find,
    Replace,
    Xref,
    Addr,
    Undo,
    Redo,
    // Hint bar - Alt page
    Encoding,
    Encoding2,
    Highlight,
    Log,
    Names,
    RevertByte,
    ImageBase,
    DecodeWidth,
    // Status bar
    ReadOnly,
    // Dialog titles and footers
    HelpTitle,
    HelpFooter,
    SettingsTitle,
    SettingsFooter,
    NamesFooter,
    MinimumLength,
    // Dialog titles. Ones that carry a number or an address are a prefix plus the
    // value, so only the words are here.
    AboutTitle,
    AboutFooter,
    LogTitle,
    LogFooter,
    CalculatorTitle,
    GotoTitle,
    ModifyBlockTitle,
    OperationTitle,
    NamesTitle,
    StringsTitle,
    RegexTitle,
    FilterRegexTitle,
    AssembleTitle,
    AddSectionTitle,
    SelectDriveTitle,
    CommentAtTitle,
    XrefTitle,
    XrefLimitReached,
    StringRefsTitle,
    OpenFileTitle,
    ImageBaseTitle,
    EditDataTitle,
    FoundCount,

    // Refusals and errors.
    //
    // Templates carry `{}` placeholders filled by [`fill`], so one message covers
    // every option that shares a shape - all six on/off options report a bad value
    // through `ErrSwitchValue`.
    ReadOnlyRefused,
    RoEditData,
    RoPaste,
    RoCase,
    RoEditMode,
    RoFillZero,
    RoFillNop,
    RoModifyBlock,
    RoAssemble,
    RoNopOut,
    RoSectionTools,
    RoStringEdit,
    ErrNothingSelected,
    ErrSaveFailedQuit,
    ErrSaveError,
    ErrCommentOutside,
    ErrRefusingAssemble,
    ErrFailedAssemble,
    ErrSwitchValue,
    ErrNeedsNumberAuto,
    ErrBytelineZero,
    ErrNotByteCount,
    ErrNeedsCharacter,
    ErrOneCharacter,
    ErrViewNames,
    ErrAddrNames,
    ErrNoCodeSection,
    ErrLangNeedsValue,
    ErrUnknownLang,
    ErrUnknownOptionSuggest,
    ErrUnknownOption,
    ErrNeedsColour,
    ErrNotColour,
    ErrUnknownEncoding,
    /// Warning for a filter regex whose only match was a zero-length one.
    WarnRegexEmptyOnly,
    /// Title of the in-place string replacement box.
    StringEditTitle,
    /// The replacement does not fit in the space the original occupies.
    ErrStringTooLong,
    /// Report of a completed replacement.
    StringReplaced,
    /// The "every encoding" choice in the references dialog's encoding filter.
    LblAllEncodings,
    /// Key hints on the strings dialog's filter box.
    StringsFooterKeys,
    /// Key hints on the string-references dialog.
    RefsFooterKeys,
    /// Key hints on the cross-references dialog.
    XrefFooterKeys,

    // Dialog contents: column headers, field labels and inline messages.
    LblType,
    LblAddress,
    LblInstruction,
    LblDisassembly,
    LblTextString,
    LblValue,
    LblStep,
    LblSearch,
    LblReplace,
    LblSubDir,
    LblError,
    ReplacePatternTitle,
    ReplaceHint,
    FindPatternTitle,
    FindHint,
    BytesSelected,
    MatchAtOffset,
    MatchAtVa,
    ReplacedAt,
    ReplacedCount,
    NotAtAMatch,
    FindEnterHex,
    FindInvalidHex,
    FindEnterText,
    FindNoMatch,
    SizeHexHint,
    ErrFieldNotNumeric,
    ErrSectionSizeZero,
    ErrNoPeHeaders,
    ErrNoOptionalHeader,
    ErrSectionTooBig,

    // Modify Block operations.
    OpAdd,
    OpSub,
    OpMul,
    OpDiv,
    OpXor,
    OpOr,
    OpAnd,
    OpNot,
    OpEndianSwap,
    OpShiftLeft,
    OpShiftRight,
    OpRandom,
    OpRollingXor,

    // Settings table notes
    NoteByteline,
    NoteCtrlchar,
    NoteEnc1,
    NoteEnc2,
    NoteAddr,
    NoteBitness,
    NoteView,
    NoteTheme,
    NoteDb,
    NoteDimctrl,
    NoteDimzero,
    NoteWrapscan,
    NoteHighlight,
    NoteHintbar,
    NoteLang,
    NoteDisasmColor,
}

/// Fills the `{}` placeholders in a translated template, in order.
///
/// `format!` needs a literal, and these templates are values - one per language -
/// so the substitution is done by hand. Placeholders are positional, so a
/// translation must keep them in the same order; none of the messages here need to
/// reorder them.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    for arg in args {
        match rest.split_once("{}") {
            Some((before, after)) => {
                out.push_str(before);
                out.push_str(arg);
                rest = after;
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

impl M {
    /// Every message, so the tests can walk the whole table.
    ///
    /// Rust has no way to enumerate a plain enum's variants without a macro or a
    /// nightly feature, so this is maintained by hand: a new variant belongs here
    /// as well as in `table`.
    #[cfg(test)]
    pub const ALL: &[M] = &[
        M::Help, M::Edit, M::HeaderView, M::Refs, M::Strings, M::TextView, M::About,
        M::Open, M::Save, M::SaveQuit, M::HexView, M::Type, M::Column, M::Case, M::Select, M::Done,
        M::Copy, M::Modify, M::Color, M::Clear, M::Data, M::Goto, M::Find, M::Replace,
        M::Xref, M::Addr, M::Undo, M::Redo, M::Encoding, M::Encoding2, M::Highlight,
        M::Log, M::Names, M::RevertByte, M::ImageBase, M::DecodeWidth, M::ReadOnly,
        M::HelpTitle, M::HelpFooter, M::SettingsTitle, M::SettingsFooter, M::NamesFooter,
        M::MinimumLength, M::NoteByteline, M::NoteCtrlchar, M::NoteEnc1, M::NoteEnc2,
        M::NoteAddr, M::NoteBitness, M::NoteView, M::NoteTheme, M::NoteDb, M::NoteDimctrl,
        M::NoteDimzero, M::NoteWrapscan, M::NoteHighlight, M::NoteHintbar, M::NoteLang,
        M::NoteDisasmColor, M::AboutTitle, M::AboutFooter, M::LogTitle, M::LogFooter,
        M::CalculatorTitle, M::GotoTitle, M::ModifyBlockTitle, M::OperationTitle,
        M::NamesTitle, M::StringsTitle, M::RegexTitle, M::FilterRegexTitle,
        M::AssembleTitle, M::AddSectionTitle, M::SelectDriveTitle, M::CommentAtTitle,
        M::XrefTitle, M::XrefLimitReached, M::StringRefsTitle, M::OpenFileTitle,
        M::ImageBaseTitle, M::EditDataTitle, M::FoundCount,
        // Refusals and errors
        M::ReadOnlyRefused, M::RoEditData, M::RoPaste, M::RoCase, M::RoEditMode,
        M::RoFillZero, M::RoFillNop, M::RoModifyBlock, M::RoAssemble, M::RoNopOut,
        M::RoSectionTools, M::RoStringEdit,
        M::ErrNothingSelected, M::ErrSaveFailedQuit, M::ErrSaveError,
        M::ErrCommentOutside, M::ErrRefusingAssemble, M::ErrFailedAssemble,
        M::ErrSwitchValue, M::ErrNeedsNumberAuto, M::ErrBytelineZero,
        M::ErrNotByteCount, M::ErrNeedsCharacter, M::ErrOneCharacter, M::ErrViewNames,
        M::ErrAddrNames, M::ErrNoCodeSection, M::ErrLangNeedsValue, M::ErrUnknownLang,
        M::ErrUnknownOptionSuggest, M::ErrUnknownOption, M::ErrNeedsColour,
        M::ErrNotColour, M::ErrUnknownEncoding, M::WarnRegexEmptyOnly,
        M::StringEditTitle, M::ErrStringTooLong, M::StringReplaced,
        M::StringsFooterKeys, M::RefsFooterKeys, M::XrefFooterKeys,
        M::LblAllEncodings,
        // Dialog contents
        M::LblType, M::LblAddress, M::LblInstruction, M::LblDisassembly,
        M::LblTextString, M::LblValue, M::LblStep, M::LblSearch, M::LblReplace,
        M::LblSubDir, M::LblError, M::ReplacePatternTitle, M::ReplaceHint,
        M::FindPatternTitle, M::FindHint, M::BytesSelected, M::MatchAtOffset, M::MatchAtVa,
        M::ReplacedAt, M::ReplacedCount, M::NotAtAMatch,
        M::FindEnterHex, M::FindInvalidHex, M::FindEnterText,
        M::FindNoMatch, M::SizeHexHint, M::ErrFieldNotNumeric, M::ErrSectionSizeZero, M::ErrNoPeHeaders,
        M::ErrNoOptionalHeader, M::ErrSectionTooBig,
        M::OpAdd, M::OpSub, M::OpMul, M::OpDiv, M::OpXor, M::OpOr, M::OpAnd, M::OpNot,
        M::OpEndianSwap, M::OpShiftLeft, M::OpShiftRight, M::OpRandom, M::OpRollingXor,
    ];

    /// The message in `lang`.
    pub fn tr(self, lang: Lang) -> &'static str {
        let [en, ko, zh] = self.table();
        match lang {
            Lang::En => en,
            Lang::Ko => ko,
            Lang::Zh => zh,
        }
    }

    /// `[English, Korean, Chinese]` for one message.
    ///
    /// Kept short: these are hint-bar slots and dialog titles, where a long
    /// translation costs columns that the narrowest supported terminal (68) does
    /// not have.
    fn table(self) -> [&'static str; 3] {
        match self {
            M::Help => ["Help", "도움말", "帮助"],
            M::Edit => ["Edit", "편집", "编辑"],
            M::HeaderView => ["Header", "헤더", "头部"],
            M::Refs => ["Refs", "참조", "引用"],
            M::Strings => ["Strings", "문자열", "字符串"],
            M::TextView => ["Text", "텍스트", "文本"],
            M::About => ["About", "정보", "关于"],
            M::Open => ["Open", "열기", "打开"],
            M::Save => ["Save", "저장", "保存"],
            // F12 writes the file *and* leaves, which "Save" alone does not say.
            // Used when the line has room; `M::Save` is the fallback when it does not.
            M::SaveQuit => ["Save and quit", "저장하고 종료", "保存并退出"],
            M::HexView => ["Hex", "헥스", "十六"],

            M::Type => ["Type", "입력", "输入"],
            M::Column => ["Column", "칼럼", "列"],
            M::Case => ["Case", "대소문자", "大小写"],
            M::Select => ["Select", "선택", "选择"],
            M::Done => ["Done", "완료", "完成"],

            M::Copy => ["Copy", "복사", "复制"],
            M::Modify => ["Modify", "일괄수정", "批量修改"],
            M::Color => ["Color", "색칠", "着色"],
            M::Clear => ["Clear", "해제", "清除"],

            M::Data => ["Data", "데이터", "数据"],
            M::Goto => ["Goto", "이동", "跳转"],
            M::Find => ["Find", "찾기", "查找"],
            M::Replace => ["Replace", "바꾸기", "替换"],
            M::Xref => ["Xref", "Xref", "交叉引用"],
            M::Addr => ["Addr", "주소", "地址"],
            M::Undo => ["Undo", "되돌리기", "撤销"],
            M::Redo => ["Redo", "다시실행", "重做"],

            M::Encoding => ["Enc", "인코딩", "编码"],
            M::Encoding2 => ["Enc2", "인코딩2", "编码2"],
            M::Highlight => ["Hilite", "강조", "高亮"],
            M::Log => ["Log", "로그", "日志"],
            M::Names => ["Names", "주석목록", "注释列表"],
            M::RevertByte => ["Revert", "복원", "还原"],
            M::ImageBase => ["Base", "베이스", "基址"],
            M::DecodeWidth => ["Width", "비트수", "位宽"],

            M::ReadOnly => ["Read Only", "읽기 전용", "只读"],

            M::HelpTitle => [" Help (F1) ", " 도움말 (F1) ", " 帮助 (F1) "],
            // Only the part that is not guessable. Arrows scrolling a long text and
            // Esc closing a dialog are true everywhere in the program; spelling them
            // out here spent the whole footer on them and left no room to mention
            // the one key nobody would try.
            M::HelpFooter => [
                " y copy to clipboard ",
                " y 클립보드 복사 ",
                " y 复制到剪贴板 ",
            ],
            M::SettingsTitle => [
                " Settings (:set <name> <value> to change) ",
                " 설정 (변경: :set <이름> <값>) ",
                " 设置 (修改: :set <名称> <值>) ",
            ],
            M::SettingsFooter => [
                " Up/Down to scroll, Esc to close ",
                " 위/아래 스크롤, Esc 닫기 ",
                " 上/下 滚动，Esc 关闭 ",
            ],
            M::NamesFooter => [
                " Enter goto │ F2 edit │ Del delete │ f filter │ o/n sort ",
                " Enter 이동 │ F2 수정 │ Del 삭제 │ f 필터 │ o/n 정렬 ",
                " Enter 跳转 │ F2 编辑 │ Del 删除 │ f 过滤 │ o/n 排序 ",
            ],
            M::MinimumLength => ["Minimum length", "최소 길이", "最小长度"],

            M::AboutTitle => [" About Dezes (F8) ", " Dezes 정보 (F8) ", " 关于 Dezes (F8) "],
            M::AboutFooter => [
                " y copy to clipboard, Esc close ",
                " y 클립보드 복사, Esc 닫기 ",
                " y 复制到剪贴板，Esc 关闭 ",
            ],
            M::LogTitle => [" Log ", " 로그 ", " 日志 "],
            M::LogFooter => [
                " y copy, Delete clear, Esc close ",
                " y 복사, Delete 지우기, Esc 닫기 ",
                " y 复制，Delete 清空，Esc 关闭 ",
            ],
            M::CalculatorTitle => [" Calculator ", " 계산기 ", " 计算器 "],
            M::GotoTitle => [" Goto Address ", " 주소로 이동 ", " 跳转到地址 "],
            M::ModifyBlockTitle => ["Modify Block Data", "블록 데이터 일괄 수정", "批量修改块数据"],
            M::OperationTitle => [" Operation ", " 연산 ", " 运算 "],
            M::NamesTitle => ["Names", "주석 목록", "注释列表"],
            M::StringsTitle => ["Strings", "문자열", "字符串"],
            M::RegexTitle => [" Regex ", " 정규식 ", " 正则 "],
            M::FilterRegexTitle => [" Filter regex ", " 정규식 필터 ", " 正则过滤 "],
            M::AssembleTitle => [" Edit assembly ", " 어셈블리 편집 ", " 编辑汇编 "],
            M::AddSectionTitle => [
                " Add New Section - Size (hex) ",
                " 섹션 추가 - 크기 (헥스) ",
                " 添加节 - 大小 (十六进制) ",
            ],
            M::SelectDriveTitle => [
                " Select Drive (Alt+F1) ",
                " 드라이브 선택 (Alt+F1) ",
                " 选择驱动器 (Alt+F1) ",
            ],
            M::CommentAtTitle => ["Comment at", "주석", "注释"],
            M::XrefTitle => ["Cross References to", "Xref", "交叉引用"],
            M::XrefLimitReached => [", limit reached", ", 한도 도달", "，已达上限"],
            M::StringRefsTitle => ["String References", "문자열 참조", "字符串引用"],
            M::OpenFileTitle => ["Open File", "파일 열기", "打开文件"],
            M::ImageBaseTitle => ["Image Base", "이미지 베이스", "映像基址"],
            M::EditDataTitle => ["Edit Data at", "데이터 편집", "编辑数据"],
            M::FoundCount => ["found", "개 찾음", "个"],

            M::ReadOnlyRefused => [
                "Read Only: cannot {}",
                "읽기 전용: {} 할 수 없습니다",
                "只读：无法{}",
            ],
            M::RoEditData => ["edit data (Ctrl+E)", "데이터 편집 (Ctrl+E)", "编辑数据 (Ctrl+E)"],
            M::RoPaste => ["paste bytes (Shift+V)", "붙여넣기 (Shift+V)", "粘贴字节 (Shift+V)"],
            M::RoCase => ["change case (~)", "대소문자 전환 (~)", "切换大小写 (~)"],
            M::RoEditMode => ["enter edit mode (F2)", "편집 모드 시작 (F2)", "进入编辑模式 (F2)"],
            M::RoFillZero => [
                "fill with 0x00 (Insert)",
                "0x00으로 채우기 (Insert)",
                "填充 0x00 (Insert)",
            ],
            M::RoFillNop => [
                "fill with 0x90 NOPs (Delete)",
                "0x90 NOP으로 채우기 (Delete)",
                "填充 0x90 NOP (Delete)",
            ],
            M::RoModifyBlock => [
                "modify block data (Ctrl+K)",
                "블록 데이터 일괄 수정 (Ctrl+K)",
                "批量修改块数据 (Ctrl+K)",
            ],
            M::RoAssemble => [
                "assemble an instruction (Space)",
                "명령어 어셈블 (Space)",
                "汇编指令 (Space)",
            ],
            M::RoSectionTools => ["use the section tools", "섹션 도구를 사용할", "使用节工具"],
            M::RoStringEdit => [
                "replace a string (e)",
                "문자열을 교체 (e)",
                "替换字符串 (e)",
            ],
            M::RoNopOut => [
                "NOP out the instruction (Delete)",
                "명령어를 NOP으로 채우기 (Delete)",
                "用 NOP 覆盖指令 (Delete)",
            ],

            M::ErrNothingSelected => [
                "Nothing selected - hold Shift and move to select a block",
                "선택된 블록이 없습니다. Shift를 누른 채 이동해 블록을 지정하세요",
                "未选择任何内容 - 按住 Shift 并移动以选择块",
            ],
            M::ErrSaveFailedQuit => [
                "Save failed, not quitting: {}",
                "저장 실패, 종료하지 않습니다: {}",
                "保存失败，未退出：{}",
            ],
            M::ErrSaveError => ["Save error: {}", "저장 오류: {}", "保存错误：{}"],
            M::ErrCommentOutside => [
                "0x{} is outside this file, cannot edit that comment",
                "0x{} 은 이 파일 범위를 벗어나 그 주석을 수정할 수 없습니다",
                "0x{} 超出本文件范围，无法编辑该注释",
            ],
            M::ErrRefusingAssemble => [
                "Refusing to assemble: {}",
                "어셈블 거부: {}",
                "拒绝汇编：{}",
            ],
            M::ErrFailedAssemble => [
                "Failed to assemble: '{}'",
                "어셈블 실패: '{}'",
                "汇编失败：'{}'",
            ],
            M::ErrSwitchValue => [
                "':set {}' takes on, off or toggle, got '{}'",
                "':set {}' 는 on, off, toggle 중 하나여야 합니다. 입력값: '{}'",
                "':set {}' 需要 on、off 或 toggle，收到 '{}'",
            ],
            M::ErrNeedsNumberAuto => [
                "':set byteline' needs a number or 'auto'",
                "':set byteline' 에는 숫자 또는 'auto' 가 필요합니다",
                "':set byteline' 需要一个数字或 'auto'",
            ],
            M::ErrBytelineZero => [
                "':set byteline 0' would leave nothing to show",
                "':set byteline 0' 은 표시할 내용이 없어집니다",
                "':set byteline 0' 会导致无内容可显示",
            ],
            M::ErrNotByteCount => [
                "'{}' is not a byte count or 'auto'",
                "'{}' 은 바이트 수도 'auto' 도 아닙니다",
                "'{}' 不是字节数，也不是 'auto'",
            ],
            M::ErrNeedsCharacter => [
                "':set ctrlchar' needs a character",
                "':set ctrlchar' 에는 문자 하나가 필요합니다",
                "':set ctrlchar' 需要一个字符",
            ],
            M::ErrOneCharacter => [
                "':set ctrlchar' takes one character, got '{}'",
                "':set ctrlchar' 는 문자 하나만 받습니다. 입력값: '{}'",
                "':set ctrlchar' 只接受一个字符，收到 '{}'",
            ],
            M::ErrViewNames => [
                "':set view' takes hex, disasm, text or header, got '{}'",
                "':set view' 는 hex, disasm, text, header 중 하나여야 합니다. 입력값: '{}'",
                "':set view' 需要 hex、disasm、text 或 header，收到 '{}'",
            ],
            M::ErrAddrNames => [
                "':set addr' takes va, offset or toggle, got '{}'",
                "':set addr' 는 va, offset, toggle 중 하나여야 합니다. 입력값: '{}'",
                "':set addr' 需要 va、offset 或 toggle，收到 '{}'",
            ],
            M::ErrNoCodeSection => [
                "this file has no code section to disassemble",
                "이 파일에는 디스어셈블할 코드 섹션이 없습니다",
                "本文件没有可反汇编的代码节",
            ],
            M::ErrLangNeedsValue => [
                "':set lang' takes {}",
                "':set lang' 는 다음 중 하나를 받습니다: {}",
                "':set lang' 需要以下之一：{}",
            ],
            M::ErrUnknownLang => [
                "Unknown language '{}' (available: {})",
                "알 수 없는 언어 '{}' (사용 가능: {})",
                "未知语言 '{}' (可用：{})",
            ],
            M::ErrUnknownOptionSuggest => [
                "Unknown option '{}' - did you mean '{}'? (':set' lists them all)",
                "알 수 없는 옵션 '{}' - '{}' 을 찾으셨나요? (':set' 으로 전체 목록)",
                "未知选项 '{}' - 是否想输入 '{}'？(':set' 列出全部)",
            ],
            M::ErrUnknownOption => [
                "Unknown option '{}' - ':set' with no arguments lists them all",
                "알 수 없는 옵션 '{}' - 인수 없이 ':set' 을 실행하면 전체 목록이 나옵니다",
                "未知选项 '{}' - 不带参数执行 ':set' 可列出全部",
            ],
            M::ErrNeedsColour => [
                "':set {}' needs a colour, e.g. #ff8800 or red",
                "':set {}' 에는 색이 필요합니다. 예: #ff8800 또는 red",
                "':set {}' 需要一个颜色，例如 #ff8800 或 red",
            ],
            M::ErrNotColour => [
                "'{}' is not a colour (try #rrggbb or a name like red)",
                "'{}' 은 색이 아닙니다 (#rrggbb 또는 red 같은 이름)",
                "'{}' 不是颜色 (可用 #rrggbb 或 red 之类的名称)",
            ],
            M::StringEditTitle => [
                " Replace the string at {} ({} bytes, {}) ",
                " {} 의 문자열 교체 ({} 바이트, {}) ",
                " 替换 {} 处的字符串 ({} 字节, {}) ",
            ],
            M::ErrStringTooLong => [
                "Too long: {} bytes needed, {} available - the replacement has to fit where the original sits",
                "너무 깁니다: {} 바이트 필요, {} 바이트만 사용 가능 - 원본이 있던 자리에 들어가야 합니다",
                "太长：需要 {} 字节，只有 {} 字节可用 - 替换内容必须放进原文所在位置",
            ],
            // Only the keys that are not guessable, in the language the rest of the
            // dialog is drawn in. These sit on a border, so the CJK spellings are kept
            // short: every Han character is two columns wide.
            // The other choices in that filter are codepage names, which stay as
            // they are in every language; this one is a word.
            M::LblAllEncodings => ["All", "전체", "全部"],
            M::StringsFooterKeys => [
                " y/Y copy | e replace ",
                " y/Y 복사 | e 교체 ",
                " y/Y 复制 | e 替换 ",
            ],
            M::RefsFooterKeys => [
                " Enter: code | Ctrl+Enter: the string in hex | y/Y: copy ",
                " Enter: 코드 | Ctrl+Enter: 헥스의 문자열 | y/Y: 복사 ",
                " Enter: 代码 | Ctrl+Enter: 十六进制中的字符串 | y/Y: 复制 ",
            ],
            M::XrefFooterKeys => [
                " Enter: jump | y/Y: copy ",
                " Enter: 이동 | y/Y: 복사 ",
                " Enter: 跳转 | y/Y: 复制 ",
            ],
            M::StringReplaced => [
                "Replaced the string at {} with {} byte(s), {} padded with 00",
                "{} 의 문자열을 {} 바이트로 교체, 남은 {} 바이트는 00 으로 채움",
                "已替换 {} 处的字符串为 {} 字节，剩余 {} 字节以 00 填充",
            ],
            M::WarnRegexEmptyOnly => [
                "This pattern only ever matched an empty string, so nothing passes - use + or {2,} instead of *",
                "이 패턴은 빈 문자열만 매칭했습니다. 그래서 결과가 없습니다 - * 대신 + 또는 {2,} 를 쓰세요",
                "此模式只匹配到空字符串，因此没有结果 - 请用 + 或 {2,} 代替 *",
            ],
            M::ErrUnknownEncoding => [
                "Unknown encoding '{}' (try {})",
                "알 수 없는 인코딩 '{}' (사용 가능: {})",
                "未知编码 '{}' (可用：{})",
            ],

            M::LblType => ["Type", "종류", "类型"],

            M::LblAddress => ["Address", "주소", "地址"],
            M::LblInstruction => ["Instruction", "명령어", "指令"],
            M::LblDisassembly => ["Disassembly", "디스어셈블", "反汇编"],
            M::LblTextString => ["Text string", "문자열", "字符串"],
            M::LblValue => ["Val", "값", "值"],
            M::LblStep => ["Step", "증분", "步进"],
            M::LblSearch => ["Search", "찾기", "查找"],
            M::LblReplace => ["Replace", "바꾸기", "替换"],
            M::LblSubDir => ["SUB-DIR", "디렉터리", "目录"],
            M::LblError => ["Error", "오류", "错误"],
            M::ReplacePatternTitle => ["Pattern Replace", "패턴 바꾸기", "模式替换"],
            M::FindPatternTitle => ["Find Pattern", "패턴 찾기", "查找模式"],
            M::FindHint => [
                "Tab field | Enter/F3 next | Shift+F3 prev | Esc close",
                "Tab 칸 이동 | Enter/F3 다음 | Shift+F3 이전 | Esc 닫기",
                "Tab 切换 | Enter/F3 下一个 | Shift+F3 上一个 | Esc 关闭",
            ],
            M::BytesSelected => [
                "Status: {} byte(s) selected",
                "상태: {} 바이트 선택됨",
                "状态：已选择 {} 个字节",
            ],
            M::ReplaceHint => [
                "Enter/F3 next | Shift+F3 prev | Alt+R replace | Alt+A all",
                "Enter/F3 다음 | Shift+F3 이전 | Alt+R 바꾸기 | Alt+A 모두",
                "Enter/F3 下一个 | Shift+F3 上一个 | Alt+R 替换 | Alt+A 全部",
            ],
            // Where the current hit is, and which of how many it is. The shape is
            // the same in every language: it is arithmetic plus an address, and
            // `offset` / `VA` are the field names the rest of the program uses.
            M::MatchAtOffset => [
                "Match ({}/{}) offset : 0x{}",
                "일치 ({}/{}) offset : 0x{}",
                "匹配 ({}/{}) offset : 0x{}",
            ],
            M::MatchAtVa => [
                "Match ({}/{}) VA : 0x{}",
                "일치 ({}/{}) VA : 0x{}",
                "匹配 ({}/{}) VA : 0x{}",
            ],
            M::ReplacedAt => [
                "Replaced the pattern at 0x{}",
                "0x{} 의 패턴을 바꿨습니다",
                "已替换 0x{} 处的模式",
            ],
            M::ReplacedCount => [
                "Replaced {} occurrence(s)",
                "{} 개를 바꿨습니다",
                "已替换 {} 处",
            ],
            M::NotAtAMatch => [
                "The cursor is not on a match - press Enter to find one first",
                "커서가 일치 지점에 없습니다. Enter로 먼저 찾으세요",
                "光标不在匹配处 - 请先按 Enter 查找",
            ],
            M::FindEnterHex => [
                "Enter a hex pattern to search for.",
                "찾을 헥스 패턴을 입력하세요.",
                "请输入要查找的十六进制模式。",
            ],
            M::FindInvalidHex => [
                "Invalid hex pattern.",
                "잘못된 헥스 패턴입니다.",
                "十六进制模式无效。",
            ],
            M::FindEnterText => [
                "Enter text to search for.",
                "찾을 텍스트를 입력하세요.",
                "请输入要查找的文本。",
            ],
            M::FindNoMatch => [
                "No matching pattern found.",
                "일치하는 패턴이 없습니다.",
                "未找到匹配的模式。",
            ],
            M::SizeHexHint => [
                "Enter a size in hex, e.g. 1000 or 0x1000",
                "크기를 헥스로 입력하세요. 예: 1000 또는 0x1000",
                "请以十六进制输入大小，例如 1000 或 0x1000",
            ],
            // Enter on a text row (the DOS stub, the PE signature) rather than a
            // numeric field.
            M::ErrFieldNotNumeric => [
                "'{}' is text, not a number to edit",
                "'{}' 은(는) 숫자 필드가 아니라 문자열입니다",
                "'{}' 是文本，不是可编辑的数值",
            ],
            M::ErrSectionSizeZero => [
                "Section size must be greater than 0",
                "섹션 크기는 0보다 커야 합니다",
                "节大小必须大于 0",
            ],
            M::ErrNoPeHeaders => [
                "No PE headers loaded",
                "PE 헤더가 로드되지 않았습니다",
                "未加载 PE 头",
            ],
            M::ErrNoOptionalHeader => [
                "PE has no Optional Header",
                "이 PE에는 Optional Header가 없습니다",
                "该 PE 没有 Optional Header",
            ],
            M::ErrSectionTooBig => [
                "Resulting section exceeds 32-bit PE limits",
                "결과 섹션이 32비트 PE 한계를 넘습니다",
                "生成的节超出 32 位 PE 限制",
            ],

            M::OpAdd => ["Add (+)", "더하기 (+)", "加 (+)"],
            M::OpSub => ["Subtract (-)", "빼기 (-)", "减 (-)"],
            M::OpMul => ["Multiply (*)", "곱하기 (*)", "乘 (*)"],
            M::OpDiv => ["Divide (/)", "나누기 (/)", "除 (/)"],
            M::OpXor => ["XOR (^)", "XOR (^)", "XOR (^)"],
            M::OpOr => ["OR (|)", "OR (|)", "OR (|)"],
            M::OpAnd => ["AND (&)", "AND (&)", "AND (&)"],
            M::OpNot => ["Invert/NOT (~)", "반전/NOT (~)", "取反/NOT (~)"],
            M::OpEndianSwap => ["Endian Swap", "엔디안 스왑", "字节序交换"],
            M::OpShiftLeft => ["Shift Left (<<)", "왼쪽 시프트 (<<)", "左移 (<<)"],
            M::OpShiftRight => ["Shift Right (>>)", "오른쪽 시프트 (>>)", "右移 (>>)"],
            M::OpRandom => ["Random Fill (rand)", "랜덤 채우기 (rand)", "随机填充 (rand)"],
            M::OpRollingXor => [
                "Rolling XOR (key+step)",
                "롤링 XOR (키+증분)",
                "滚动 XOR (密钥+步进)",
            ],

            M::NoteByteline => [
                "bytes per line, or auto",
                "한 줄에 표시할 바이트 수, 또는 auto",
                "每行字节数，或 auto",
            ],
            M::NoteCtrlchar => [
                "stand-in for a non-graphic byte",
                "표시 불가 바이트를 대신할 문자",
                "非可见字节的替代字符",
            ],
            M::NoteEnc1 => ["primary text encoding", "주 텍스트 인코딩", "主文本编码"],
            M::NoteEnc2 => ["secondary text column", "보조 텍스트 칼럼", "次文本列"],
            M::NoteAddr => [
                "address column: va or offset",
                "주소 칼럼: va 또는 offset",
                "地址列：va 或 offset",
            ],
            M::NoteBitness => [
                "disassembly decoding width",
                "디스어셈블 디코딩 비트 수",
                "反汇编解码位宽",
            ],
            M::NoteView => [
                "hex, disasm, text or header",
                "hex, disasm, text, header",
                "hex、disasm、text 或 header",
            ],
            M::NoteTheme => ["hex-view colours", "헥스 뷰 색상", "十六进制视图配色"],
            M::NoteDb => [
                "write the .dzdb sidecar file",
                ".dzdb 주석 파일 저장",
                "写入 .dzdb 附属文件",
            ],
            M::NoteDimctrl => ["dim control bytes", "제어 바이트 흐리게", "淡化控制字节"],
            M::NoteDimzero => ["dim null bytes", "널 바이트 흐리게", "淡化空字节"],
            M::NoteWrapscan => [
                "searches wrap around EOF",
                "파일 끝에서 검색 순환",
                "搜索在文件末尾回绕",
            ],
            M::NoteHighlight => [
                "disassembly syntax colours",
                "디스어셈블 구문 색상",
                "反汇编语法配色",
            ],
            M::NoteHintbar => ["bottom hint line", "하단 힌트 줄", "底部提示栏"],
            M::NoteLang => ["interface language", "인터페이스 언어", "界面语言"],
            M::NoteDisasmColor => ["disassembly colour", "디스어셈블 색상", "反汇编颜色"],
        }
    }
}

#[cfg(test)]
mod i18n_tests {
    use super::*;

    /// Every message must be filled in for every language.
    ///
    /// An empty slot would render as a blank hint slot or an untitled dialog, which
    /// is harder to spot than a missing translation in a list.
    #[test]
    fn no_message_is_empty() {
        for m in M::ALL {
            for lang in Lang::ALL {
                assert!(
                    !m.tr(lang).trim().is_empty(),
                    "{:?} has no {} text",
                    m,
                    lang.name()
                );
            }
        }
    }

    /// Hint-bar words have to fit the row. CJK is double-width, so the limit is in
    /// display columns, not characters.
    #[test]
    fn hint_labels_stay_short() {
        use unicode_width::UnicodeWidthStr;

        // `M::SaveQuit` is deliberately absent: it is the long wording for the last
        // slot on the line, which falls back to `M::Save` when it does not fit, and
        // `hint_bar`'s width tests check what actually matters.
        let labels = [
            M::Help, M::Edit, M::HeaderView, M::Refs, M::Strings, M::TextView, M::About,
            M::Open, M::Save, M::HexView, M::Type, M::Column, M::Case, M::Select, M::Done,
            M::Copy, M::Modify, M::Color, M::Clear, M::Data, M::Goto, M::Find, M::Replace,
            M::Xref, M::Addr, M::Undo, M::Redo, M::Encoding, M::Encoding2, M::Highlight,
            M::Log, M::Names, M::RevertByte, M::ImageBase, M::DecodeWidth,
        ];
        for m in labels {
            for lang in Lang::ALL {
                let width = UnicodeWidthStr::width(m.tr(lang));
                assert!(
                    width <= 10,
                    "{:?} in {} is {} columns wide, too wide for a hint slot",
                    m,
                    lang.name(),
                    width
                );
            }
        }
    }

    /// The names `:set lang` accepts round-trip.
    #[test]
    fn language_names_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_name(lang.name()), Some(lang));
        }
        assert_eq!(Lang::from_name("Korean"), Some(Lang::Ko));
        assert_eq!(Lang::from_name(" CN "), Some(Lang::Zh));
        assert_eq!(Lang::from_name("english"), Some(Lang::En));
        assert_eq!(Lang::from_name("klingon"), None);
    }

}

#[cfg(test)]
mod fill_tests {
    use super::*;

    /// Placeholders are filled in order.
    #[test]
    fn placeholders_are_filled_in_order() {
        assert_eq!(fill("a {} b {} c", &["1", "2"]), "a 1 b 2 c");
        assert_eq!(fill("{}", &["x"]), "x");
        assert_eq!(fill("no placeholder", &["x"]), "no placeholder");
    }

    /// A template with more placeholders than arguments keeps the rest verbatim
    /// rather than panicking - a translation with an extra `{}` must not take the
    /// program down.
    #[test]
    fn a_short_argument_list_is_survivable() {
        assert_eq!(fill("{} and {}", &["one"]), "one and {}");
        assert_eq!(fill("{}", &[]), "{}");
    }

    /// Every message that takes arguments has the same number of placeholders in
    /// all three languages. A translation that drops one silently loses the value.
    #[test]
    fn placeholder_counts_match_across_languages() {
        for m in M::ALL {
            let counts: Vec<usize> = Lang::ALL
                .iter()
                .map(|lang| m.tr(*lang).matches("{}").count())
                .collect();
            assert!(
                counts.iter().all(|c| *c == counts[0]),
                "{:?} has {:?} placeholders across en/ko/zh",
                m,
                counts
            );
        }
    }
}

#[cfg(test)]
mod dialog_chrome_tests {
    //! The borders of the result dialogs have to follow `:set lang` like everything
    //! else, and they have to fit.
    //!
    //! The key hints added to the strings, references and cross-reference boxes were
    //! written straight into the drawing code in English, so changing the language
    //! left them behind. CJK is the reason the width matters: every Han character is
    //! two columns, so a translation that reads fine in English can overflow the
    //! border it sits on and come out truncated - as `Minimum length` did, drawn as
    //! `imum length`.

    use crate::i18n::{Lang, M};
    use unicode_width::UnicodeWidthStr;

    /// Every footer is translated, i.e. no two languages share the English text.
    #[test]
    fn the_footers_are_translated() {
        for message in [M::StringsFooterKeys, M::RefsFooterKeys, M::XrefFooterKeys, M::LblAllEncodings] {
            let en = message.tr(Lang::En);
            for lang in [Lang::Ko, Lang::Zh] {
                let translated = message.tr(lang);
                assert!(!translated.trim().is_empty(), "{:?} is empty in {:?}", message, lang);
                assert_ne!(
                    translated, en,
                    "{:?} is still the English text in {:?}",
                    message, lang
                );
            }
        }
    }

    /// The keys themselves survive translation: a hint that drops the letter it is
    /// about is worse than no hint.
    #[test]
    fn the_footers_still_name_their_keys() {
        for lang in Lang::ALL {
            let strings = M::StringsFooterKeys.tr(lang);
            assert!(strings.contains("y/Y"), "{:?}: {:?}", lang, strings);
            assert!(strings.contains(" e "), "{:?}: {:?}", lang, strings);

            let refs = M::RefsFooterKeys.tr(lang);
            assert!(refs.contains("Enter"), "{:?}: {:?}", lang, refs);
            assert!(refs.contains("Ctrl+Enter"), "{:?}: {:?}", lang, refs);
            assert!(refs.contains("y/Y"), "{:?}: {:?}", lang, refs);

            let xref = M::XrefFooterKeys.tr(lang);
            assert!(xref.contains("Enter"), "{:?}: {:?}", lang, xref);
            assert!(xref.contains("y/Y"), "{:?}: {:?}", lang, xref);
        }
    }

    /// Each footer fits the border it is drawn on, measured in terminal columns.
    ///
    /// The widths are the ones the dialogs use: the references box is a fixed 96, the
    /// cross-reference box a fixed 82, and the strings box is half the terminal - so
    /// it is checked against a 68-column terminal, the narrowest the program claims
    /// to support.
    #[test]
    fn the_footers_fit_their_boxes() {
        // Strings dialog: half of 68 is 34, less two border columns, and the filter
        // box it sits on is another two in.
        let strings_inner = 34 - 2 - 2;
        let refs_inner = 96 - 2;
        let xref_inner = 82 - 2;

        for lang in Lang::ALL {
            let cases = [
                (M::StringsFooterKeys, strings_inner),
                (M::RefsFooterKeys, refs_inner),
                (M::XrefFooterKeys, xref_inner),
            ];
            for (message, room) in cases {
                let text = message.tr(lang);
                assert!(
                    text.width() <= room,
                    "{:?} in {:?} is {} columns, {} available: {:?}",
                    message,
                    lang,
                    text.width(),
                    room,
                    text
                );
            }
        }
    }

    /// The strings dialog draws its own border text in the chosen language.
    #[test]
    fn the_strings_dialog_draws_the_translated_footer() {
        use ratatui::{Terminal, backend::TestBackend};

        let dir = std::env::temp_dir().join(format!("dz6_chrome_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("sample.bin");
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x15].copy_from_slice(b"HELLO");
        std::fs::write(&path, &bytes).expect("write");

        let mut app = crate::app::App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true).expect("open");
        crate::commands::Commands::strings(&mut app);

        for lang in [Lang::Ko, Lang::Zh, Lang::En] {
            app.config.lang = lang;
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
            terminal
                .draw(|f| crate::hex::strings::dialog_strings_draw(&mut app, f))
                .expect("draw");
            let buffer = terminal.backend().buffer().clone();
            let screen: String = (0..30)
                .map(|y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect();

            // The first word of the footer in that language, whatever it is.
            let word = M::StringsFooterKeys
                .tr(lang)
                .split_whitespace()
                .last()
                .expect("a word");
            // Whitespace is dropped from both sides before comparing: a double-width
            // character occupies two cells, and the backend fills the second with a
            // space - so reading the buffer cell by cell turns `교체` into `교 체`.
            let flat: String = screen.chars().filter(|c| !c.is_whitespace()).collect();
            let wanted: String = word.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(
                flat.contains(&wanted),
                "{:?}: the footer word {:?} is not on screen",
                lang,
                word
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}