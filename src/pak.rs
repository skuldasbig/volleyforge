//! Decompression and slicing of the original `*.pak` containers.
//!
//! # Container format (recovered from the binary, not guessed)
//!
//! The unit list in the `PACKAGEINFO` resource names an `lzwlike` unit, and
//! `FUN_004418A8` is indeed an LZW decoder — but it is **TIFF-style old LZW**,
//! not a GIF/TIFF-newstyle variant:
//!
//! | detail | value | evidence |
//! |--------|-------|----------|
//! | code width | fixed 12 bit | `FUN_0044176C`: `code = word & 0xFFF` |
//! | packing | two codes per 24-bit little-endian word, low half first | `word & 0xFFF` then `word >> 12 & 0xFFF` |
//! | stream window | 768 bytes = 256 words | `stream.Read(buf, 0x300)` |
//! | clear code | `0xFFE` | `FUN_004418A8`: `if (uVar3 == 0xFFE) dictSize = 0x100` |
//! | end code | `0xFFF` | `while (uVar3 != 0xFFF)` |
//! | dictionary | starts at 256, +1 per code pair | `dict[iVar2*2] = prev; dict[iVar2*2+1] = code` |
//!
//! # What the decoded stream contains
//!
//! `FUN_004186E4(screen, row)` is not a length — it returns a pointer to
//! **row `row`** of the decoded 800x600 image. The unpacker then `memcpy`s
//! from there, so every sprite sheet is a band at a fixed row of the one
//! decompressed bitmap:
//!
//! | row | contents | bytes each |
//! |-----|----------|-----------|
//! | 599 | whole background | 800*600*4 |
//! | 100 | 5 player blobs, 148x101 | 59 792 |
//! | 63  | 16 cursor/menu icons, 64x64 | 16 384 |
//! | 24  | 10 shadows, 129x25 | 12 900 |
//! | 23  | 55 font glyphs, 24x24 | 2 304 |
//!
//! Pixels are BGRA dwords. Transparency is derived from brightness:
//! `b0 + b1 + b2 < 0x28` makes a sprite pixel transparent, `< 0xB4` for the
//! icons.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info, trace, warn};

/// Clear code in the original's LZW dialect.
pub const CLEAR: u16 = 0xFFE;
/// End code in the original's LZW dialect.
pub const END: u16 = 0xFFF;
/// Number of bytes consumed per refill (256 24-bit words).
const WINDOW: usize = 0x300;
/// Words per window.
const WORDS: usize = 0x100;

// ---------------------------------------------------------------------------
// Bit reader: 12-bit codes, two per 24-bit little-endian word, low half first
// ---------------------------------------------------------------------------

struct CodeReader<'a> {
    data: &'a [u8],
    /// Byte offset of the current 768-byte window.
    base: usize,
    /// Word index inside the window.
    word: usize,
    /// Whether the high half of the current 24-bit word is still pending.
    high: bool,
    /// The 24-bit word currently being consumed.
    val: u32,
    end: bool,
    /// Set after an END code: the next read starts exactly here.
    pending_base: Option<usize>,
    /// Word index at the most recent END code.
    last_end_pos: i64,
}

