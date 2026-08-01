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

---

## ⌨️ Shortcuts & Keybindings Reference

### GLOBAL (any view)
```text
  Tab / Shift+Tab      Switch view (Hex <-> Disasm; skips non-executables)
  F6                   Strings list
  F7                   Text view (press again to come back)
  F4                   Header view (press again to come back)
  F5                   String References dialog
  F8                   About / program info (paths, encodings, license)
  F9 / Ctrl+O          Open File dialog
  F12                  Save and quit (same as ':wq')
  Alt+F1               Select Drive dialog
  Alt+F2               Toggle Offset <-> VA address display
  Alt+F6               Set image base (blank = back to the file's own)
  Alt+F7               Cycle decoding width: auto -> 16 -> 32 -> 64
  ;                    Comment the byte under the cursor
  Esc                  Back / cancel / clear selection
  :                    Command line
  =                    Calculator (hex by default, 't' = decimal,
                        Ctrl+L clears, Up/Down history)
  Ctrl+G                Goto Address (hex or VA)
  Ctrl+X                Copy current VA to clipboard
  Ctrl+Left / '-'       Jump back to previous cursor position
  Ctrl+Right / '+'      Jump forward to next cursor position
  Alt+L                 Log window (y copies it to the clipboard)
  Ctrl+K                Modify Block dialog
  Ctrl+H                Wildcard Hex Pattern Replace dialog
  Ctrl+B                Find Pattern dialog (ANSI/UTF-8/UNICODE/Hex)
  F3 / Shift+F3          Repeat last pattern search forward / backward
  Ctrl+R                Cross References (Xref) search
```

### HEX VIEW - navigation
```text
  Arrow keys             Move cursor
  Home / Ctrl+Home       Start of line / start of file
  End / Ctrl+End          End of line / end of file
  PageUp / PageDown       Scroll one page
  Backspace               Go to last visited offset
  [ / ]                    Mark the block: start / end at the cursor
  Alt+[ / Alt+]            Jump to the block's ends, or the nearest
                            coloured block edge
```

### HEX VIEW - editing
```text
  F2                    Enter edit mode at cursor
  Tab                    Switch edit column: HEX -> enc1 -> enc2
  Shift + arrows         Select a block in the focused column
  Ctrl+C                 Copy that block (hex from the byte column,
                          decoded text from an encoding column)
  Ctrl+E                Edit Data dialog
  ~                     Toggle upper/lower case of byte under cursor
  Shift+V               Paste hex or text bytes from clipboard
  Alt+H                 Toggle highlight for byte under cursor
  Ctrl+Z / Alt+Backspace Undo last change (or reverted selection)
  Ctrl+Y                Redo last undone change
  Alt+F3                Revert only the byte under the cursor
```

### HEX VIEW - inside edit mode (F2)
```text
  0-9 a-f                 Type hex digits (two make a byte)
  Tab                     Switch column: HEX -> enc1 -> enc2
  Shift + arrows          Select a block in the focused column
  Ctrl+C                  Copy that block
  Ctrl+E                  Edit Data dialog (also works here)
  ~                       Toggle case and advance
  Esc / Enter             Leave edit mode
```

### HEX VIEW - selection
```text
  Shift + movement        Start / extend a selection (as in Disasm view)
  Esc                     Clear the selection
  Insert                  Fill selection with 0x00 (no selection: 1 byte)
  Delete                  Fill selection with 0x90 NOPs (or just 1 byte)
  ~                       Toggle case of the selection (or 1 byte)
  Ctrl+C                  Copy the selection to the clipboard. What is
                           copied follows the column the block was
                           selected in: hex bytes, or the text those
                           bytes spell in enc1 / enc2
  Ctrl+Z                  Revert changed bytes in the selection
  Ctrl+K                  Modify Block dialog
  Alt+M                   Colorize block (new or existing)
  Mouse drag              Selects too; Enter keeps the block, Esc clears it
```

### HEX VIEW - search & lists
```text
  Ctrl+B                Open Find Pattern dialog (text or hex, Tab/Up/Down
                         to switch field, Enter searches the focused one)
  F3 / Shift+F3          Repeat last pattern search forward / backward
  Alt+N                 Names dialog
  F6                    Strings list
  Alt+E / Alt+Shift+E   Change primary / secondary text encoding
```

