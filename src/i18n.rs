// Interface language: English, Korean and Chinese.
//
// Only the words are translated - key names (`F1`, `Ctrl+C`), option names
// (`byteline`), status-bar mode labels (`HEX`, `DISASM`) and everything a user
// types stay as they are. Those are identifiers, and translating them would mean
// the documentation, the `:set` command and the screen no longer agree.

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
    WarnRegexEmptyOnly,
    StringEditTitle,
    ErrStringTooLong,
    ErrAsciiOnly,
    StringReplaced,
    LblAllEncodings,
    StringsFooterKeys,
    StringEditFooterKeys,
    RefsFooterKeys,
    XrefFooterKeys,

    // Newly added localized error and label messages
    ErrAddressOutOfBounds,
    ErrInvalidAddressExpr,
    ErrInvalidNumericValue,
    ErrNothingToCopy,
    ErrClipboardAccess,
    ErrAltMSelectionNeeded,
    ErrNoSectionPicked,
    ErrNoRoomForSectionHeader,
    LblSectionNameMax8,
    LblFileBase,
    ErrNothingToAssemble,
    ErrFileReadOnly,
    ErrAssemblePastEof,
    ErrNotAnAddress,

    // Dialog contents
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
        M::StringEditTitle, M::ErrStringTooLong, M::ErrAsciiOnly, M::StringReplaced,
        M::StringsFooterKeys, M::StringEditFooterKeys, M::RefsFooterKeys, M::XrefFooterKeys,
        M::LblAllEncodings,
        M::ErrAddressOutOfBounds, M::ErrInvalidAddressExpr, M::ErrInvalidNumericValue,
        M::ErrNothingToCopy, M::ErrClipboardAccess, M::ErrAltMSelectionNeeded,
        M::ErrNoSectionPicked, M::ErrNoRoomForSectionHeader, M::LblSectionNameMax8,
        M::LblFileBase, M::ErrNothingToAssemble, M::ErrFileReadOnly, M::ErrAssemblePastEof,
        M::ErrNotAnAddress,
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

    pub fn tr(self, lang: Lang) -> &'static str {
        let [en, ko, zh] = self.table();
        match lang {
            Lang::En => en,
            Lang::Ko => ko,
            Lang::Zh => zh,
        }
    }

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
                " Ctrl+C copy to clipboard, Esc close ",
                " Ctrl+C 클립보드 복사, Esc 닫기 ",
                " Ctrl+C 复制到剪贴板，Esc 关闭 ",
            ],
            M::LogTitle => [" Log ", " 로그 ", " 日志 "],
            M::LogFooter => [
                " Ctrl+C copy, Delete clear, Esc close ",
                " Ctrl+C 복사, Delete 지우기, Esc 닫기 ",
                " Ctrl+C 复制，Delete 清空，Esc 关闭 ",
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
            M::ErrAsciiOnly => [
                "ASCII mode only allows ASCII characters. Use F2 to switch to UTF-8 or CP949 for non-ASCII text",
                "ASCII 모드에서는 영문/숫자/기호만 저장할 수 있습니다. 한글/다국어 수정을 원하시면 F2를 눌러 UTF-8 또는 CP949 스캔 모드로 변경하세요",
                "ASCII 模式仅支持 ASCII 字符。非 ASCII 文本请按 F2 切换至 UTF-8 或 CP949 模式",
            ],
            M::LblAllEncodings => ["All", "전체", "全部"],
            M::StringsFooterKeys => [
                " Ctrl+C: copy line | Ctrl+Shift+C: copy all ",
                " Ctrl+C: 한줄복사 | Ctrl+Shift+C: 전체복사 ",
                " Ctrl+C: 复制单行 | Ctrl+Shift+C: 复制全部 ",
            ],
            M::StringEditFooterKeys => [
                " Enter save | Esc cancel | Alt+E enc ",
                " Enter 저장 | Esc 취소 | Alt+E 인코딩 ",
                " Enter 保存 | Esc 取消 | Alt+E 编码 ",
            ],
            M::RefsFooterKeys => [
                " Enter: code | Ctrl+Enter: hex | Ctrl+C: copy line | Ctrl+Shift+C: copy all ",
                " Enter: 코드 | Ctrl+Enter: 헥스 | Ctrl+C: 한줄복사 | Ctrl+Shift+C: 전체복사 ",
                " Enter: 代码 | Ctrl+Enter: 十六进制 | Ctrl+C: 复制单行 | Ctrl+Shift+C: 复制全部 ",
            ],
            M::XrefFooterKeys => [
                " Enter: jump | Ctrl+C: copy line | Ctrl+Shift+C: copy all ",
                " Enter: 이동 | Ctrl+C: 한줄복사 | Ctrl+Shift+C: 전체복사 ",
                " Enter: 跳转 | Ctrl+C: 复制单行 | Ctrl+Shift+C: 复制全部 ",
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

            // Newly added localized messages
            M::ErrAddressOutOfBounds => [
                "Address 0x{} out of bounds",
                "주소 0x{} 은 범위를 벗어났습니다",
                "地址 0x{} 超出范围",
            ],
            M::ErrInvalidAddressExpr => [
                "Invalid address expression: '{}'",
                "잘못된 주소 수식: '{}'",
                "无效的地址表达式：'{}'",
            ],
            M::ErrInvalidNumericValue => [
                "Invalid numeric value: '{}'",
                "잘못된 숫자 값: '{}'",
                "无效的数值：'{}'",
            ],
            M::ErrNothingToCopy => [
                "Nothing to copy",
                "복사할 내용이 없습니다",
                "没有可复制的内容",
            ],
            M::ErrClipboardAccess => [
                "Could not access the clipboard",
                "클립보드에 접근할 수 없습니다",
                "无法访问剪贴板",
            ],
            M::ErrAltMSelectionNeeded => [
                "Alt+M needs a selection (Shift+arrows) or a block at the cursor",
                "Alt+M 은 블록 선택(Shift+방향키) 또는 커서 위치의 블록이 필요합니다",
                "Alt+M 需要选择块 (Shift+方向键) 或在光标处有块",
            ],
            M::ErrNoSectionPicked => [
                "No section at index {} - open the Section tab and pick one first",
                "인덱스 {} 에 섹션이 없습니다 - 먼저 섹션 탭에서 선택하세요",
                "索引 {} 处没有节 - 请先在节标签页中选择",
            ],
            M::ErrNoRoomForSectionHeader => [
                "No room for another section header (SizeOfHeaders = 0x{:X} leaves no padding after the last entry)",
                "새 섹션 헤더를 추가할 공간이 없습니다 (SizeOfHeaders = 0x{:X} 패딩 부족)",
                "没有空间添加新节头 (SizeOfHeaders = 0x{:X} 缺少填充)",
            ],
            M::LblSectionNameMax8 => [
                " Section Name (max 8 chars): {} ",
                " 섹션 이름 (최대 8자): {} ",
                " 节名称 (最多 8 字符)：{} ",
            ],
            M::LblFileBase => [
                " (file: {:X}) ",
                " (파일: {:X}) ",
                " (文件: {:X}) ",
            ],
            M::ErrNothingToAssemble => [
                "nothing to assemble",
                "어셈블할 내용이 없습니다",
                "没有可汇编的内容",
            ],
            M::ErrFileReadOnly => [
                "file is read-only",
                "파일이 읽기 전용입니다",
                "文件为只读",
            ],
            M::ErrAssemblePastEof => [
                "{} byte(s) at 0x{:X} would run past the end of the file (0x{:X})",
                "0x{:X} 의 {} 바이트가 파일 끝(0x{:X})을 벗어납니다",
                "0x{:X} 处的 {} 字节超出文件末尾 (0x{:X})",
            ],
            M::ErrNotAnAddress => [
                "'{}' is not an address",
                "'{}' 은(는) 주소가 아닙니다",
                "'{}' 不是有效的地址",
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
                "Enter/F3 下一个 | Shift+F3 상一个 | Alt+R 替换 | Alt+A 全部",
            ],
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
