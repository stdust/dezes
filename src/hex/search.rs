use crate::app::App;

#[derive(Default, Debug)]
pub struct Search {
    pub direction: SearchDirection,
    pub matches: Vec<usize>,
    pub match_index: Option<usize>,
    /// Length of the pattern that produced `matches`.
    ///
    /// Needed to paint the hits: a match is a *range*, and without its length the
    /// view could only mark the first byte of each one.
    pub match_len: usize,
}

/// True when `offset` falls inside one of the current search hits.
///
/// `matches` is sorted, so this is a binary search per byte rather than a scan of
/// the list - with the cap at 5000 hits and a screenful of bytes to draw every
/// frame, the difference is between a few hundred comparisons and a few million.
pub fn in_match(app: &App, offset: usize) -> bool {
    let search = &app.hex_view.search;
    if search.matches.is_empty() || search.match_len == 0 {
        return false;
    }
    let idx = match search.matches.binary_search(&offset) {
        Ok(_) => return true,
        Err(0) => return false,
        Err(insert) => insert - 1,
    };
    let start = search.matches[idx];
    offset < start.saturating_add(search.match_len)
}

#[derive(Default, PartialEq, Debug)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HexPatternByte {
    Exact(u8),
    Wildcard,
}

/// Parses a run of hex characters (already known to have even length) into
/// pattern bytes, pushing them onto `pattern`. Shared by both the
/// "single concatenated blob" and "token by token" parsing paths below,
/// so the pairwise-hex-decoding logic only lives in one place.
fn push_hex_pairs(s: &str, pattern: &mut Vec<HexPatternByte>) -> Option<()> {
    if s.is_empty() || s.len() % 2 != 0 {
        return None;
    }
    for chunk in s.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).ok()?;
        if pair == "??" {
            pattern.push(HexPatternByte::Wildcard);
        } else {
            pattern.push(HexPatternByte::Exact(u8::from_str_radix(pair, 16).ok()?));
        }
    }
    Some(())
}

pub fn hex_string_to_pattern(hex_string: &str) -> Option<Vec<HexPatternByte>> {
    let cleaned = hex_string.replace("0x", "").replace("0X", "");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    let mut pattern = Vec::new();

    // A single long token (no internal whitespace) is treated as one
    // concatenated hex blob, e.g. "deadbeef" -> DE AD BE EF.
    // Everything else (multiple tokens, or a single byte/nibble-sized
    // token) is parsed token by token, which is what lets wildcards be
    // mixed in with individual bytes, e.g. "de ?? be".
    let single_blob = tokens.len() == 1 && tokens[0].len() > 2;

    if single_blob {
        push_hex_pairs(tokens[0], &mut pattern)?;
    } else {
        for token in tokens.iter().filter(|t| !t.is_empty()) {
            match *token {
                "?" | "??" => pattern.push(HexPatternByte::Wildcard),
                t if t.len() == 1 => {
                    pattern.push(HexPatternByte::Exact(u8::from_str_radix(t, 16).ok()?));
                }
                t if t.len() % 2 == 0 => push_hex_pairs(t, &mut pattern)?,
                _ => return None,
            }
        }
    }

    if pattern.is_empty() {
        None
    } else {
        Some(pattern)
    }
}

/// Finds every start offset in the file where `pattern` matches, wildcards
/// included.
///
/// Rather than testing *every* position in the file byte-by-byte
/// (O(filesize * pattern.len())), this anchors the scan on the first
/// non-wildcard byte in the pattern and uses `memchr` (SIMD-accelerated)
/// to jump straight to candidate positions, only doing the full
/// byte-by-byte comparison on those candidates. For a mostly-exact
/// pattern with a handful of wildcards this is close to O(filesize).
/// A pattern that is entirely wildcards has no anchor, so every position
/// trivially matches and is added directly.
/// Upper bound on collected search hits.
///
/// Each hit is a `usize` in a `Vec`, so an unbounded count scales the allocation
/// with the file: a pattern that matches everywhere (`??`) reserved eight bytes
/// per file byte. The cap also bounds the status bar's `(n/m)` counter and the
/// F3/Shift+F3 walk to something a person can actually work through.
pub const MAX_MATCHES: usize = 100_000;

