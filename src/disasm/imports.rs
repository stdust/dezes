//! Resolves addresses to imported function names for the disassembly listing.
//!
//! A call through the import table reads as `call qword ptr [0x140002138]` on its
//! own, which says nothing about what is being called. The PE import directory
//! already tells us that this slot holds `CreateFileW`, and that information was
//! parsed when the file was opened - it just never reached the disassembler.
//!
//! Names are resolved by *address* rather than by pattern-matching the formatted
//! text, so both the 64-bit form (`[rip+disp]`, whose target the decoder computes)
//! and the 32-bit form (`[0x00402004]`, an absolute displacement) land on the same
//! lookup.

use std::collections::HashMap;

use crate::header::header_view::Pe;

/// `KERNEL32.dll` + `CreateFileW` -> `KERNEL32.CreateFileW`.
///
/// The `.dll` suffix is dropped because it is noise repeated on every line, and
/// the comment column is the narrowest part of the layout. An import with no name
/// (bound by ordinal) is labelled with its ordinal instead, which is all the file
/// itself records.
fn format_label(dll: &str, name: &str, ordinal: u16) -> String {
    let module = dll
        .rsplit_once('.')
        .map(|(stem, ext)| {
            if ext.eq_ignore_ascii_case("dll") {
                stem
            } else {
                dll
            }
        })
        .unwrap_or(dll);

    if name.is_empty() {
        format!("{}.#{}", module, ordinal)
    } else {
        format!("{}.{}", module, name)
    }
}

/// Maps the virtual address of each import-address-table slot to a display label.
///
/// Keyed on the slot's own address - the pointer the code reads *through* - not on
/// the address of the function, which is only known at run time.
///
/// The address comes from goblin's `Import::offset`, which despite the name is the
/// RVA of the IAT entry. `Import::rva` is the RVA of the hint/name structure in the
/// import directory, which no instruction ever references: keying on that produced
/// a map that matched nothing at all.
pub fn build_labels(pe: &Pe, image_base: u64) -> HashMap<u64, String> {
    let mut labels = HashMap::with_capacity(pe.imports.len());

    for import in &pe.imports {
        // A zero slot RVA carries no address to key on; keeping it would collide
        // on `image_base` itself and mislabel whatever lives there.
        if import.offset == 0 {
            continue;
        }
        let va = image_base.wrapping_add(import.offset as u64);
        labels.insert(va, format_label(&import.dll, &import.name, import._ordinal));
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dll_suffix_is_dropped_but_other_extensions_are_kept() {
        assert_eq!(
            format_label("KERNEL32.dll", "CreateFileW", 0),
            "KERNEL32.CreateFileW"
        );
        // Case-insensitive, since the import directory's spelling varies.
        assert_eq!(format_label("user32.DLL", "MessageBoxA", 0), "user32.MessageBoxA");
        // `.drv`, `.sys` and the like are part of the module name, not noise.
        assert_eq!(format_label("winspool.drv", "OpenPrinterW", 0), "winspool.drv.OpenPrinterW");
        // No dot at all.
        assert_eq!(format_label("MYMOD", "Fn", 0), "MYMOD.Fn");
    }

    /// Imports bound by ordinal have no name; the ordinal is all there is to show.
    #[test]
    fn a_nameless_import_falls_back_to_its_ordinal() {
        assert_eq!(format_label("WS2_32.dll", "", 115), "WS2_32.#115");
    }

    /// The real test binary's own imports must be keyed at `image_base + rva`,
    /// which is the address its call sites reference.
    #[test]
    fn labels_are_keyed_on_the_slot_address() {
        let mut app = crate::app::App::new();
        app.config.database = false;
        let Ok(exe) = std::env::current_exe() else { return };
        let Some(exe) = exe.to_str() else { return };
        if app.load_file(exe, 0, true).is_err() {
            return;
        }
        let Some(pe) = app.header_view.pe.as_ref() else { return };
        if pe.imports.is_empty() {
            return;
        }

        let base = app.get_image_base();
        let labels = build_labels(pe, base);
        assert!(!labels.is_empty(), "a PE with imports must produce labels");

        let import = pe
            .imports
            .iter()
            .find(|i| i.offset != 0)
            .expect("an import with a slot address");
        let va = base + import.offset as u64;
        assert!(
            labels.contains_key(&va),
            "import {} is not keyed at image_base + slot rva",
            import.name
        );
    }
}

