//! The release's install: the game's code recompiled from your ROM into a
//! library that the Zakuro beside the launcher runs, with this project's
//! patches, overrides and cheats, which come inside the launcher. It needs
//! neither Rust, git nor this project's source: what `cargo build-game`
//! does, with a library for Zakuro to load in place of a program of its own.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use eframe::egui;
use recomp3ds::build::{self, Event};
use recomp3ds::compile::{self, Compiler};
use smt4_patches::TITLE_ID;

use crate::{data_dir, zig, Job};

/// the job's name, which the window gives the install's steps by
pub const WHAT: &str = "Installing the game";

/// the install's steps, by the number its log gives them
pub const STEPS: [&str; 8] = ["", "compiler", "", "the game's code as C", "compiling", "", "installing", "cheats"];

/// the Zakuro a release brings beside the launcher, in Contents/MacOS in the
/// macOS app, none in a checkout of the project
pub fn zakuro() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let zakuro = exe.with_file_name(format!("zakuro{}", std::env::consts::EXE_SUFFIX));
    zakuro.is_file().then_some(zakuro)
}

/// where the library goes: the launcher's own folder, which Zakuro is told
/// of, out of the way of its own Recompile, which leaves this project's
/// changes out
pub fn dir() -> PathBuf {
    data_dir().join("recompiled")
}

fn library() -> PathBuf {
    dir().join(format!("{TITLE_ID:016X}.{}", std::env::consts::DLL_EXTENSION))
}

fn stamp_file() -> PathBuf {
    dir().join("installed.txt")
}

/// what the library was made from: this launcher, with its patches and
/// overrides, and the ROM
fn stamp(rom: &Path) -> String {
    let size = rom.metadata().map_or(0, |metadata| metadata.len());
    format!("launcher {}\nrom {size}\n", env!("CARGO_PKG_VERSION"))
}

/// whether the game is installed from this ROM by this launcher
pub fn installed(rom: &Path) -> bool {
    library().is_file() && std::fs::read_to_string(stamp_file()).is_ok_and(|text| text == stamp(rom))
}

/// Zakuro's data folder, where it reads the cheats and keeps its tools
pub fn zakuro_dir() -> PathBuf {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = || env("HOME").unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) {
        env("APPDATA").unwrap_or_else(home).join("zakuro")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support").join("zakuro")
    } else {
        env("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local").join("share")).join("zakuro")
    }
}

/// the computer's C compiler, none on a Mac without Apple's command line
/// tools, where trying cc would ask to install them
fn system_compiler() -> Option<Compiler> {
    if cfg!(target_os = "macos") {
        let tools = Command::new("/usr/bin/xcode-select").arg("-p").output();
        if !tools.is_ok_and(|output| output.status.success()) {
            return None;
        }
    }
    compile::check(None).ok()
}

/// the compiler the install will use, for the window: the computer's, the
/// Zig downloaded before, or the Zig it will download; none when it has
/// none and can't download one
pub fn compiler_note() -> Option<String> {
    if let Some(compiler) = system_compiler() {
        return Some(compiler.describe());
    }
    if let Some(zig) = zig::installed(&zig::tools_dir(&zakuro_dir())) {
        return Some(zig.describe());
    }
    let size = zig::download_size().filter(|_| zig::can_download())?;
    Some(format!("Zig, downloaded once when installing ({} MB)", size / 1_000_000))
}

/// installs the game from rom on a thread of its own, its steps and how
/// far along it is in job
pub fn start(job: Arc<Mutex<Job>>, ctx: egui::Context, rom: PathBuf) {
    *job.lock().unwrap() = Job { what: WHAT.to_owned(), running: true, ..Default::default() };
    std::thread::spawn(move || {
        let say = |line: String| {
            job.lock().unwrap().take_line(line);
            ctx.request_repaint();
        };
        let done = run(&rom, &say);
        if let Err(error) = &done {
            say(format!("error: {error}"));
        }
        let mut j = job.lock().unwrap();
        j.running = false;
        j.result = Some(done.is_ok());
        j.log.push(if done.is_ok() { "done".into() } else { "failed, see above".into() });
        ctx.request_repaint();
    });
}