/// True when a result list was cut short by [`MAX_MATCHES`].
pub fn matches_truncated(matches: &[usize]) -> bool {
    matches.len() >= MAX_MATCHES
}

pub fn find_all_pattern_matches(app: &mut App, pattern: &[HexPatternByte]) -> Vec<usize> {
    // `with_effective_buffer` hands over the mmap slice directly when there are
    // no pending edits, instead of copying the whole file onto the heap for
    // every single search.
    app.with_effective_buffer(|buffer| {
        let mut matches = Vec::new();
        // Bound everything by the buffer we actually got, not by `file_info.size`
        // (which can be larger and would invert the window slice below).
        let filesize = buffer.len();

        if filesize == 0 || pattern.is_empty() || pattern.len() > filesize {
            return matches;
        }

        let match_at = |start_idx: usize| -> bool {
            if start_idx + pattern.len() > buffer.len() {
                return false;
            }
            pattern.iter().enumerate().all(|(i, pat)| match pat {
                HexPatternByte::Exact(b) => buffer[start_idx + i] == *b,
                HexPatternByte::Wildcard => true,
            })
        };

        let max_pos = filesize.saturating_sub(pattern.len());

        let anchor = pattern.iter().enumerate().find_map(|(i, p)| match p {
            HexPatternByte::Exact(b) => Some((i, *b)),
            HexPatternByte::Wildcard => None,
        });

        match anchor {
            Some((anchor_idx, anchor_byte)) => {
                // Byte range in `buffer` where the anchor byte could land for
                // start positions 0..=max_pos.
                let window_start = anchor_idx.min(buffer.len());
                let window_end = (max_pos + anchor_idx + 1).min(buffer.len()).max(window_start);
                let haystack = &buffer[window_start..window_end];

                let mut offset = 0;
                while let Some(rel_pos) = memchr::memchr(anchor_byte, &haystack[offset..]) {
                    let found_at = offset + rel_pos;
                    let start_idx = found_at; // haystack[k] == buffer[k + anchor_idx]
                    if match_at(start_idx) {
                        matches.push(start_idx);
                        if matches.len() >= MAX_MATCHES {
                            break;
                        }
                    }
                    offset = found_at + 1;
                    if offset >= haystack.len() {
                        break;
                    }
                }
            }
            None => {
                // Pattern is all wildcards: every position matches.
                //
                // This used to reserve and fill one entry per byte in the file -
                // eight bytes of Vec per file byte, so 40 MB for a 5 MB file and
                // 8 GB for a 1 GB one, which is an out-of-memory abort rather
                // than a slow search. A list that long is not navigable anyway.
                let count = (max_pos + 1).min(MAX_MATCHES);
                matches.reserve(count);
                matches.extend(0..count);
            }
        }

        matches
    })
}

/// Finds the index within the (sorted) `matches` list closest to
/// `target_ofs`. Uses `binary_search` and only inspects the immediate
/// neighbors of the insertion point, so this is O(log n) instead of
/// scanning every match to find the minimum distance.
pub fn update_match_index(app: &mut App, target_ofs: usize) {
    let matches = &app.hex_view.search.matches;

    if matches.is_empty() {
        app.hex_view.search.match_index = None;
        return;
    }

    let idx = match matches.binary_search(&target_ofs) {
        Ok(idx) => idx,
        Err(insert_pos) => {
            if insert_pos == 0 {
                0
            } else if insert_pos == matches.len() {
                matches.len() - 1
            } else {
                let prev = matches[insert_pos - 1];
                let next = matches[insert_pos];
                if target_ofs - prev <= next - target_ofs {
                    insert_pos - 1
                } else {
                    insert_pos
                }
            }
        }
    };

    app.hex_view.search.match_index = Some(idx);
}

