//! Instruction-boundary arithmetic for the disassembly view.
//!
//! Every navigation key used to work this out for itself, from a window anchored
//! at `reader.page_start`, and each did it slightly differently:
//!
//! * `Down` decided it had reached the cursor with `cur_ofs >= offset`, so with
//!   the cursor inside an instruction the flag first tripped on the *following*
//!   one and the key moved two instructions.
//! * `Down` fell back to `offset + 1` whenever the cursor was more than 1 KiB
//!   past `page_start`, leaving the cursor mid-instruction.
//! * `Up` started decoding at `offset - 64`, which is not a boundary, so it could
//!   jump the full 64 bytes or land mid-instruction.
//! * `PageUp` estimated a page as `rows * 3` bytes.
//! * `End` parked `page_start` on the last byte, showing one row.
//! * Follow scanned forward from `page_start` for at most 1 KiB looking for the
//!   cursor, so on a tall terminal the lower rows did nothing at all.
//!
//! All of them are the same two questions - what is the next boundary, what is
//! the previous one - so they live here once.
//!
//! Decoding also goes through the pending edits, which the navigation keys used
//! to ignore even though `draw.rs` applies them: after patching a byte, the
//! lengths the cursor moved by disagreed with the instructions on screen.

use iced_x86::{Decoder, DecoderOptions};

use crate::app::App;

/// Longest possible x86 instruction.
pub const MAX_INSTR_BYTES: usize = 16;

/// How far back to look for a boundary that lines up with the cursor.
///
/// Four maximum-length instructions: enough to re-sync in practice, and bounded
/// so a keypress can never do more than a few hundred decodes.
const BACK_WINDOW: usize = MAX_INSTR_BYTES * 4;

/// Last offset the view may put the cursor on.
fn last_offset(app: &App) -> usize {
    scan_limit(app).saturating_sub(1)
}

/// Upper bound for every offset here: the live mapping, not `file_info.size`,
/// which comes from the directory entry and can be larger.
fn scan_limit(app: &App) -> usize {
    app.file_info.size.min(app.file_info.buffer_len())
}

/// Copy of `start..end` with the pending edits applied.
///
/// A small window rather than `with_effective_buffer`, which copies the entire
/// file as soon as anything has been edited - unacceptable per keypress.
fn window(app: &App, start: usize, end: usize) -> Vec<u8> {
    let buffer = app.file_info.get_buffer_ref();
    let end = end.min(buffer.len());
    if start >= end {
        return Vec::new();
    }
    let mut bytes = buffer[start..end].to_vec();
    if !app.hex_view.changed_bytes.is_empty() {
        for (i, byte) in bytes.iter_mut().enumerate() {
            if let Some(hex) = app.hex_view.changed_bytes.get(&(start + i))
                && let Ok(v) = u8::from_str_radix(hex.trim(), 16)
            {
                *byte = v;
            }
        }
    }
    bytes
}

/// Length of the instruction starting at `offset`, or `None` if nothing decodes.
///
/// With no pending edits the decoder reads the mapping in place. It used to copy a
/// 16-byte window onto the heap for *every* call, and `sync_from_behind` makes
/// hundreds of calls per keypress, so that allocation was the single hottest thing
/// in disassembly navigation.
pub fn instruction_len(app: &App, offset: usize) -> Option<usize> {
    let limit = scan_limit(app);
    if offset >= limit {
        return None;
    }
    let end = offset.saturating_add(MAX_INSTR_BYTES).min(limit);

    if app.hex_view.changed_bytes.is_empty() {
        let buffer = app.file_info.get_buffer_ref();
        let end = end.min(buffer.len());
        if offset >= end {
            return None;
        }
        return decode_len(app, &buffer[offset..end], offset);
    }

    let bytes = window(app, offset, end);
    if bytes.is_empty() {
        return None;
    }
    decode_len(app, &bytes, offset)
}

