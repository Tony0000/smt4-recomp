//! What smt4-recomp changes in Shin Megami Tensei IV's recompiled code, for
//! the builder (tools/build, the program with the code linked in) and the
//! launcher (a library Zakuro loads) alike:
//!
//! - the patches to 3dsrecomp's generated C, at sites that aren't function
//!   entries, so a RECOMP_OVERRIDE can't reach them;
//! - the overrides, overrides/*.c, carried in here so the launcher needs
//!   no copy of the project;
//! - the cheats file, and putting ours in Zakuro's without touching the
//!   rest.
//!
//! Each patch is anchored on the instruction comment 3dsrecomp writes before
//! the code for each ARM instruction ("/* ADDRESS OPCODE */"), so it fails
//! loudly instead of patching the wrong place if a new port moves things
//! around. Re-running is a no-op.

use std::path::{Path, PathBuf};

pub mod icon;

/// Shin Megami Tensei IV (USA)
pub const TITLE_ID: u64 = 0x0004_0000_000E_5C00;

/// overrides/*.c, by file name
pub const OVERRIDES: [(&str, &str); 3] = [
    ("multipliers.c", include_str!("../../overrides/multipliers.c")),
    ("recruit.c", include_str!("../../overrides/recruit.c")),
    ("widescreen.c", include_str!("../../overrides/widescreen.c")),
];

/// the cheats for Zakuro's cheat menu, cheats/00040000000E5C00.txt
pub const CHEATS: &str = include_str!("../../cheats/00040000000E5C00.txt");

/// writes the overrides into dir, for 3dsrecomp's --overrides
pub fn write_overrides(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in OVERRIDES {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(())
}

const DECLS: [&str; 4] = [
    "extern uint32_t smt4_boost_exp(Context *ctx, uint32_t gain);\n",
    "extern uint32_t smt4_boost_demon_exp(Context *ctx, uint32_t address);\n",
    "extern uint32_t smt4_widen_aspect(Context *ctx, uint32_t bits);\n",
    "extern uint32_t smt4_talk_debug(Context *ctx, uint32_t index, uint32_t value);\n",
];

pub struct Patch {
    pub desc: &'static str,
    /// the instruction comment it's anchored on
    pub anchor: &'static str,
    /// the text right after it
    pub old: &'static str,
    pub new: &'static str,
}

pub const PATCHES: [Patch; 5] = [
    Patch {
        desc: "hero EXP from battle results: hero.exp(r1) + gain(r0) at 0x0018CF84",
        anchor: "/* 0018CF84 E0817000 */",
        old: "    uint32_t m = reg0_;",
        new: "    uint32_t m = smt4_boost_exp(ctx, reg0_); /* smt4-recomp: EXP multiplier */",
    },
    // the gain is boosted where it's loaded, and written back, because
    // 0x0018D230 later re-reads it through a pointer to undo the add
    Patch {
        desc: "demon EXP from battle results: load of results[0x35B8 + id*4] at 0x0018D190",
        anchor: "/* 0018D190 E5900000 */",
        old: "    uint32_t v = mem_read32(ctx, oa);",
        new: "    uint32_t v = smt4_boost_demon_exp(ctx, oa); /* smt4-recomp: EXP multiplier */",
    },
    Patch {
        desc: "widescreen: aspect (s1) on entry to the perspective builder at 0x002B761C",
        anchor: "/* 002B761C E92D4010 */",
        old: "    { uint32_t base = reg13_, p = base - 8, fin = base - 8;",
        new: "    vfp1_ = smt4_widen_aspect(ctx, vfp1_); /* smt4-recomp: widescreen */\n    { uint32_t base = reg13_, p = base - 8, fin = base - 8;",
    },
    Patch {
        desc: "widescreen: aspect (s3) on entry to the camera's SetPerspective at 0x002C1FFC",
        anchor: "/* 002C1FFC E92D4070 */",
        old: "    { uint32_t base = reg13_, p = base - 16, fin = base - 16;",
        new: "    vfp3_ = smt4_widen_aspect(ctx, vfp3_); /* smt4-recomp: widescreen */\n    { uint32_t base = reg13_, p = base - 16, fin = base - 16;",
    },
    // r4 is the talk context, oa = r4 + 0x22A8 + index * 4
    Patch {
        desc: "easy recruit: talk debug setting read by script native 0x0259 at 0x002F1298",
        anchor: "/* 002F1298 E59002A8 */",
        old: "    uint32_t v = mem_read32(ctx, oa);",
        new: "    uint32_t v = smt4_talk_debug(ctx, (oa - reg4_ - 0x22A8u) >> 2, mem_read32(ctx, oa)); /* smt4-recomp: easy recruit */",
    },
];

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    let mut n = 0;
    let mut at = 0;
    while let Some(i) = find(haystack, needle, at) {
        n += 1;
        at = i + needle.len();
    }
    n
}

/// what patching a file's text did
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Patched,
    Already,
}

