//! Hex pattern search & replace engine with wildcard byte support (`??`).
//!
//! Design goals:
//! - **Fast**: uses `memchr` (SIMD-accelerated) to jump directly to
//!   candidate positions on the *rarest* literal byte in the pattern,
//!   instead of scanning every byte with a naive O(n·m) loop. Beyond that,
//!   the actual per-candidate verification/replace also avoids a per-byte
//!   branch: contiguous literal bytes are precomputed into runs and
//!   compared/copied with slice ops (`==` / `copy_from_slice`), which LLVM
//!   lowers to `memcmp`/`memcpy` and can auto-vectorize — much cheaper than
//!   matching on `PatternByte` once per byte, especially for patterns that
//!   are mostly literal with only occasional `??` gaps.
//! - **Stable**: replace is always byte-for-byte — the search pattern and
//!   replace pattern must be the same length — so no bytes are ever
//!   inserted or removed. File size and every downstream offset stay
//!   exactly where they were.
//! - **Wildcard-aware**: `??` in the *search* pattern matches any byte.
//!   `??` in the *replace* pattern means "leave this byte untouched".

use memchr::memchr_iter;
use std::fmt;
use std::ops::Range;

/// One "slot" in a hex pattern: either a fixed byte value or a wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternByte {
    Literal(u8),
    Wildcard,
}

/// A parsed hex pattern, e.g. "DE AD ?? EF" -> [DE, AD, *, EF]
#[derive(Debug, Clone)]
pub struct HexPattern {
    bytes: Vec<PatternByte>,
    /// Flat byte view of `bytes`: the literal value where `bytes[i]` is
    /// `Literal`, or `0` (unused placeholder) where it's `Wildcard`.
    /// Only ever read through `literal_runs`, so the placeholder value
    /// never affects correctness.
    flat: Vec<u8>,
    /// Maximal contiguous ranges of `Literal` slots in `bytes`/`flat`.
    /// Precomputed once at parse time so matching/replacing can operate
    /// on whole runs via slice ops instead of a per-byte enum branch.
    literal_runs: Vec<Range<usize>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatternError {
    EmptyPattern,
    InvalidToken(String),
    LengthMismatch { search_len: usize, replace_len: usize },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternError::EmptyPattern => write!(f, "pattern is empty"),
            PatternError::InvalidToken(t) => write!(f, "invalid hex token: '{t}'"),
            PatternError::LengthMismatch { search_len, replace_len } => write!(
                f,
                "search pattern ({search_len} bytes) and replace pattern ({replace_len} bytes) \
                 must be the same length for a stable in-place replace"
            ),
        }
    }
}

impl std::error::Error for PatternError {}

