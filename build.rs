//! Embeds the Windows icon and version metadata into the executable.
//!
//! rustc does not emit a `.rsrc` section on its own - no native Windows compiler
//! does - so the icon Explorer shows, and the fields on the file's Details tab,
//! have to be compiled into a resource and handed to the linker. `winresource`
//! does that, reading the version and description straight from `Cargo.toml`.
//!
//! Failure is a warning, not an error. Compiling a resource needs `rc.exe` from
//! the Windows SDK (or `windres` on a GNU toolchain), and a machine without one
//! should still be able to build a working dz6 - just without the icon.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/dezes.ico");
        println!("cargo:rerun-if-changed=build.rs");

        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/dezes.ico");
        res.set(
            "FileDescription",
            "Dezes - terminal hex editor and disassembler",
        );
        res.set("ProductName", "Dezes");
        res.set("OriginalFilename", "dezes.exe");
        res.set("LegalCopyright", "GPL-3.0-or-later");

        if let Err(e) = res.compile() {
            println!("cargo:warning=icon/version resource not embedded: {}", e);
        }
    }
}