/// applies one patch to a file's text, at every place its anchor is: an
/// instruction 3dsrecomp decodes as part of more than one function, once
/// Zakuro's hints split one, is in each of them
pub fn apply(text: &mut Vec<u8>, patch: &Patch) -> Result<Outcome, String> {
    let anchor = patch.anchor.as_bytes();
    let marker = patch.new.split("/*").next().unwrap_or("").trim().as_bytes();
    let mut next = find(text, anchor, 0).ok_or_else(|| format!("{}: {} is gone", patch.desc, patch.anchor))?;
    let mut patched = false;
    loop {
        let at = next;
        // up to the next instruction comment
        let window_end = find(text, b"/*", at + anchor.len())
            .ok_or_else(|| format!("{}: nothing after {}", patch.desc, patch.anchor))?;
        let near = &text[at..(at + anchor.len() + 400).min(text.len())];
        if find(near, marker, 0).is_none() {
            let window = &text[at..window_end];
            if count(window, patch.old.as_bytes()) != 1 {
                return Err(format!("{}: expected exactly one {:?} after {}", patch.desc, patch.old, patch.anchor));
            }
            let old_at = at + find(window, patch.old.as_bytes(), 0).unwrap();
            text.splice(old_at..old_at + patch.old.len(), patch.new.bytes());
            patched = true;
        }
        match find(text, anchor, at + anchor.len()) {
            Some(at) => next = at,
            None => break,
        }
    }
    if !patched {
        return Ok(Outcome::Already);
    }
    let missing: String = DECLS.iter().filter(|d| find(text, d.as_bytes(), 0).is_none()).copied().collect();
    if !missing.is_empty() {
        // before the first generated function
        let first = find(text, b"\nvoid f_", 0).ok_or("no generated function to put the declarations before")? + 1;
        text.splice(first..first, missing.bytes());
    }
    Ok(Outcome::Patched)
}

/// what patching a folder of generated C did: the files any patch is in,
/// and a line for each patch in each file
pub struct Patched {
    pub files: Vec<PathBuf>,
    pub notes: Vec<String>,
}