impl HexPattern {
    /// Parse a pattern like `"DE AD ?? EF"` or the compact form
    /// `"DEAD??EF"` (spaces are optional, but if you omit them the whole
    /// string must be an even number of characters).
    pub fn parse(input: &str) -> Result<Self, PatternError> {
        let tokens: Vec<String> = if input.contains(char::is_whitespace) {
            input.split_whitespace().map(|s| s.to_string()).collect()
        } else {
            let chars: Vec<char> = input.chars().collect();
            if chars.is_empty() || chars.len() % 2 != 0 {
                return Err(PatternError::InvalidToken(input.to_string()));
            }
            chars.chunks(2).map(|c| c.iter().collect()).collect()
        };

        if tokens.is_empty() {
            return Err(PatternError::EmptyPattern);
        }

        let mut bytes = Vec::with_capacity(tokens.len());
        for tok in tokens {
            if tok == "??" {
                bytes.push(PatternByte::Wildcard);
            } else {
                match u8::from_str_radix(&tok, 16) {
                    Ok(b) => bytes.push(PatternByte::Literal(b)),
                    Err(_) => return Err(PatternError::InvalidToken(tok)),
                }
            }
        }

        // Single pass over `bytes`: build the flat byte view and collect
        // maximal contiguous literal runs at the same time.
        let mut flat = Vec::with_capacity(bytes.len());
        let mut literal_runs = Vec::new();
        let mut run_start: Option<usize> = None;
        for (i, pb) in bytes.iter().enumerate() {
            match pb {
                PatternByte::Literal(v) => {
                    flat.push(*v);
                    run_start.get_or_insert(i);
                }
                PatternByte::Wildcard => {
                    flat.push(0);
                    if let Some(s) = run_start.take() {
                        literal_runs.push(s..i);
                    }
                }
            }
        }
        if let Some(s) = run_start {
            literal_runs.push(s..bytes.len());
        }

        Ok(HexPattern { bytes, flat, literal_runs })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_slice(&self) -> &[PatternByte] {
        &self.bytes
    }
}

/// Finds and replaces occurrences of a wildcard hex pattern in a byte slice.
pub struct HexReplacer {
    search: HexPattern,
    replace: HexPattern,
    /// (index into search.bytes, literal byte value) used to drive memchr.
    /// `None` only when the search pattern is *all* wildcards (degenerate
    /// case — every window matches trivially).
    anchor: Option<(usize, u8)>,
}

impl HexReplacer {
    /// Build a replacer. `haystack_hint` is a sample of the data (or the
    /// whole file/buffer) used to pick the *rarest* literal byte in the
    /// pattern as the memchr anchor. This is what keeps searches fast even
    /// on repetitive data (e.g. long runs of 0x00) — you avoid anchoring
    /// on a byte that appears everywhere.
    pub fn new(
        search: HexPattern,
        replace: HexPattern,
        haystack_hint: &[u8],
    ) -> Result<Self, PatternError> {
        if search.is_empty() {
            return Err(PatternError::EmptyPattern);
        }
        if search.len() != replace.len() {
            return Err(PatternError::LengthMismatch {
                search_len: search.len(),
                replace_len: replace.len(),
            });
        }

        let anchor = pick_anchor(&search, haystack_hint);
        Ok(Self { search, replace, anchor })
    }

    pub fn pattern_len(&self) -> usize {
        self.search.len()
    }

    /// Find all non-overlapping matches, left to right. Returns start offsets.
    pub fn find_all(&self, haystack: &[u8]) -> Vec<usize> {
        let len = self.search.len();
        if haystack.len() < len {
            return Vec::new();
        }

        match self.anchor {
            Some((anchor_idx, anchor_byte)) => {
                let mut out = Vec::new();
                let mut next_allowed = 0usize; // enforce non-overlap
                for idx in memchr_iter(anchor_byte, haystack) {
                    if idx < anchor_idx {
                        continue;
                    }
                    let start = idx - anchor_idx;
                    if start < next_allowed {
                        continue;
                    }
                    let end = start + len;
                    if end > haystack.len() {
                        continue;
                    }
                    if matches_at(&self.search, &haystack[start..end]) {
                        out.push(start);
                        next_allowed = end;
                    }
                }
                out
            }
            // All-wildcard pattern: every window matches by definition.
            // There's no useful anchor byte, so just step through
            // non-overlapping windows directly.
            None => {
                let mut out = Vec::new();
                let mut pos = 0;
                while pos + len <= haystack.len() {
                    out.push(pos);
                    pos += len;
                }
                out
            }
        }
    }

    /// Find the first match at or after `from`. Useful for an interactive
    /// "find next" / step-through-and-confirm replace flow.
    #[allow(dead_code)] // kept alongside ind_all; the dialogs use the list
    pub fn find_next(&self, haystack: &[u8], from: usize) -> Option<usize> {
        let len = self.search.len();
        if from > haystack.len() || haystack.len() - from < len {
            return None;
        }

        match self.anchor {
            Some((anchor_idx, anchor_byte)) => {
                for idx in memchr_iter(anchor_byte, &haystack[from..]) {
                    let idx = idx + from;
                    if idx < anchor_idx {
                        continue;
                    }
                    let start = idx - anchor_idx;
                    if start < from {
                        continue;
                    }
                    let end = start + len;
                    if end > haystack.len() {
                        continue;
                    }
                    if matches_at(&self.search, &haystack[start..end]) {
                        return Some(start);
                    }
                }
                None
            }
            None => {
                if from + len <= haystack.len() {
                    Some(from)
                } else {
                    None
                }
            }
        }
    }