/// `"Found match at offset 0x1234"` or, in the Disasm view, `"Found match at
/// VA 0x140001234"` - matches are always found by file offset, but Disasm
/// users think in virtual addresses, so the message (and F3/Shift+F3, via
/// [`goto_adjacent_match`]) reports whichever address space the current view
/// uses.
pub fn found_at_message(app: &App, offset: usize) -> String {
    match_position_message(app, offset)
}

/// `"Match (1341/2063) offset : 0x2290"`, or `VA :` in the Disasm view / VA mode.
///
/// The counter is the point: "found a match" said nothing about how many there
/// were, so there was no way to judge what a Replace All would do. The index comes
/// from the shared match list, which both the Find and Replace dialogs fill.
pub fn match_position_message(app: &App, offset: usize) -> String {
    let lang = app.config.lang;
    let total = app.hex_view.search.matches.len();
    let index = app
        .hex_view
        .search
        .matches
        .binary_search(&offset)
        .map(|i| i + 1)
        .unwrap_or_else(|_| app.hex_view.search.match_index.map(|i| i + 1).unwrap_or(1));

    let as_va = app.editor_view == crate::editor::AppView::Disasm || app.hex_view.show_va;
    let (message, address) = if as_va {
        (crate::i18n::M::MatchAtVa, app.get_va(offset))
    } else {
        (crate::i18n::M::MatchAtOffset, offset as u64)
    };

    crate::i18n::fill(
        message.tr(lang),
        &[
            &index.to_string(),
            &total.to_string(),
            &format!("{:X}", address),
        ],
    )
}

/// Moves to the next (or previous) match in the *already computed* matches
/// list, without re-running the search. This is what F3 / Shift+F3 use to
/// repeat the last Find/Replace pattern search - the pattern itself doesn't
/// change, only the direction, so there's no need to re-scan the file.
pub fn goto_adjacent_match(app: &mut App, forward: bool) {
    if app.hex_view.search.matches.is_empty() {
        crate::beep!();
        return;
    }

    app.hex_view.search.direction = if forward {
        SearchDirection::Forward
    } else {
        SearchDirection::Backward
    };

    let existing = app.hex_view.search.matches.clone();
    if let Some(ofs) = apply_search_results(app, existing) {
        app.goto(ofs);
    }
}

/// Shared tail-end of `search` / `search_pattern`: stores the match list,
/// picks the next match in the configured direction (honoring wraparound),
/// and updates `match_index`. `matches` is sorted ascending (both finders
/// build it in scan order), so the direction lookup uses `partition_point`
/// (binary search, O(log n)) instead of a linear `find`/`rfind` scan.
fn apply_search_results(app: &mut App, all_matches: Vec<usize>) -> Option<usize> {
    if all_matches.is_empty() {
        app.hex_view.search.matches.clear();
        app.hex_view.search.match_index = None;
        crate::beep!();
        return None;
    }

    if matches_truncated(&all_matches) {
        // Said out loud, because the status bar's `(n/m)` would otherwise present
        // a capped total as if it were the whole answer.
        crate::app::App::log(
            app,
            format!(
                "Search stopped at the first {} matches - narrow the pattern to see the rest",
                MAX_MATCHES
            ),
        );
    }

    app.hex_view.search.matches = all_matches;

    let current_ofs = app.hex_view.offset;
    let wrap = app.config.search_wrap;
    let forward = app.hex_view.search.direction == SearchDirection::Forward;

    let target_ofs = {
        let matches = &app.hex_view.search.matches;
        if forward {
            let idx = matches.partition_point(|&ofs| ofs <= current_ofs);
            matches
                .get(idx)
                .copied()
                .or(if wrap { matches.first().copied() } else { None })
        } else {
            let idx = matches.partition_point(|&ofs| ofs < current_ofs);
            if idx > 0 {
                Some(matches[idx - 1])
            } else if wrap {
                matches.last().copied()
            } else {
                None
            }
        }
    };

    if let Some(ofs) = target_ofs {
        update_match_index(app, ofs);
        Some(ofs)
    } else {
        crate::beep!();
        None
    }
}