### NAMES DIALOG (Alt+N) - the comments in this file
```text
  Up / Down               Move through the list
  PageUp / PageDown       Scroll
  Enter                   Go to that offset
  F2                      Edit that comment
  Delete                  Delete that comment (stays in the list)
  f                       Filter by regex
  o / n                   Sort by offset / by comment text
  Esc                     Close
```

### DISASSEMBLY VIEW
```text
  Up / Down             Previous / next instruction
  PageUp / PageDown     Scroll one page of instructions
  Home / End            First / last instruction
  Shift + movement       Extend selection
  Enter                   Follow branch or memory target
  Ctrl+Enter              Follow target and switch to Hex view
  Space                  Assemble instruction at cursor
                          (numbers are hex; add 't' for decimal, e.g.
                           'push 10' = 0x10, 'push 10t' = 10)
  Ctrl+C                 Copy selected instructions to clipboard
  Ctrl+E                 Edit Data dialog
  Ctrl+R                 Cross References (Xref) search
  F6                      Strings list (addresses shown as VA here)
  Delete                  Fill the instruction under the cursor with NOPs
                           (uses its exact decoded length)
  Ctrl+Z / Alt+Backspace  Undo last change
  Ctrl+Y                  Redo last undone change
  Alt+F3                  Revert only the byte under the cursor
```

### TEXT VIEW
```text
  Up / Down              Scroll a line, then move the window through the file
  Left / Right           Scroll sideways
  Home / Ctrl+Home       Start of line / start of file
  Ctrl+End                End of last visible line
  PageUp / PageDown       Scroll one page
  Alt+E                   Change text encoding
```

### HEADER VIEW
```text
  Left / Right          Switch pane, move column
  Up / Down             Move selection
  PageUp / PageDown     Move a screenful
  Home / End            First / last entry
  Tab                   Switch sidebar <-> detail pane
  Enter                 Edit the selected field
  g / f                 Jump to that field's offset in Hex view
  Esc / q               Leave Header view
```

#### HEADER VIEW - Section Tools (PE only, sidebar category 7)
```text
  Align Offset to VA    Set PointerToRawData = VirtualAddress
  Add New Section       Append a section of a given size (default 0x1000)
  Note                  Edits stay in memory; ':w' writes them to disk
```

### COMMAND LINE (':' to open)
```text
  :q                      Quit
  :about  /  :ver         Program info (same as F8; 'y' copies it)
  :w [file]               Save (to file, if given)
  :wq  /  :x [file]       Save and quit
  :wb <file>  /  :wblock <file>   Save selected block to file
  :o [file]  /  :open [file]      Open file (blank = Open dialog)
  :cmt <offset> <text>    Add a comment at offset
  :<address>              Goto address (hex; 't' suffix = decimal;
                           '+'/'-' prefix = relative; 'cur'/'base'/'oep'
                           keywords; supports + and - expressions)
  :set                    Show every option and its current value
  :set byteline <n|auto>  Bytes shown per line (alias: width)
  :set ctrlchar <c>       Non-graphic byte placeholder character
  :set enc1 <name>        Primary encoding (utf-8, cp949, cp936,
                           iso-8859-1, iso-8859-2, utf-16le, utf-16be)
  :set enc2 <name|none>   Secondary encoding, same names plus 'none'
  :set lang en|ko|zh      Interface language (English, 한국어, 中文).
                           Labels only: key names, option names and the
                           status-bar modes stay as they are
  :set theme <name>       Load a hex-view color theme. Disassembly
                           colors are left alone unless the theme file
                           declares them; use ':set disasmtheme' for those
  :set disasmtheme <name>  Disassembly colors only: dark, light, grey,
                           another theme name, or a path to a file
  :set addr va|offset|toggle    Address column contents
                           (':set va' and ':set offset' still work)
  :set bitness <16|32|64|auto>  Force the disassembly decoding width
  :set view hex|disasm|text|header   Switch view

  Every on/off option below takes 'on', 'off' or 'toggle'; with no
  value it turns on. The old 'no<name>' spellings still work.
  :set highlight          Disassembly syntax colors (alias: hilight)
  :set hintbar            Bottom hint line (hold Ctrl or Alt while it
                           is showing to see those bindings)
  :set wrapscan           Wrap search around EOF
  :set db                 Write the .dzdb annotation sidecar file
  :set dimctrl            Dim control bytes
  :set dimzero            Dim null bytes (independent of dimctrl)
  :set disasm_mem/reg/imm/kw/seg/import/import_fg/comment <color>
                          Disassembly colors, #rrggbb or a name
```