impl<'a> CodeReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        CodeReader {
            data,
            base: 0,
            word: 0,
            high: false,
            val: 0,
            end: false,
            pending_base: None,
            last_end_pos: -1,
        }
    }

    fn next(&mut self) -> Option<u16> {
        if self.end {
            return None;
        }
        if !self.high {
            // refill the 768-byte window when it is exhausted
            if let Some(b) = self.pending_base.take() {
                if b >= self.data.len() {
                    self.end = true;
                    return None;
                }
                self.base = b;
                self.word = 0;
            } else if self.word >= WORDS {
                if self.base + WINDOW > self.data.len() {
                    self.end = true;
                    return None;
                }
                self.base += WINDOW;
                self.word = 0;
            }
            let off = self.base + self.word * 3;
            if off + 3 > self.data.len() {
                self.end = true;
                return None;
            }
            self.val = (self.data[off] as u32)
                | ((self.data[off + 1] as u32) << 8)
                | ((self.data[off + 2] as u32) << 16);
            self.word += 1;
            self.high = true;
            let code = (self.val & 0xFFF) as u16;
            if code == END {
                self.mark_end();
            }
            Some(code)
        } else {
            self.high = false;
            let code = ((self.val >> 12) & 0xFFF) as u16;
            if code == END {
                self.mark_end();
            }
            Some(code)
        }
    }

    /// Reproduce `stream.Seek((pos - 0x100) * 3, SEEK_CUR)` from
    /// `FUN_0044176C`: after an END code the file cursor moves by
    /// `pos - 0x100` words from its current position, then `pos` is armed to
    /// 0x100 so the following call refills the window. When the END sits on
    /// the last word this is a no-op; otherwise it walks *backwards* into the
    /// current window, which is exactly the next chunk's start.
    /// Handle an END code.
    ///
    /// The container stores a dedicated terminator word `0xFFFFFF` between
    /// images: both of its 12-bit halves are 0xFFF, and the next word starts
    /// the next image's BMP header ('B','M' = 0x42,0x4D). Decoding therefore
    /// simply continues at the following word.
    ///
    /// The original's `stream.Seek((pos - 0x100) * 3, ...)` from the
    /// decompiled `FUN_0044176C` is a Ghidra artefact of that function's
    /// destroyed stack frame - the raw bytes contradict it.
    fn mark_end(&mut self) {
        self.high = false;
        self.pending_base = None;
        self.last_end_pos = self.word as i64;
    }

    /// Position of the word index at the most recent END code.
    fn last_end_pos(&self) -> i64 {
        self.last_end_pos
    }

    fn exhausted(&self) -> bool {
        self.end || (self.base >= self.data.len() && self.word >= WORDS && self.pending_base.is_none())
    }
}

// ---------------------------------------------------------------------------
// LZW decode
// ---------------------------------------------------------------------------

/// Prefix/suffix dictionary, mirroring the original's flat pair array.
struct Dict {
    prefix: Vec<u16>,
    suffix: Vec<u8>,
    next: u16,
}

impl Dict {
    fn new() -> Self {
        let mut d = Dict {
            prefix: vec![0; 4096],
            suffix: vec![0; 4096],
            next: 256,
        };
        d.reset();
        d
    }

    fn reset(&mut self) {
        for i in 0..256 {
            self.prefix[i] = 0;
            self.suffix[i] = i as u8;
        }
        self.next = 256;
    }

    /// Resolve a code to its byte string.
    ///
    /// Codes below 256 are plain literals and terminate the chain; dictionary
    /// entries (>= 256) carry a prefix code. Walking literals one step too far
    /// is what produced the stray `00` between every output byte earlier.
    fn string(&self, code: u16) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        let mut c = code;
        let mut guard = 0;
        loop {
            out.push(self.suffix[c as usize]);
            if (c as usize) < 256 {
                break; // literal: no prefix
            }
            c = self.prefix[c as usize];
            guard += 1;
            if guard > 4096 {
                break;
            }
        }
        out.reverse();
        out
    }

    fn add(&mut self, code: u16, byte: u8) {
        if self.next < 4094 {
            self.prefix[self.next as usize] = code;
            self.suffix[self.next as usize] = byte;
            self.next += 1;
        }
    }
}


/// A parsed 8-bit BMP exactly as the original stores it.
#[derive(Debug)]
pub struct Bitmap8 {
    pub w: usize,
    pub h: usize,
    pub bpp: usize,
    /// Bytes per scanline, padded to a multiple of four as BMP requires.
    pub stride: usize,
    /// 256 BGRA palette entries (8 bpp only).
    pub palette: Vec<u8>,
    /// Palette indices (8 bpp) or BGR triples (24 bpp), row-major, **bottom-up**.
    pub indices: Vec<u8>,
}