/// installs the game from rom, each step said as it starts, in the
/// builder's words, which the window's progress reads
pub fn run(rom: &Path, say: &(dyn Fn(String) + Sync)) -> Result<(), String> {
    // the compilers' leftovers, such as zig cc's a.o when asked its
    // version, go in the launcher's folder, not wherever it was opened from
    let rom = &std::path::absolute(rom).map_err(|e| format!("could not find {}: {e}", rom.display()))?;
    let _ = std::fs::create_dir_all(data_dir()).and_then(|()| std::env::set_current_dir(data_dir()));
    say("== 1. compiler".into());
    let compiler = match system_compiler() {
        Some(compiler) => compiler,
        None => {
            let tools = zig::tools_dir(&zakuro_dir());
            match zig::installed(&tools) {
                Some(zig) => zig,
                None => {
                    say("no C compiler here, downloading Zig's".into());
                    let tenth = AtomicBool::new(false);
                    let last = std::sync::atomic::AtomicU32::new(0);
                    let progress = |done: f32| {
                        let percent = (done * 100.0) as u32 / 10 * 10;
                        if percent > last.swap(percent, std::sync::atomic::Ordering::Relaxed) {
                            say(format!("downloading Zig, {percent}%"));
                        }
                    };
                    zig::download(&tools, &progress, &tenth)?
                }
            }
        }
    };
    say(format!("C compiler: {}", compiler.describe()));

    say("== 3. the game's code as C".into());
    let work = data_dir().join("work");
    let _ = std::fs::remove_dir_all(&work);
    let overrides = work.join("overrides");
    smt4_patches::write_overrides(&overrides).map_err(|e| format!("could not write the overrides: {e}"))?;
    let events = |event: Event| match event {
        Event::Generated { files, bytes, overrides } => {
            say(format!("wrote {files} files, {} MiB of C, {overrides} overrides", bytes >> 20))
        }
        Event::Note(note) => say(note),
        _ => {}
    };
    let generated = build::generate(rom, |_| work.join("code"), Some(&overrides), Default::default(), &events)?;
    if generated.title.program_id() != TITLE_ID {
        return Err(format!("the ROM is title {:016X}, not Shin Megami Tensei IV (USA)", generated.title.program_id()));
    }
    let patched = smt4_patches::patch_dir(&generated.dir)?;
    for note in patched.notes {
        say(note);
    }

    say("== 4. compiling".into());
    say("this takes a few minutes or more, and a few GB of memory".into());
    let made = generated.dir.join(format!("game.{}", std::env::consts::DLL_EXTENSION));
    let progress = |done: usize, total: usize| {
        say(format!("compiled {done} of {total}"));
        true
    };
    compile::compile(&compiler, &generated.dir, &generated.sources, &made, &progress, true)?;

    say("== 6. installing".into());
    let target = library();
    std::fs::create_dir_all(dir()).map_err(|e| format!("could not make {}: {e}", dir().display()))?;
    let partial = target.with_extension(format!("{}.new", std::env::consts::DLL_EXTENSION));
    std::fs::copy(&made, &partial).map_err(|e| format!("could not copy the library: {e}"))?;
    // Windows keeps a library open while the game runs
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| format!("could not replace {}, is the game open? {e}", target.display()))?;
    }
    std::fs::rename(&partial, &target).map_err(|e| format!("could not install {}: {e}", target.display()))?;
    std::fs::write(stamp_file(), stamp(rom)).map_err(|e| format!("could not write {}: {e}", stamp_file().display()))?;
    say(format!("installed {}", target.display()));
    let _ = std::fs::remove_dir_all(&work);

    say("== 7. cheats".into());
    say(smt4_patches::install_cheats(&zakuro_dir().join("cheats").join(format!("{TITLE_ID:016X}.txt")))?);
    Ok(())
}

/// the Zakuro command that plays the installed game
pub fn play(zakuro: &Path, rom: &Path) -> Command {
    let mut command = Command::new(zakuro);
    command.arg("--recompiled").arg(dir()).arg(rom);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the library is the one Zakuro looks for in the folder it's given,
    /// named after the title
    #[test]
    fn the_library_is_named_as_zakuro_looks_for_it() {
        let name = library().file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("00040000000E5C00."), "{name}");
        assert!(name.ends_with(std::env::consts::DLL_EXTENSION));
    }

    /// a ROM of another size, or another launcher, means installing again
    #[test]
    fn the_stamp_names_the_launcher_and_the_rom() {
        let rom = std::env::temp_dir().join(format!("smt4-stamp {}", std::process::id()));
        std::fs::write(&rom, b"abc").unwrap();
        let stamp = stamp(&rom);
        assert!(stamp.contains(env!("CARGO_PKG_VERSION")) && stamp.contains("rom 3"), "{stamp}");
        std::fs::remove_file(rom).unwrap();
    }

    /// a checkout has no Zakuro beside the launcher's test program
    #[test]
    fn a_checkout_is_no_release() {
        assert!(zakuro().is_none());
    }
}
