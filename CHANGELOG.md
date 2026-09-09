# Changelog - Dezes

All notable changes to the **Dezes** project are documented in this file.

## [0.8.2] - 2026-09-10

### ✨ New Features & Enhancements
- **Dialog Input History (`Ctrl+Up` / `Ctrl+Down`)**: Added input history navigation to Edit (`Ctrl+E`), Find (`Ctrl+B`), and Goto (`Ctrl+G`) dialogs, allowing rapid recall of previous entries without conflict with tab switching.
- **Escape Sequence Hex Input (`Ctrl+E`)**: Supported literal escape sequences (such as `\n`, `\r`, `\t`, `\\`) in text string edit mode to seamlessly insert control bytes.
- **PE Section Tools Localization & Expanded UI**: Added full multilingual translation (English, Korean, Simplified Chinese) and expanded the PE Section Tool dialog width to 90% for clear inspection of section characteristics.
- **Wider Strings Viewer**: Expanded the Strings Dialog viewport to 75% screen width, eliminating clipping on long string references.
- **Disassembly Theme Refinements**: Updated return instruction highlighting and theme rendering.

## [0.8.1] - 2026-08-08

### ✨ New Features & Enhancements
- **Mutual Ctrl+B <-> Ctrl+H Quick Navigation**: Seamlessly switch between Find (`Ctrl+B`) and Replace (`Ctrl+H`) dialogs while preserving search queries and encoding states (`src/hex/find_dialog.rs`, `src/events.rs`).
- **Unified Text Field Engine (`src/text_field.rs`)**: Standardized Shift+Arrows selection, Home/End, block overwrite, and Ctrl+C/V/X clipboard handling across all dialog input fields.
- **Enhanced DBCS & UTF-16 String Filtering (`src/hex/strings.rs`)**: Added strict NUL-terminator and DBCS false-positive filtering rules for Korean/Chinese text scans.
- **Standardized Project Structure**: Cleaned up temporary version files and reorganized source files into standard Cargo (`src/`) directory tree.

### 🐛 Bug Fixes & Stability Improvements
- **Mouse Event Isolation Fix (`src/events.rs`)**: Resolved an issue where scrolling the mouse wheel while a modal dialog was open leaked events to background Hex/Disasm views, causing UI state corruption.
- **UTF-16LE Search Pattern Fix (`src/hex/find_dialog.rs`)**: Fixed a bug where UTF-16LE searches converted queries to UTF-8 instead of matching wide-character bytes.
- **Terminal Resize Safety (`src/events.rs`)**: Prevented arithmetic underflow crash when narrowing terminal window width (`bytes_per_line` calculation).
- **Read-Only File Protection (`src/app.rs`, `src/events.rs`)**: Enforced upfront edit refusal popups when attempting modifications on read-only files.
