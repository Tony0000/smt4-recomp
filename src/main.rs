//! sdds4, recompiled ahead of time by 3dsrecomp and run by Zakuro. it
//! still reads the game for everything but the code, so it takes the path
//! to a copy of it the way Zakuro does.

use std::ffi::c_void;
use std::ptr::addr_of;

extern "C" {
    static recomp_abi: u32;
    static recomp_entry_count: u32;
    static recomp_entries: u8;
    static recomp_module_count: u32;
    static recomp_modules: u8;
}

/// appends a line to the file named by SMT4_HOOK_LOG, when it is set, without
/// going through the logger, so it works before and regardless of it. the
/// multipliers note each boost there themselves, overrides/multipliers.c.
fn hook_log(line: &str) {
    use std::io::Write;
    let Some(path) = std::env::var_os("SMT4_HOOK_LOG") else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn hook_log_start() {
    hook_log("--- sdds4 started, multiplier hooks linked ---");
}

fn main() {
    hook_log_start();
    // SAFETY: the tables 3dsrecomp wrote beside the code build.rs links in
    let code = unsafe {
        zakuro::Linked::new(
            0x00040000000E5C00,
            recomp_abi,
            addr_of!(recomp_entries) as *const c_void,
            recomp_entry_count,
            addr_of!(recomp_modules) as *const c_void,
            recomp_module_count,
        )
    };
    zakuro::run(Some(code));
}
