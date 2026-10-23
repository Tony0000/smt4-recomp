//! The game's own icon, the one the 3DS HOME Menu shows, on the launcher's
//! window and, on macOS, in the Dock, read from the ROM once it's chosen, as
//! the game shows its own: the launcher carries none of the game's art.

use std::path::Path;

use eframe::egui;

/// how big the icon is shown, the ROM's 48 pixels as square blocks
const SIZE: usize = 256;

/// shows the icon of the game at rom, when it has one
pub fn show(ctx: &egui::Context, rom: &Path) {
    let Ok(rgb) = smt4_patches::icon::from_rom(rom) else { return };
    let rgba = smt4_patches::icon::rgba(&rgb, SIZE);
    #[cfg(target_os = "macos")]
    {
        let _ = ctx;
        dock(&rgba, SIZE);
    }
    #[cfg(not(target_os = "macos"))]
    ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }))));
}

/// the Dock's icon, which macOS takes from the application rather than its
/// window
#[cfg(target_os = "macos")]
fn dock(rgba: &[u8], size: usize) {
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
    use objc2_foundation::NSSize;

    // AppKit is the main thread's, which the window's events come on
    let Some(main) = MainThreadMarker::new() else { return };
    // SAFETY: no planes given, so the image allocates its own pixels, which
    // are written below, size by size RGBA, 4 bytes a pixel
    unsafe {
        let Some(image) = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            size as isize,
            size as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (size * 4) as isize,
            32,
        ) else {
            return;
        };
        let pixels = image.bitmapData();
        if pixels.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), pixels, size * size * 4);
        let icon = NSImage::initWithSize(NSImage::alloc(), NSSize::new(size as f64, size as f64));
        icon.addRepresentation(&image);
        NSApplication::sharedApplication(main).setApplicationIconImage(Some(&icon));
    }
}
