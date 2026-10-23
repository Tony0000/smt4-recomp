//! Patches 3dsrecomp's generated C at sites that aren't function entries, so a
//! RECOMP_OVERRIDE can't reach them. Step 5 of the build runs this after
//! every `3dsrecomp port`; on its own, from the project root:
//!
//!     cargo build-game --patch-only
//!
//! Each patch is anchored on the instruction comment 3dsrecomp writes before
//! the code for each ARM instruction ("/* ADDRESS OPCODE */"), so it fails
//! loudly instead of patching the wrong place if a new port moves things
//! around. Re-running is a no-op.
//!
//! Then the patched files are recompiled with the flags 3dsrecomp uses and put
//! into code/librecomp.a, so `cargo build` picks them up. CC and AR pick the
//! compiler and archiver (default gcc and ar, MinGW-w64's on Windows, Apple's
//! clang and ar on macOS).

use std::path::{Path, PathBuf};

use crate::{fail, run, sorted_files};

/// the flags 3dsrecomp compiles generated code with (src/compile.rs)
const CFLAGS: [&str; 4] = ["-O2", "-ffp-contract=off", "-fno-math-errno", "-w"];

/// applies every patch (the patches crate, which the launcher shares);
/// returns the files the patches are in
fn patch_all(code: &Path) -> Vec<PathBuf> {
    let patched = smt4_patches::patch_dir(code).unwrap_or_else(|error| fail(&error));
    for note in &patched.notes {
        println!("{note}");
    }
    patched.files
}

/// recompiles the patched files and the overrides into code/librecomp.a
fn rebuild(root: &Path, code: &Path, files: Vec<PathBuf>, cc: &str, ar: &str) {
    // overrides/*.c are recompiled too, so editing an override's body doesn't
    // need a new port. An override at a *new* address still does: port is
    // what points the callers and the entry table at it.
    let mut sources = files;
    sources.extend(sorted_files(&root.join("overrides"), |name| name.ends_with(".c")));
    let mut objects = Vec::new();
    for path in &sources {
        let obj = code.join(format!("{}.o", path.file_stem().unwrap().to_string_lossy()));
        println!("compiling {}", path.file_name().unwrap().to_string_lossy());
        let mut args: Vec<std::ffi::OsString> = CFLAGS.iter().map(Into::into).collect();
        args.extend(["-I".into(), code.into(), "-c".into(), path.into(), "-o".into(), obj.clone().into()]);
        run(cc, &args, root);
        objects.push(obj);
    }
    // archive members are named after the file, so this replaces the objects
    // 3dsrecomp put there, and s writes the index again, without which
    // Apple's linker turns the archive down
    let mut args: Vec<std::ffi::OsString> = vec!["rs".into(), code.join("librecomp.a").into()];
    args.extend(objects.iter().map(Into::into));
    run(ar, &args, root);
    for obj in &objects {
        let _ = std::fs::remove_file(obj);
    }
    println!("updated librecomp.a with {} object(s)", objects.len());
}

pub fn patch(root: &Path, cc: &str, ar: &str) {
    let code = root.join("code");
    let files = patch_all(&code);
    rebuild(root, &code, files, cc, ar);
}
