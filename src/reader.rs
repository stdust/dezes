/// Reader is the class that keep tracks on current page size
/// and its location in the memory mapped file.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reader {
    pub page_current_size: usize,
    pub page_start: usize,
    pub page_end: usize,
}

impl Reader {
    pub fn new() -> Self {
        Reader {
            // We initialize this to a non-zero value to avoid division by zero
            page_current_size: 1,
            page_start: 0,
            page_end: 1,
        }
    }
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}