pub fn search_pattern(app: &mut App, pattern: &[HexPatternByte]) -> Option<usize> {
    let all_matches = find_all_pattern_matches(app, pattern);
    // Recorded so the view can paint the whole of each hit, not just its first byte.
    app.hex_view.search.match_len = pattern.len();
    apply_search_results(app, all_matches)
}

#[cfg(test)]
mod match_cap_tests {
    use super::*;

    fn app_with(bytes: &[u8]) -> App {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join("dz6_match_cap");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("s_{}.bin", id));
        std::fs::write(&path, bytes).expect("write");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("path"), 0, true)
            .expect("open");
        app
    }

    /// A pattern that matches everywhere must not allocate one entry per file
    /// byte.
    ///
    /// `??` used to reserve and fill `filesize` entries - eight bytes of `Vec` per
    /// file byte, i.e. 40 MB for a 5 MB file and 8 GB for a 1 GB one, which aborts
    /// rather than searching slowly.
    #[test]
    fn wildcard_only_pattern_is_capped() {
        let size = MAX_MATCHES + 5_000;
        let mut app = app_with(&vec![0u8; size]);
        let pattern = hex_string_to_pattern("??").expect("pattern");

        let matches = find_all_pattern_matches(&mut app, &pattern);

        assert_eq!(matches.len(), MAX_MATCHES, "the result list must be capped");
        assert!(matches_truncated(&matches));
        // Still in scan order, which `apply_search_results` relies on for its
        // binary searches.
        assert!(matches.windows(2).all(|w| w[0] < w[1]));
    }

    /// An anchored pattern with more hits than the cap stops too.
    #[test]
    fn anchored_pattern_is_capped() {
        let size = MAX_MATCHES + 5_000;
        let mut app = app_with(&vec![0xAAu8; size]);
        let pattern = hex_string_to_pattern("AA").expect("pattern");

        let matches = find_all_pattern_matches(&mut app, &pattern);

        assert_eq!(matches.len(), MAX_MATCHES);
        assert!(matches_truncated(&matches));
    }

    /// Below the cap, nothing changes: every hit is reported and the list is not
    /// flagged as truncated.
    #[test]
    fn small_result_sets_are_complete() {
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x10] = 0xAA;
        bytes[0x20] = 0xAA;
        bytes[0x30] = 0xAA;
        let mut app = app_with(&bytes);
        let pattern = hex_string_to_pattern("AA").expect("pattern");

        let matches = find_all_pattern_matches(&mut app, &pattern);

        assert_eq!(matches, vec![0x10, 0x20, 0x30]);
        assert!(!matches_truncated(&matches));
    }

    /// Wildcards inside an anchored pattern still match, capped or not.
    #[test]
    fn wildcards_within_a_pattern_still_match() {
        let mut bytes = vec![0u8; 0x100];
        bytes[0x40] = 0xAA;
        bytes[0x41] = 0x99;
        bytes[0x42] = 0xBB;
        let mut app = app_with(&bytes);
        let pattern = hex_string_to_pattern("AA ?? BB").expect("pattern");

        assert_eq!(find_all_pattern_matches(&mut app, &pattern), vec![0x40]);
    }
}

#[cfg(test)]
mod in_match_tests {
    use super::*;
    use crate::app::App;

    fn app_with_matches(matches: Vec<usize>, len: usize) -> App {
        let mut app = App::new();
        app.config.database = false;
        app.hex_view.search.matches = matches;
        app.hex_view.search.match_len = len;
        app
    }