/// Length of the first instruction in `bytes`, which start at file `offset`.
fn decode_len(app: &App, bytes: &[u8], offset: usize) -> Option<usize> {
    let decoder = Decoder::with_ip(
        app.bitness(),
        bytes,
        app.get_va(offset),
        DecoderOptions::NONE,
    );
    decoder.into_iter().next().map(|i| i.len()).filter(|l| *l > 0)
}

/// Start of the instruction that contains `offset`.
///
/// Equal to `offset` when it is already a boundary. Off a boundary - which
/// happens after a raw `:goto`, or a jump into the middle of an instruction -
/// this is the row the view actually draws the cursor on, so it is the right
/// basis for moving up or down.
pub fn containing_instruction(app: &App, offset: usize) -> usize {
    match sync_from_behind(app, offset) {
        Some(chain) => chain.containing,
        None => offset,
    }
}

/// Result of re-syncing the instruction stream ahead of `offset`.
struct Sync {
    /// Start of the instruction containing `offset` (equal to `offset` when it is
    /// itself a boundary).
    containing: usize,
    /// Start of the instruction before that one.
    previous: usize,
}

/// Re-synchronises the instruction stream at `offset` by decoding forward from
/// each candidate start in the preceding window.
///
/// x86 cannot be decoded backwards, and it is self-synchronising: a chain started
/// at the wrong byte usually realigns after a few instructions, so *many*
/// candidates produce a chain that passes through `offset`. Taking the first one
/// that fits picked an arbitrary answer - it reported `0x104` as its own boundary
/// in a stream of five-byte instructions starting at `0x100`.
///
/// The candidate that decodes the *most* instructions before reaching `offset`
/// wins instead. That is the usual linear-sweep heuristic: a correctly aligned
/// chain runs the whole window, while a misaligned one wastes bytes realigning
/// and yields fewer instructions. Ties go to the earliest candidate so the result
/// is deterministic.
fn sync_from_behind(app: &App, offset: usize) -> Option<Sync> {
    if offset == 0 {
        return Some(Sync {
            containing: 0,
            previous: 0,
        });
    }
    let lo = offset.saturating_sub(BACK_WINDOW);

    // Every candidate walks forward over the same bytes, so the same offsets get
    // decoded again and again - up to `BACK_WINDOW` times each. Lengths are a pure
    // function of the offset here, so they are decoded once and reused, which turns
    // the loop below from quadratic into linear in the window size.
    let mut lengths: Vec<Option<Option<usize>>> = vec![None; offset - lo];
    let mut len_at = |cursor: usize| -> Option<usize> {
        let idx = cursor - lo;
        match lengths.get(idx) {
            Some(Some(cached)) => *cached,
            Some(None) => {
                let len = instruction_len(app, cursor);
                lengths[idx] = Some(len);
                len
            }
            // Outside the memoised window; the walks below never ask for this.
            None => instruction_len(app, cursor),
        }
    };

    let mut best: Option<(usize, Sync)> = None; // (instruction count, sync)

    for start in lo..=offset {
        let mut cursor = start;
        let mut previous = start;
        let mut count = 0usize;
        let mut hit = None;

        while cursor <= offset {
            if cursor == offset {
                hit = Some(Sync {
                    containing: offset,
                    previous,
                });
                break;
            }
            let Some(len) = len_at(cursor) else {
                break;
            };
            if cursor + len > offset {
                // `offset` is inside this instruction.
                hit = Some(Sync {
                    containing: cursor,
                    previous,
                });
                break;
            }
            previous = cursor;
            cursor += len;
            count += 1;
        }

        if let Some(sync) = hit
            && best.as_ref().is_none_or(|(best_count, _)| count > *best_count)
        {
            best = Some((count, sync));
        }
    }

    best.map(|(_, sync)| sync)
}