    /// Replace all non-overlapping matches in place. Wildcard slots in the
    /// replace pattern leave the original byte untouched. Returns the
    /// offsets that were replaced.
    pub fn replace_all(&self, haystack: &mut [u8]) -> Vec<usize> {
        let matches = self.find_all(haystack);
        let len = self.search.len();
        for &start in &matches {
            apply_replace(&self.replace, &mut haystack[start..start + len]);
        }
        matches
    }

    /// Replace a single match at a known offset (e.g. one already found via
    /// `find_next`, after the user confirms it in an interactive flow).
    /// Returns `false` (no-op) if the bytes at `start` no longer match —
    /// which matters if the buffer changed between "find" and "confirm".
    pub fn replace_at(&self, haystack: &mut [u8], start: usize) -> bool {
        let len = self.search.len();
        if start + len > haystack.len() {
            return false;
        }
        if !matches_at(&self.search, &haystack[start..start + len]) {
            return false;
        }
        apply_replace(&self.replace, &mut haystack[start..start + len]);
        true
    }
}

/// Cap on how much of `haystack_hint` we'll scan to build the byte-frequency
/// table for anchor selection. Callers may legitimately pass an entire
/// multi-gigabyte file as the "hint" (per the doc comment on
/// `HexReplacer::new`); a bounded sample gives an equally good anchor choice
/// in practice while keeping anchor selection O(1) with respect to input
/// size instead of O(file size).
const ANCHOR_HINT_SAMPLE_CAP: usize = 1 << 20; // 1 MiB

/// Pick the literal pattern byte that is rarest in `haystack_hint`, to
/// minimize false-positive candidates from memchr. `None` if the pattern
/// has no literal bytes at all (all wildcards).
fn pick_anchor(pattern: &HexPattern, haystack_hint: &[u8]) -> Option<(usize, u8)> {
    let literal_positions: Vec<(usize, u8)> = pattern
        .as_slice()
        .iter()
        .enumerate()
        .filter_map(|(i, pb)| match pb {
            PatternByte::Literal(b) => Some((i, *b)),
            PatternByte::Wildcard => None,
        })
        .collect();

    // Nothing to choose between: skip building the frequency table
    // entirely. This is the common case for short/simple patterns.
    if literal_positions.len() <= 1 {
        return literal_positions.first().copied();
    }

    let sample = if haystack_hint.len() > ANCHOR_HINT_SAMPLE_CAP {
        &haystack_hint[..ANCHOR_HINT_SAMPLE_CAP]
    } else {
        haystack_hint
    };

    let mut freq = [0u64; 256];
    for &b in sample {
        freq[b as usize] += 1;
    }

    // `min_by_key` keeps the first element on ties, matching the original
    // "first strictly-lower count wins" selection order.
    literal_positions
        .into_iter()
        .min_by_key(|&(_, b)| freq[b as usize])
}

/// Check whether `pattern` matches `window` (same length), honoring wildcards.
///
/// Instead of walking `pattern`/`window` one byte at a time and branching on
/// `PatternByte` for every position, this compares whole contiguous literal
/// runs at once via slice equality (`==`), which the compiler can lower to
/// `memcmp` / vectorized comparisons. Wildcard positions are skipped
/// entirely since they match unconditionally.
#[inline]
fn matches_at(pattern: &HexPattern, window: &[u8]) -> bool {
    debug_assert_eq!(pattern.len(), window.len());
    pattern
        .literal_runs
        .iter()
        .all(|r| pattern.flat[r.start..r.end] == window[r.start..r.end])
}

/// Overwrite `window` with `replace`, leaving wildcard slots untouched.
///
/// As with `matches_at`, this copies whole contiguous literal runs via
/// `copy_from_slice` (a `memcpy`) rather than looping byte-by-byte with a
/// branch, and skips wildcard runs entirely instead of visiting-and-ignoring
/// each wildcard slot.
#[inline]
fn apply_replace(replace: &HexPattern, window: &mut [u8]) {
    for r in &replace.literal_runs {
        window[r.start..r.end].copy_from_slice(&replace.flat[r.start..r.end]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(s: &str) -> HexPattern {
        HexPattern::parse(s).unwrap()
    }

    #[test]
    fn parse_forms() {
        assert_eq!(pat("DE AD ?? EF").len(), 4);
        assert_eq!(pat("DEAD??EF").len(), 4);
        assert!(HexPattern::parse("").is_err());
        assert!(HexPattern::parse("ABC").is_err());
        assert!(HexPattern::parse("ZZ").is_err());
    }

    #[test]
    fn find_and_replace_basic() {
        let search = pat("DE AD ?? EF");
        let replace = pat("00 00 ?? 00");
        let mut data = vec![0x11, 0xDE, 0xAD, 0x99, 0xEF, 0x22, 0xDE, 0xAD, 0x77, 0xEF];
        let r = HexReplacer::new(search, replace, &data).unwrap();
        let hits = r.find_all(&data);
        assert_eq!(hits, vec![1, 6]);
        r.replace_all(&mut data);
        assert_eq!(
            data,
            vec![0x11, 0x00, 0x00, 0x99, 0x00, 0x22, 0x00, 0x00, 0x77, 0x00]
        );
    }

    #[test]
    fn wildcard_only_pattern() {
        let search = pat("?? ??");
        let replace = pat("?? ??");
        let data = vec![1, 2, 3, 4, 5];
        let r = HexReplacer::new(search, replace, &data).unwrap();
        assert_eq!(r.find_all(&data), vec![0, 2]); // non-overlapping windows
    }

    #[test]
    fn find_next_and_replace_at() {
        let search = pat("AA BB");
        let replace = pat("CC ??");
        let mut data = vec![0xAA, 0xBB, 0x00, 0xAA, 0xBB];
        let r = HexReplacer::new(search, replace, &data).unwrap();
        let first = r.find_next(&data, 0).unwrap();
        assert_eq!(first, 0);
        assert!(r.replace_at(&mut data, first));
        assert_eq!(data[0..2], [0xCC, 0xBB]);
        let second = r.find_next(&data, first + 1).unwrap();
        assert_eq!(second, 3);
    }

    #[test]
    fn length_mismatch_rejected() {
        let search = pat("AA BB CC");
        let replace = pat("DD EE");
        assert!(HexReplacer::new(search, replace, &[]).is_err());
    }

    #[test]
    fn anchor_picks_rarest_byte() {
        let mut hint = vec![0xAAu8; 10_000];
        hint.push(0xFF);
        let search = pat("AA FF");
        let replace = pat("00 00");
        let r = HexReplacer::new(search, replace, &hint).unwrap();
        assert_eq!(r.anchor, Some((1, 0xFF)));
    }

    #[test]
    fn long_literal_run_matches_and_replaces() {
        let search_str: String = (0..64).map(|i| format!("{:02X}", i as u8)).collect::<Vec<_>>().join(" ");
        let search = pat(&search_str);
        let mut replace_tokens: Vec<String> = (0..64).map(|i| format!("{:02X}", (i as u8).wrapping_add(1))).collect();
        replace_tokens[30] = "??".to_string();
        let replace = pat(&replace_tokens.join(" "));

        let mut data: Vec<u8> = (0..64u8).collect();
        let r = HexReplacer::new(search, replace, &data).unwrap();
        assert_eq!(r.find_all(&data), vec![0]);
        r.replace_all(&mut data);
        for i in 0..64usize {
            if i == 30 {
                assert_eq!(data[i], 30u8);
            } else {
                assert_eq!(data[i], (i as u8).wrapping_add(1));
            }
        }
    }
}
