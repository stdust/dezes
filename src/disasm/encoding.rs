#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisasmCommentEncoding {
    #[default]
    Auto,
    Utf16Le,
    Utf8,
    Cp949,
    Cp936,
    Ascii,
}

impl DisasmCommentEncoding {
    pub const ALL: [DisasmCommentEncoding; 6] = [
        DisasmCommentEncoding::Auto,
        DisasmCommentEncoding::Utf16Le,
        DisasmCommentEncoding::Utf8,
        DisasmCommentEncoding::Cp949,
        DisasmCommentEncoding::Cp936,
        DisasmCommentEncoding::Ascii,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf8 => "UTF-8",
            Self::Cp949 => "CP949",
            Self::Cp936 => "CP936",
            Self::Ascii => "ASCII",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Auto => "Auto (Smart Detect)",
            Self::Utf16Le => "UTF-16LE (Unicode)",
            Self::Utf8 => "UTF-8",
            Self::Cp949 => "CP949 (Korean EUC-KR)",
            Self::Cp936 => "CP936 (Chinese GBK)",
            Self::Ascii => "ASCII",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Auto => Self::Utf16Le,
            Self::Utf16Le => Self::Utf8,
            Self::Utf8 => Self::Cp949,
            Self::Cp949 => Self::Cp936,
            Self::Cp936 => Self::Ascii,
            Self::Ascii => Self::Auto,
        }
    }

    #[allow(dead_code)]
    pub fn prev(&self) -> Self {
        match self {
            Self::Auto => Self::Ascii,
            Self::Utf16Le => Self::Auto,
            Self::Utf8 => Self::Utf16Le,
            Self::Cp949 => Self::Utf8,
            Self::Cp936 => Self::Cp949,
            Self::Ascii => Self::Cp936,
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Auto)
    }

    pub fn to_index(&self) -> usize {
        Self::ALL.iter().position(|e| e == self).unwrap_or(0)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let v = name.trim();
        let matches = |candidates: &[&str]| candidates.iter().any(|c| v.eq_ignore_ascii_case(c));

        if matches(&["auto", "default", "smart", "all"]) {
            Some(Self::Auto)
        } else if matches(&["utf-16le", "utf-16-le", "utf16le", "utf16", "unicode", "u16"]) {
            Some(Self::Utf16Le)
        } else if matches(&["utf-8", "utf8", "u8"]) {
            Some(Self::Utf8)
        } else if matches(&["cp949", "euc-kr", "korean", "ko"]) {
            Some(Self::Cp949)
        } else if matches(&["cp936", "gbk", "chinese", "zh", "cn"]) {
            Some(Self::Cp936)
        } else if matches(&["ascii", "ansi"]) {
            Some(Self::Ascii)
        } else {
            None
        }
    }
}