/// applies every patch to the generated C in dir, code000.c on, each to
/// every file its anchor is in, and writes the files it changed
pub fn patch_dir(dir: &Path) -> Result<Patched, String> {
    let mut sources: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("could not read {}: {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("code") && n.ends_with(".c")))
        .collect();
    sources.sort();
    let mut texts: Vec<(PathBuf, Vec<u8>, bool)> = Vec::new();
    for path in sources {
        let text = std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        if PATCHES.iter().any(|patch| find(&text, patch.anchor.as_bytes(), 0).is_some()) {
            texts.push((path, text, false));
        }
    }
    let mut notes = Vec::new();
    for patch in &PATCHES {
        let mut found = false;
        for (path, text, changed) in texts.iter_mut().filter(|(_, text, _)| find(text, patch.anchor.as_bytes(), 0).is_some()) {
            found = true;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            match apply(text, patch)? {
                Outcome::Already => notes.push(format!("already patched: {} ({name})", patch.desc)),
                Outcome::Patched => {
                    notes.push(format!("patched: {} ({name})", patch.desc));
                    *changed = true;
                }
            }
        }
        if !found {
            return Err(format!("{}: {} is in no generated file", patch.desc, patch.anchor));
        }
    }
    for (path, text, changed) in &texts {
        if *changed {
            std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
    }
    Ok(Patched { files: texts.into_iter().map(|(path, ..)| path).collect(), notes })
}

/// the cheats in a cheats file, by name in the order they come, each with
/// its lines
pub fn cheat_blocks(text: &str) -> Vec<(String, Vec<String>)> {
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if line.starts_with('[') && line.trim_end().ends_with(']') {
            let name = line.trim().to_owned();
            // a name seen twice keeps its place and the last lines
            current = Some(match blocks.iter().position(|(n, _)| *n == name) {
                Some(i) => {
                    blocks[i].1 = vec![line.to_owned()];
                    i
                }
                None => {
                    blocks.push((name, vec![line.to_owned()]));
                    blocks.len() - 1
                }
            });
        } else if let Some(i) = current {
            blocks[i].1.push(line.to_owned());
        }
    }
    blocks
}

/// the cheats file with the cheats it lacks from ours added at the end, and
/// how many; Zakuro keeps which cheats are on in this same file, so the ones
/// already there are left as they are
pub fn merge_cheats(theirs: &str, ours: &str) -> (String, usize) {
    let have = cheat_blocks(theirs);
    let missing: Vec<Vec<String>> = cheat_blocks(ours)
        .into_iter()
        .filter(|(name, _)| !have.iter().any(|(n, _)| n == name))
        .map(|(_, lines)| lines)
        .collect();
    let newline = if theirs.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = theirs.lines().map(str::to_owned).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    for block in &missing {
        let end = block.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
        lines.push(String::new());
        lines.extend_from_slice(&block[..end]);
    }
    (lines.join(newline) + newline, missing.len())
}

/// puts our cheats in the cheats file at path, adding only the ones it
/// lacks, and says what it did
pub fn install_cheats(path: &Path) -> Result<String, String> {
    let count = cheat_blocks(CHEATS).len();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    }
    if !path.is_file() {
        std::fs::write(path, CHEATS).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        return Ok(format!("copied {count} cheats to {}", path.display()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (merged, added) = merge_cheats(&text, CHEATS);
    if added == 0 {
        return Ok(format!("{} already has all {count} cheats", path.display()));
    }
    std::fs::write(path, merged).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    let kept = cheat_blocks(&text).len();
    Ok(format!("added {added} cheats to {}, leaving the {kept} already there as they were", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = "#include \"recomp.h\"\n\nvoid f_0018CF00(Context *ctx) {\n/* 0018CF84 E0817000 */\n    uint32_t m = reg0_;\n    reg7_ = reg1_ + m;\n/* 0018CF88 E5847000 */\n    uint32_t m = reg0_;\n}\n";

    #[test]
    fn patches_once_and_declares() {
        let mut text = GENERATED.as_bytes().to_vec();
        assert_eq!(apply(&mut text, &PATCHES[0]), Ok(Outcome::Patched));
        let s = String::from_utf8(text.clone()).unwrap();
        assert!(s.contains("/* 0018CF84 E0817000 */\n    uint32_t m = smt4_boost_exp(ctx, reg0_); /* smt4-recomp"));
        // only the one after the anchor, not the next instruction's
        assert!(s.contains("/* 0018CF88 E5847000 */\n    uint32_t m = reg0_;"));
        // the declarations go before the first function, once
        let decl = s.find("extern uint32_t smt4_boost_exp").unwrap();
        assert!(decl < s.find("void f_").unwrap());
        assert_eq!(s.matches("extern uint32_t smt4_talk_debug").count(), 1);
        // a second run changes nothing
        assert_eq!(apply(&mut text, &PATCHES[0]), Ok(Outcome::Already));
        assert_eq!(String::from_utf8(text).unwrap(), s);
    }

    #[test]
    fn fails_when_the_code_moved() {
        let mut text = GENERATED.replace("    uint32_t m = reg0_;\n    reg7_", "    uint32_t m = reg2_;\n    reg7_").into_bytes();
        assert!(apply(&mut text, &PATCHES[0]).unwrap_err().contains("expected exactly one"));
    }

    /// an instruction in two functions, as when Zakuro's hints split one,
    /// is patched in both
    #[test]
    fn patches_every_function_an_instruction_is_in() {
        let twice = format!("{GENERATED}\nvoid f_0018CF40(Context *ctx) {{\n/* 0018CF84 E0817000 */\n    uint32_t m = reg0_;\n/* 0018CF88 E5847000 */\n}}\n");
        let mut text = twice.into_bytes();
        assert_eq!(apply(&mut text, &PATCHES[0]), Ok(Outcome::Patched));
        let s = String::from_utf8(text).unwrap();
        assert_eq!(s.matches("smt4_boost_exp(ctx, reg0_)").count(), 2);
        assert_eq!(s.matches("extern uint32_t smt4_boost_exp").count(), 1);
    }

    /// a folder's files are patched where their anchors are, and a patch
    /// whose anchor is nowhere fails
    #[test]
    fn a_folder_is_patched_and_missing_anchors_fail() {
        let dir = std::env::temp_dir().join(format!("smt4-patches {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("code000.c"), GENERATED).unwrap();
        std::fs::write(dir.join("other.c"), GENERATED).unwrap();
        let error = patch_dir(&dir).err().unwrap();
        assert!(error.contains("0018D190"), "{error}");
        // the first patch was applied before the second one failed, but
        // nothing is written then
        assert_eq!(std::fs::read_to_string(dir.join("code000.c")).unwrap(), GENERATED);
        assert_eq!(std::fs::read_to_string(dir.join("other.c")).unwrap(), GENERATED, "only code*.c");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn overrides_and_cheats_come_along() {
        assert!(OVERRIDES.iter().all(|(_, text)| text.contains("RECOMP_OVERRIDE") || text.contains("smt4_")));
        assert!(OVERRIDES[0].1.contains("static void smt4_log_override"), "the log goes with the code");
        assert!(cheat_blocks(CHEATS).len() >= 20);
    }

    #[test]
    fn adds_only_missing_cheats() {
        let ours = "[Widescreen 16:9]\nD3000000 00000000\n\n[EXP x2]\n00000000 00000002\n\n";
        let theirs = "[Widescreen 16:9]\n*citra_enabled\nD3000000 00000000\r\n\r\n\r\n";
        let theirs = theirs.replace("\r\n", "\n").replace('\n', "\r\n");
        let (merged, added) = merge_cheats(&theirs, ours);
        assert_eq!(added, 1);
        assert_eq!(
            merged,
            "[Widescreen 16:9]\r\n*citra_enabled\r\nD3000000 00000000\r\n\r\n[EXP x2]\r\n00000000 00000002\r\n"
        );
        assert_eq!(merge_cheats(&merged, ours).1, 0);
    }
}
