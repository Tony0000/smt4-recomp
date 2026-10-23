//! The game's own icon, the one the 3DS HOME Menu shows, read from your ROM:
//! the 48x48 picture in the SMDH, the ExeFS's "icon" file. The builder puts
//! it on the macOS app it makes (tools/build/src/bundle.rs), the launcher
//! shows it on its window and in the Dock once you choose the ROM. Like the
//! rest of what the build makes, it comes from the game: nothing of it is
//! in this repository.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// how big the icon is, a square of 8x8 tiles
pub const SIZE: usize = 48;
/// where the large icon starts in the SMDH
const LARGE_ICON: usize = 0x24C0;
const SMDH_SIZE: usize = 0x36C0;

/// reads n bytes at offset
fn read_at(file: &mut std::fs::File, offset: u64, n: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0; n];
    file.seek(SeekFrom::Start(offset)).and_then(|_| file.read_exact(&mut bytes)).map_err(|e| format!("the ROM is cut short: {e}"))?;
    Ok(bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> u64 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as u64
}

/// the icon of a decrypted .3ds/.cci (NCSD) or .cxi (NCCH), RGB rows from
/// the top
pub fn from_rom(rom: &Path) -> Result<Vec<[u8; 3]>, String> {
    let mut file = std::fs::File::open(rom).map_err(|e| format!("could not open the ROM: {e}"))?;
    let header = read_at(&mut file, 0, 0x200)?;
    // a .3ds holds the game's NCCH as its first partition
    let ncch = match &header[0x100..0x104] {
        b"NCSD" => u32_at(&header, 0x120) * 0x200,
        b"NCCH" => 0,
        _ => return Err("not a decrypted .3ds/.cci/.cxi".into()),
    };
    let ncch_header = read_at(&mut file, ncch, 0x200)?;
    if &ncch_header[0x100..0x104] != b"NCCH" {
        return Err("no NCCH where the game should be".into());
    }
    let unit = 0x200u64 << ncch_header[0x188 + 6];
    let exefs = ncch + u32_at(&ncch_header, 0x1A0) * unit;
    let exefs_header = read_at(&mut file, exefs, 0x200)?;
    let icon = (0..8)
        .map(|i| &exefs_header[i * 16..i * 16 + 16])
        .find(|entry| &entry[..8] == b"icon\0\0\0\0")
        .ok_or("the ROM has no icon")?;
    let smdh = read_at(&mut file, exefs + 0x200 + u32_at(icon, 8), SMDH_SIZE)?;
    if &smdh[..4] != b"SMDH" {
        return Err("the ROM's icon can't be read, is it decrypted?".into());
    }
    Ok(decode(&smdh[LARGE_ICON..LARGE_ICON + SIZE * SIZE * 2]))
}

/// RGB565 texels in 8x8 tiles, left to right then top to bottom, each
/// tile's texels in Z order, as rows of RGB from the top
fn decode(texels: &[u8]) -> Vec<[u8; 3]> {
    let mut rgb = vec![[0; 3]; SIZE * SIZE];
    let mut texel = texels.as_chunks::<2>().0.iter().map(|&t| u16::from_le_bytes(t));
    for tile_y in (0..SIZE).step_by(8) {
        for tile_x in (0..SIZE).step_by(8) {
            for i in 0..64 {
                // x takes the even bits of i, y the odd ones
                let x = (i & 1) | ((i >> 1) & 2) | ((i >> 2) & 4);
                let y = ((i >> 1) & 1) | ((i >> 2) & 2) | ((i >> 3) & 4);
                let v = texel.next().unwrap_or(0);
                let (r, g, b) = ((v >> 11) & 0x1F, (v >> 5) & 0x3F, v & 0x1F);
                // the top bits again in the bottom ones, so white stays white
                let wide = |c: u16, bits: u32| ((c << (8 - bits)) | (c >> (2 * bits - 8))) as u8;
                rgb[(tile_y + y) * SIZE + tile_x + x] = [wide(r, 5), wide(g, 6), wide(b, 5)];
            }
        }
    }
    rgb
}

/// the icon at size pixels across, each of its pixels a square block, as
/// the HOME Menu's pixels are
pub fn scaled(rgb: &[[u8; 3]], size: usize) -> Vec<[u8; 3]> {
    (0..size * size).map(|i| rgb[(i / size) * SIZE / size * SIZE + (i % size) * SIZE / size]).collect()
}

/// the icon at size pixels across, as RGBA rows from the top, for a window
pub fn rgba(rgb: &[[u8; 3]], size: usize) -> Vec<u8> {
    scaled(rgb, size).iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect()
}

/// an uncompressed PNG of RGB rows, square, size pixels across: zlib's
/// stored blocks need nothing to compress with, and macOS's tools read it
pub fn png(rgb: &[[u8; 3]], size: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(size * (size * 3 + 1));
    for row in rgb.chunks_exact(size) {
        raw.push(0); // no filter
        raw.extend(row.iter().flatten());
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks = raw.chunks(0xFFFF);
    let last = blocks.len() - 1;
    for (i, block) in raw.chunks(0xFFFF).enumerate() {
        let len = block.len() as u16;
        zlib.push((i == last) as u8);
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend(block);
    }
    zlib.extend(adler32(&raw).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend((size as u32).to_be_bytes());
    header.extend((size as u32).to_be_bytes());
    header.extend([8, 2, 0, 0, 0]); // 8 bits, RGB, deflate, no filter, no interlace
    for (kind, data) in [(b"IHDR", &header), (b"IDAT", &zlib), (b"IEND", &Vec::new())] {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_go_in_z_order() {
        // the 3rd texel of the first tile is (0, 1), the 64th (7, 7), the
        // 65th the first of the next tile across
        let mut texels = vec![0u8; SIZE * SIZE * 2];
        for (i, value) in [(2, 0xF800u16), (63, 0x07E0), (64, 0x001F)] {
            texels[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        let rgb = decode(&texels);
        assert_eq!(rgb[SIZE], [255, 0, 0]);
        assert_eq!(rgb[7 * SIZE + 7], [0, 255, 0]);
        assert_eq!(rgb[8], [0, 0, 255]);
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn scaling_keeps_the_corners() {
        let mut rgb = vec![[0u8; 3]; SIZE * SIZE];
        rgb[0] = [1, 1, 1];
        rgb[SIZE * SIZE - 1] = [2, 2, 2];
        let big = scaled(&rgb, 1024);
        assert_eq!(big[0], [1, 1, 1]);
        assert_eq!(big[1024 * 1024 - 1], [2, 2, 2]);
        assert_eq!(png(&big, 1024)[..8], *b"\x89PNG\r\n\x1a\n");
    }
}
