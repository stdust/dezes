# Dezes

**Dezes** is an advanced, high-performance TUI (Terminal User Interface) **Win32 PE Disassembler, Hex Editor, and Software Localization Tool** built in Rust.

It is designed for reverse engineers, binary analysts, and software translators to disassemble x86/x64 executables, edit binary structures, and translate program strings smoothly across multiple encodings.

---

## ✨ Key Features

- 🔍 **PE Binary Disassembler & Analyzer**
  - Full support for PE32 (32-bit) and PE64 (64-bit) Windows executables.
  - Interactive Assembly editing (`F3`), Cross-References / Xref (`Ctrl+R`), and String References (`F5`).

- 🌐 **Software Localization & Translation Suite**
  - **String Scanning (`F6`)**: Multi-encoding string scanning supporting **ASCII**, **UTF-8**, **CP949 (Korean 2-byte ANSI)**, **CP936 (Simplified Chinese)**, and **UTF-16LE**.
  - **In-place String Edit (`F4`)**: Direct string replacement with live encoding budget checks. Defaults to CP949 for immediate Korean translation without text corruption. Cycle target encodings on the fly via `Alt+E`.
  - **Seamless Navigation**: Switch between String Reference (`F5`) and String Scan (`F6`) while preserving search query, filter state, and matching cursor offsets.

- 🛠️ **Full-Featured Hex Editor**
  - Instant navigation, block selection, multi-row copying (`Ctrl+C` for single line, `Ctrl+Shift+C` for all rows), undo/redo history, and built-in hexadecimal calculator.

---

## 🚀 Building & Running

### Prerequisites
- [Rust](https://www.rust-lang.org/) (cargo / rustc)

### Build Release Binary
```bash
cargo build --release
```
The optimized executable will be generated at: `target/release/dezes.exe`

---

## ⌨️ Shortcuts & Keybindings

| Key | Description |
|---|---|
| `F6` | Open String Scan Dialog |
| `F5` | Open String Reference Dialog (Seamless switch with `F6`) |
| `F4` | Edit / Replace Selected String (Inside String Scan) |
| `Alt+E` | Cycle Target Encoding inside String Edit (`CP949` ➔ `CP936` ➔ `UTF-16LE` ➔ `ASCII` ➔ `UTF-8`) |
| `Ctrl+C` | Copy selected single row / line |
| `Ctrl+Shift+C` | Copy all filtered rows / lines |
| `Ctrl+R` | Open Cross-References (Xref) Dialog |
| `F8` | Program Info (About Dezes) |
| `F1` | Help Dialog |

---

## 📄 License

Distributed under the terms of the GNU General Public License (GPL).
