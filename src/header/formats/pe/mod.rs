pub mod draw;
pub mod events;
pub mod fields;
pub mod section_tools;

use crate::header::header_view::Pe;

/// Byte offsets and sizes of Optional Header fields.
///
/// PE32 and PE32+ do **not** share one layout, and the difference is not just
/// the header size:
///
///   - PE32 has `BaseOfData` at +24, then a 4-byte `ImageBase` at +28.
///   - PE32+ drops `BaseOfData` and puts an 8-byte `ImageBase` at +24.
///
/// Both then realign at +32 (`SectionAlignment`) and stay in step until
/// `SizeOfStackReserve`, where PE32+ switches to 8-byte size/heap fields and
/// pushes everything after it - including the Data Directory array - 24 bytes
/// further out.
///
/// The header code used to hardcode the PE32 numbers everywhere, so on any
/// 64-bit binary `ImageBase` was read as the low 4 bytes of the wrong field
/// (0x140000000 showed up as 0x1), and the stack/heap fields and Data
/// Directory offsets were all off. This type is the single place that layout
/// is decided.
#[derive(Debug, Clone, Copy)]
pub struct OptionalHeaderLayout {
    /// Offset of the Optional Header from the start of the file.
    pub base: usize,
    /// True for PE32+ (magic 0x20B), i.e. a 64-bit image.
    pub is_pe32_plus: bool,
}

impl OptionalHeaderLayout {
    pub const PE32_PLUS_MAGIC: u16 = 0x20B;

    pub fn from_pe(pe: &Pe) -> Self {
        let base = pe.dos_header.pe_pointer as usize + 24;
        let is_pe32_plus = pe
            .optional_header
            .as_ref()
            .map(|opt| opt.standard_fields.magic == Self::PE32_PLUS_MAGIC)
            .unwrap_or(false);
        Self { base, is_pe32_plus }
    }

    /// `(offset, size)` of `ImageBase`.
    pub fn image_base(&self) -> (usize, usize) {
        if self.is_pe32_plus {
            (self.base + 24, 8)
        } else {
            (self.base + 28, 4)
        }
    }

    /// Size of each of the four stack/heap reserve/commit fields.
    pub fn size_field_width(&self) -> usize {
        if self.is_pe32_plus { 8 } else { 4 }
    }

    pub fn size_of_stack_reserve(&self) -> (usize, usize) {
        (self.base + 72, self.size_field_width())
    }

    pub fn size_of_stack_commit(&self) -> (usize, usize) {
        let w = self.size_field_width();
        (self.base + 72 + w, w)
    }

    pub fn size_of_heap_reserve(&self) -> (usize, usize) {
        let w = self.size_field_width();
        (self.base + 72 + w * 2, w)
    }

    pub fn size_of_heap_commit(&self) -> (usize, usize) {
        let w = self.size_field_width();
        (self.base + 72 + w * 3, w)
    }

    /// Offset of `LoaderFlags`, which follows the last heap field.
    pub fn loader_flags(&self) -> usize {
        self.base + 72 + self.size_field_width() * 4
    }

    /// Offset of the first `IMAGE_DATA_DIRECTORY` entry: right after
    /// `NumberOfRvaAndSizes`.
    pub fn data_directories(&self) -> usize {
        self.loader_flags() + 8
    }

    /// Offset of the `idx`-th Data Directory entry's RVA field. Each entry is
    /// 8 bytes: 4-byte RVA followed by 4-byte size.
    pub fn data_directory(&self, idx: usize) -> usize {
        self.data_directories() + idx * 8
    }
}