/// Parse the decompressed `.pak` stream as an 8-bit BMP.
///
/// The header recovered from the live decode:
///
/// ```text
/// 42 4D 36 57 07 00   'BM', bfSize = 0x75736 = 481 078 (== decoded length)
///          36 04 00 00 bfOffBits = 1078 = 54 + 256*4
///          28 00 00 00 biSize = 40
///          20 03 00 00 biWidth = 800
///          58 02 00 00 biHeight = 600
///          01 00        biPlanes = 1
///          08 00        biBitCount = 8
/// ```
pub fn parse_bmp8(data: &[u8]) -> Result<Bitmap8, String> {
    let head: String = data.iter().take(30).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
    info!(head = %head, "chunk header bytes");
    if data.len() < 14 || &data[0..2] != b"BM" {
        return Err("not a BMP stream".into());
    }
    let u16at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as usize;
    let u32at = |o: usize| {
        u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize
    };

    let off_bits = u32at(10);
    let info = 14;
    let bi_size = u32at(14);
    let w = u32at(18);
    let h = u32at(22);
    let planes = u16at(26);
    let bpp = u16at(28);
    let compression = u32at(30);

    info!(
        bf_size = u32at(2),
        off_bits,
        bi_size,
        w,
        h,
        planes,
        bpp,
        compression,
        "parsed BMP header"
    );

    if bpp != 8 && bpp != 24 {
        return Err(format!("expected 8 or 24 bpp, found {bpp}"));
    }
    if planes != 1 {
        return Err(format!("expected 1 plane, found {planes}"));
    }
    if compression != 0 {
        return Err(format!("expected BI_RGB, found compression {compression}"));
    }

    let palette_off = info + bi_size;
    let palette_bytes = if bpp == 8 {
        off_bits.saturating_sub(palette_off)
    } else {
        0
    };
    let palette_len = palette_bytes.min(1024);
    let palette = if palette_len > 0 {
        data[palette_off..palette_off + palette_len].to_vec()
    } else {
        Vec::new()
    };

    let bytes_per_px = bpp / 8;
    // BMP scanlines are padded to a multiple of four bytes.
    let stride = (w * bytes_per_px + 3) & !3;
    let indices = data[off_bits..].to_vec();
    let expected = stride * h;
    info!(
        palette_entries = palette_len / 4,
        index_bytes = indices.len(),
        expected,
        stride,
        w,
        h,
        bpp,
        "BMP payload"
    );

    if indices.len() < expected {
        return Err(format!(
            "pixel data is {} bytes, need {expected} for {w}x{h} at {bpp} bpp",
            indices.len()
        ));
    }

    Ok(Bitmap8 {
        w,
        h,
        bpp,
        stride,
        palette,
        indices,
    })
}