/// Start of the instruction after the one the cursor is on.
///
/// Anchored on the *containing* instruction, so from the middle of a five-byte
/// instruction this lands on the next row rather than decoding whatever byte the
/// cursor happens to sit on. The old handler's `cur_ofs >= offset` test made this
/// skip a row instead.
pub fn next_instruction(app: &App, offset: usize) -> usize {
    let last = last_offset(app);
    let base = containing_instruction(app, offset);
    match instruction_len(app, base) {
        Some(len) => base.saturating_add(len).min(last),
        // Undecodable byte: still move, or the key would appear dead.
        None => offset.saturating_add(1).min(last),
    }
}

/// Start of the instruction before the one at `offset`.
///
/// x86 cannot be decoded backwards, so this re-syncs: for each candidate start
/// in the window before the cursor, decode forward and keep the one whose
/// instruction chain lands exactly on `offset`. The farthest candidate that
/// lines up wins, which is the usual heuristic and matches what the view shows,
/// since the view decodes forward too.
pub fn prev_instruction(app: &App, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    match sync_from_behind(app, offset) {
        // Off a boundary, the row above the cursor is the containing instruction
        // itself; on one, it is the instruction before it.
        Some(sync) if sync.containing < offset => sync.containing,
        Some(sync) if sync.previous < offset => sync.previous,
        // Nothing lined up (data, or an undecodable region): step one byte so the
        // key still responds.
        _ => offset - 1,
    }
}