    /// Every byte of a hit is marked, not just its first.
    #[test]
    fn the_whole_match_is_covered() {
        let app = app_with_matches(vec![0x10, 0x40], 3);

        for ofs in 0x10..0x13 {
            assert!(in_match(&app, ofs), "0x{:X} is inside the first hit", ofs);
        }
        assert!(!in_match(&app, 0x13), "one past the hit is outside it");
        assert!(!in_match(&app, 0x0F), "one before it is outside it");
        for ofs in 0x40..0x43 {
            assert!(in_match(&app, ofs), "0x{:X} is inside the second hit", ofs);
        }
    }

    /// No search, no highlight - including the case where a length was left behind
    /// without any hits, or hits without a length.
    #[test]
    fn nothing_is_marked_without_a_search() {
        let app = app_with_matches(Vec::new(), 4);
        assert!(!in_match(&app, 0));

        let app = app_with_matches(vec![0x10], 0);
        assert!(
            !in_match(&app, 0x10),
            "a zero-length pattern must not paint the file"
        );
    }

    /// A single-byte pattern marks exactly one byte per hit.
    #[test]
    fn single_byte_patterns_mark_one_byte() {
        let app = app_with_matches(vec![0, 2, 4], 1);
        assert!(in_match(&app, 0));
        assert!(!in_match(&app, 1));
        assert!(in_match(&app, 2));
        assert!(!in_match(&app, 3));
        assert!(in_match(&app, 4));
        assert!(!in_match(&app, 5));
    }

    /// Adjacent and overlapping hits must not leave gaps: the answer for a byte
    /// only depends on the hit that starts at or before it.
    #[test]
    fn adjacent_hits_have_no_gaps() {
        let app = app_with_matches(vec![0x10, 0x12, 0x14], 2);
        for ofs in 0x10..0x16 {
            assert!(in_match(&app, ofs), "0x{:X} should be covered", ofs);
        }
        assert!(!in_match(&app, 0x16));
    }
}

#[cfg(test)]
mod counter_tests {
    use super::*;
    use crate::app::App;

    fn app_with_hits(hits: Vec<usize>, len: usize) -> App {
        let mut app = App::new();
        app.config.database = false;
        app.hex_view.search.matches = hits;
        app.hex_view.search.match_len = len;
        app
    }

    /// The result row reads "(index/total) offset : 0xADDR".
    ///
    /// "Found a match" alone gave no sense of scale - with 12,578 hits in a file,
    /// the count is what tells you what a Replace All would do.
    #[test]
    fn the_counter_reports_position_and_total() {
        let app = app_with_hits(vec![0x100, 0x2289, 0x4000], 3);

        assert_eq!(
            match_position_message(&app, 0x2289),
            "Match (2/3) offset : 0x2289"
        );
        assert_eq!(match_position_message(&app, 0x100), "Match (1/3) offset : 0x100");
        assert_eq!(
            match_position_message(&app, 0x4000),
            "Match (3/3) offset : 0x4000"
        );
    }

    /// In VA mode the address is the virtual one, and says so.
    #[test]
    fn va_mode_reports_a_va() {
        let mut app = app_with_hits(vec![0x100], 1);
        app.hex_view.show_va = true;

        let message = match_position_message(&app, 0x100);
        assert!(message.starts_with("Match (1/1) VA : 0x"), "got: {}", message);
        assert!(!message.contains("offset"));
    }

    /// An offset that is not itself a hit still gets a sensible position rather
    /// than a panic - the cursor can sit mid-match.
    #[test]
    fn an_offset_between_hits_falls_back_to_the_index() {
        let mut app = app_with_hits(vec![0x100, 0x200], 4);
        app.hex_view.search.match_index = Some(1);

        let message = match_position_message(&app, 0x101);
        assert!(message.starts_with("Match (2/2)"), "got: {}", message);
    }
}