/// Expand to a **top-down BGRA** buffer, which is the layout the game's
/// surface has once GDI has blitted it.
pub fn expand_bgra(bmp: &Bitmap8) -> Vec<u8> {
    let mut out = vec![0u8; bmp.w * bmp.h * 4];
    let bpp = bmp.bpp / 8;
    for y in 0..bmp.h {
        let src_row = (bmp.h - 1 - y) * bmp.stride; // bottom-up -> top-down
        let dst_row = y * bmp.w;
        for x in 0..bmp.w {
            let s = src_row + x * bpp;
            let d = (dst_row + x) * 4;
            if bpp == 1 {
                let idx = bmp.indices[s] as usize;
                let p = idx * 4;
                if p + 4 <= bmp.palette.len() {
                    out[d] = bmp.palette[p];
                    out[d + 1] = bmp.palette[p + 1];
                    out[d + 2] = bmp.palette[p + 2];
                    // The palette's fourth byte is the *reserved* field of a
                    // Windows palette entry and is 0 in every image of the
                    // container. The original never reads it - transparency is
                    // decided by brightness alone (`FUN_00445AA0`). Copying it
                    // would make the whole 8 bpp background transparent.
                    out[d + 3] = 255;
                }
            } else {
                // 24 bpp stores BGR triples. The buffer is always BGRA, so
                // that `to_rgba` can convert every kind of sprite the same
                // way.
                out[d] = bmp.indices[s];
                out[d + 1] = bmp.indices[s + 1];
                out[d + 2] = bmp.indices[s + 2];
                out[d + 3] = 255;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Slice the decoded image into the original's sprite sheets
// ---------------------------------------------------------------------------

pub const IMG_W: usize = 800;
pub const IMG_H: usize = 600;
pub const PIXELS: usize = IMG_W * IMG_H;
const SCANLINE: usize = IMG_W * 4;

/// One decoded sheet plus the rows it came from.
#[derive(Debug)]
pub struct Sheet {
    pub name: String,
    pub row: usize,
    pub count: usize,
    pub w: usize,
    pub h: usize,
    /// BGRA bytes, `count` frames laid out back to back.
    pub pixels: Vec<u8>,
}

impl Sheet {
    pub fn frame(&self, index: usize) -> &[u8] {
        let stride = self.w * self.h * 4;
        &self.pixels[index * stride..(index + 1) * stride]
    }
}

/// Convert a BGRA sprite to RGBA.
///
/// The original never looks at the alpha byte: it decides transparency purely
/// from brightness, `b0 + b1 + b2 < threshold`. The palette shipped in the
/// bitmap has an alpha of 0 everywhere, so trusting it would erase the whole
/// image — the rule below is what `FUN_00445AA0` actually applies
/// (0x28 for blobs, glyphs and shadows, 0xB4 for the menu icons, 0 for the
/// opaque background).
pub fn to_rgba(bgra: &[u8], w: usize, h: usize, threshold: u32) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let b = bgra[i * 4] as u32;
        let g = bgra[i * 4 + 1] as u32;
        let r = bgra[i * 4 + 2] as u32;
        out[i * 4] = r as u8;
        out[i * 4 + 1] = g as u8;
        out[i * 4 + 2] = b as u8;
        out[i * 4 + 3] = if b + g + r < threshold { 0 } else { 255 };
    }
    out
}

/// One sprite sheet: `count` frames of `w`x`h` BGRA pixels.
#[derive(Debug)]
pub struct Group {
    pub name: String,
    pub frame_w: usize,
    pub frame_h: usize,
    pub count: usize,
    pub bpp: usize,
    /// BGRA bytes, frames laid out back to back.
    pub pixels: Vec<u8>,
    /// The original BMP files, byte for byte as they came out of the
    /// container. Kept so the untouched source can be written to disk next to
    /// the rendered PNG.
    pub raw_frames: Vec<Vec<u8>>,
}

impl Group {
    pub fn frame(&self, index: usize) -> &[u8] {
        let stride = self.frame_w * self.frame_h * 4;
        &self.pixels[index * stride..(index + 1) * stride]
    }
}

/// All sprite sheets cut out of one decoded `graph.pak` image.
#[derive(Debug)]
pub struct Assets {
    pub decoded_len: usize,
    /// Full 800x600 background, BGRA.
    pub background: Vec<u8>,
    /// Every further sheet, in file order.
    pub groups: Vec<Group>,
}

/// Copy `count` frames of `w`x`h` BGRA pixels out of a top-down image.
fn cut(img: &[u8], row: usize, count: usize, w: usize, h: usize) -> Option<Sheet> {
    let need = row * IMG_W * 4 + count * w * h * 4;
    if img.len() < need {
        warn!(
            row,
            count,
            w,
            h,
            need,
            have = img.len(),
            "sheet does not fit"
        );
        return None;
    }
    Some(Sheet {
        name: format!("row{row}"),
        row,
        count,
        w,
        h,
        pixels: img[row * IMG_W * 4..need].to_vec(),
    })
}

/// Decode **every** LZW chunk in a `.pak` file.
///
/// The original calls the decoder once per asset, and the file cursor keeps
/// running, so a `.pak` is a sequence of independently compressed bitmaps.
/// `FUN_0044176C` stops at an END code, skips the remainder of the current
/// 768-byte window, and resumes at the next one — that boundary is what
/// separates the chunks.
pub fn decode_chunks(data: &[u8]) -> Vec<Vec<u8>> {
    let mut r = CodeReader::new(data);
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let mut dict = Dict::new();
    let mut prev: Option<u16> = None;
    let mut ends = 0u32;
    let mut clears = 0u32;

    while !r.exhausted() {
        let Some(code) = r.next() else {
            info!(base = r.base, word = r.word, "code reader returned None");
            break;
        };
        if code == END {
            // End of the current image. The original stops its decode loop
            // here; the next call to the driver continues at the file cursor
            // the reader repositioned to.
            info!(
                base = r.base,
                end_pos = r.last_end_pos(),
                produced = out.len(),
                "END code"
            );
            if !out.is_empty() {
                chunks.push(std::mem::take(&mut out));
            }
            dict.reset();
            prev = None;
            if out.capacity() == 0 {
                out = Vec::with_capacity(64 * 1024);
            }
            continue;
        }
        if code == CLEAR {
            dict.reset();
            prev = None;
            clears += 1;
            continue;
        }

        let entry: Vec<u8> = if (code as usize) < dict.next as usize {
            dict.string(code)
        } else if (code as usize) == dict.next as usize {
            match prev {
                Some(p) => {
                    let mut s = dict.string(p);
                    let fb = s.first().copied().unwrap_or(0);
                    s.push(fb);
                    s
                }
                None => break,
            }
        } else {
            info!(
                chunk = chunks.len(),
                code,
                next = dict.next,
                base = r.base,
                word = r.word,
                "code outside dictionary - stopping"
            );
            break;
        };

        out.extend_from_slice(&entry);
        if out.len() > 16 * 1024 * 1024 {
            break;
        }
        if let Some(p) = prev {
            dict.add(p, entry[0]);
        }
        prev = Some(code);
    }

    if !out.is_empty() {
        chunks.push(out);
    }

    info!(
        chunks = chunks.len(),
        total = chunks.iter().map(|c| c.len()).sum::<usize>(),
        ends,
        clears,
        "LZW chunks decoded"
    );
    chunks
}

impl Assets {
    /// Decode a `.pak` file and slice every image it contains.
    pub fn load(path: &Path) -> Result<Assets, String> {
        let data = fs::read(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        info!(file = ?path.file_name(), bytes = data.len(), "decoding pack");

        Self::from_bytes(&data)
    }

    /// Decode an in-memory `.pak` container.
    pub fn from_bytes(data: &[u8]) -> Result<Assets, String> {
        info!(bytes = data.len(), "decoding pack");

        let chunks = decode_chunks(data);
        if chunks.is_empty() {
            return Err("no LZW chunks decoded".into());
        }

        let mut assets: Option<Assets> = None;
        let mut groups: Vec<Group> = Vec::new();
        let mut cur_sig: Option<(usize, usize, usize)> = None;

        for (idx, chunk) in chunks.iter().enumerate() {
            let bmp = match parse_bmp8(chunk) {
                Ok(b) => b,
                Err(e) => {
                    warn!(chunk = idx, bytes = chunk.len(), error = %e, "not a BMP chunk");
                    continue;
                }
            };
            let img = expand_bgra(&bmp);
            info!(chunk = idx, w = bmp.w, h = bmp.h, bpp = bmp.bpp, "bitmap chunk");

            if assets.is_none() {
                assets = Some(Assets {
                    decoded_len: chunk.len(),
                    background: img,
                    groups: Vec::new(),
                });
                continue;
            }

            let sig = (bmp.w, bmp.h, bmp.bpp);
            if cur_sig != Some(sig) {
                cur_sig = Some(sig);
                groups.push(Group {
                    name: format!("g{}_{}x{}_{}bpp", groups.len(), bmp.w, bmp.h, bmp.bpp),
                    frame_w: bmp.w,
                    frame_h: bmp.h,
                    count: 0,
                    bpp: bmp.bpp,
                    pixels: Vec::new(),
                    raw_frames: Vec::new(),
                });
            }
            let g = groups.last_mut().unwrap();
            g.count += 1;
            g.pixels.extend_from_slice(&img);
            g.raw_frames.push(chunk.clone());
        }

        let mut assets = assets.ok_or("first chunk was not a bitmap")?;
        assets.groups = groups;
        info!(
            images = chunks.len(),
            groups = assets.groups.len(),
            inventory = ?assets
                .groups
                .iter()
                .map(|g| format!("{}:{}x{}x{}", g.name, g.frame_w, g.frame_h, g.count))
                .collect::<Vec<_>>(),
            "asset inventory"
        );
        Ok(assets)
    }
}

/// The folder the game looks in: the folder of the executable.
///
/// The original's containers (`graph.pak`, `sound.pak`, `text.pak`) have to
/// lie next to the program - the desktop version reads them from there, the
/// web version has them compiled into the wasm module.
pub fn default_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