/// `count` instructions forward.
pub fn advance(app: &App, offset: usize, count: usize) -> usize {
    let last = last_offset(app);
    let mut current = offset;
    for _ in 0..count {
        if current >= last {
            break;
        }
        let next = next_instruction(app, current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// `count` instructions backward.
pub fn retreat(app: &App, offset: usize, count: usize) -> usize {
    let mut current = offset;
    for _ in 0..count {
        if current == 0 {
            break;
        }
        let prev = prev_instruction(app, current);
        if prev == current {
            break;
        }
        current = prev;
    }
    current
}

/// Page start that puts `offset` on the last of `rows` visible rows.
pub fn page_start_ending_at(app: &App, offset: usize, rows: usize) -> usize {
    retreat(app, offset, rows.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built instruction sequence with known lengths, so the expected
    /// boundaries are not themselves computed by the code under test.
    ///
    /// nop(1) nop(1) push rax(1) mov eax,imm32(5) ret(1) int3(1)
    const CODE: &[u8] = &[
        0x90, // 0: nop
        0x90, // 1: nop
        0x50, // 2: push rax
        0xB8, 0x78, 0x56, 0x34, 0x12, // 3..8: mov eax, 0x12345678
        0xC3, // 8: ret
        0xCC, // 9: int3
    ];
    /// Boundaries in `CODE`.
    const BOUNDS: &[usize] = &[0, 1, 2, 3, 8, 9];

    /// Panics rather than returning `None`: an earlier version handed back
    /// `Option` and every test quietly passed by returning early when the fixture
    /// could not be written, which hid a real disagreement about what `Down`
    /// should do from mid-instruction.
    fn app_with_code() -> App {
        // A distinct file per call. Loading maps the file, and tests run in
        // parallel, so a shared path made `write` fail with "the file is in use
        // by another process" (ERROR_USER_MAPPED_FILE) - which the previous
        // `Option`-returning helper turned into a silent skip.
        static FIXTURE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let dir = std::env::temp_dir().join("dz6_nav_tests");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(format!("code_{}.bin", id));
        // Padded so the last instruction is not at EOF.
        let mut bytes = CODE.to_vec();
        bytes.resize(0x40, 0x90);
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut app = App::new();
        app.config.database = false;
        app.load_file(path.to_str().expect("utf-8 path"), 0, true)
            .expect("open fixture");
        app
    }

    #[test]
    fn instruction_lengths_are_decoded() {
        let app = app_with_code();
        assert_eq!(instruction_len(&app, 0), Some(1), "nop");
        assert_eq!(instruction_len(&app, 2), Some(1), "push rax");
        assert_eq!(instruction_len(&app, 3), Some(5), "mov eax, imm32");
        assert_eq!(instruction_len(&app, 8), Some(1), "ret");
    }

    /// Down must land on the next boundary, from anywhere - including from inside
    /// an instruction, where the old handler skipped a line.
    #[test]
    fn next_instruction_walks_boundaries() {
        let app = app_with_code();
        for pair in BOUNDS.windows(2) {
            assert_eq!(
                next_instruction(&app, pair[0]),
                pair[1],
                "from boundary 0x{:X}",
                pair[0]
            );
        }
        // From the middle of the 5-byte mov, the next boundary is still 8 - not
        // the one after it.
        assert_eq!(next_instruction(&app, 5), 8, "from mid-instruction");
    }

    #[test]
    fn prev_instruction_walks_boundaries() {
        let app = app_with_code();
        for pair in BOUNDS.windows(2) {
            assert_eq!(
                prev_instruction(&app, pair[1]),
                pair[0],
                "back from 0x{:X}",
                pair[1]
            );
        }
        assert_eq!(prev_instruction(&app, 0), 0, "already at the start");
    }

    /// The property the old code broke: down-then-up returns to where it started.
    #[test]
    fn down_then_up_is_a_round_trip() {
        let app = app_with_code();
        for &start in BOUNDS {
            let down = next_instruction(&app, start);
            if down == start {
                continue;
            }
            assert_eq!(
                prev_instruction(&app, down),
                start,
                "0x{:X} -> 0x{:X} -> back",
                start,
                down
            );
        }
    }

    /// PageDown/PageUp must also round-trip, which the 3-bytes-per-instruction
    /// estimate could not.
    #[test]
    fn page_down_then_page_up_is_a_round_trip() {
        let app = app_with_code();
        let down = advance(&app, 0, 4);
        assert_eq!(down, BOUNDS[4], "four instructions from 0");
        assert_eq!(retreat(&app, down, 4), 0);
    }

    /// Every landing place must be a real boundary.
    #[test]
    fn advance_and_retreat_stay_on_boundaries() {
        let app = app_with_code();
        for count in 0..6 {
            let ofs = advance(&app, 0, count);
            assert!(
                BOUNDS.contains(&ofs) || ofs >= 0x40 - 1,
                "advance({}) landed at 0x{:X}, not a boundary",
                count,
                ofs
            );
        }
    }

    /// Navigation follows the bytes on screen, which include pending edits.
    #[test]
    fn edits_change_instruction_lengths() {
        let mut app = app_with_code();
        assert_eq!(instruction_len(&app, 0), Some(1), "nop is one byte");

        // Patch offset 0 into `mov eax, imm32`, which is five bytes.
        app.hex_view.changed_bytes.insert(0, "B8".to_string());
        assert_eq!(
            instruction_len(&app, 0),
            Some(5),
            "the edited instruction is what the view shows, so it is what Down must step over"
        );
        assert_eq!(next_instruction(&app, 0), 5);
    }

    /// Bounds: nothing may run past the mapping or underflow at zero.
    #[test]
    fn offsets_stay_in_range() {
        let app = app_with_code();
        let last = last_offset(&app);
        assert_eq!(advance(&app, last, 10), last);
        assert_eq!(retreat(&app, 0, 10), 0);
        assert!(next_instruction(&app, last) <= last);
        assert!(instruction_len(&app, last + 1).is_none());
    }

    /// An empty file must not panic or produce an offset.
    #[test]
    fn empty_file_is_safe() {
        let app = App::new();
        assert_eq!(next_instruction(&app, 0), 0);
        assert_eq!(prev_instruction(&app, 0), 0);
        assert_eq!(advance(&app, 0, 5), 0);
        assert_eq!(retreat(&app, 0, 5), 0);
        assert!(instruction_len(&app, 0).is_none());
    }

    /// `End` should show a full page, not a single row.
    #[test]
    fn page_start_ending_at_backs_up_a_page() {
        let app = app_with_code();
        let last = last_offset(&app);
        let start = page_start_ending_at(&app, last, 10);
        assert!(start < last, "the page must start before the cursor");
        assert_eq!(advance(&app, start, 9), last, "cursor lands on the last row");
    }
}
