//! The 800x600 software canvas.
//!
//! The original composes every screen into a DirectDraw surface and then
//! blits the dirty rectangles to the window. VolleyForge does the same thing
//! with a plain RGBA buffer, which keeps the rendering pixel exact and makes
//! `--shot` produce exactly what the window shows.
//!
//! The blit semantics follow the original: a pixel is either copied verbatim
//! or skipped completely - there is no alpha blending in the original.

use image::RgbaImage;

use crate::court::{VIEW_H, VIEW_W};

/// A fixed 800x600 RGBA frame buffer.
#[derive(Clone, Debug)]
pub struct Canvas {
    data: Vec<u8>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    pub fn new() -> Self {
        Canvas {
            data: vec![0; (VIEW_W * VIEW_H * 4) as usize],
        }
    }

    /// Raw RGBA bytes, row by row, top down.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Take ownership of a raw RGBA buffer.
    pub fn from_raw(data: Vec<u8>) -> Canvas {
        Canvas { data }
    }

    /// Darken the picture the way the original's match-over screen does
    /// (`FUN_00449D3C`): every colour channel keeps its top six bits and is
    /// then halved, the rectangle in the middle is quartered.
    ///
    /// The rectangle is given in top-down coordinates.
    pub fn darken(&mut self, rect: (i32, i32, i32, i32)) {
        for y in 0..VIEW_H {
            for x in 0..VIEW_W {
                let d = ((y * VIEW_W + x) * 4) as usize;
                let inside = x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3;
                for c in 0..3 {
                    let v = self.data[d + c] & 0xfc;
                    self.data[d + c] = if inside { v >> 2 } else { v >> 1 };
                }
            }
        }
    }

    /// Write a single pixel.
    pub fn put_pixel(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8) {
        if x < 0 || y < 0 || x >= VIEW_W || y >= VIEW_H {
            return;
        }
        let d = ((y * VIEW_W + x) * 4) as usize;
        self.data[d] = r;
        self.data[d + 1] = g;
        self.data[d + 2] = b;
        self.data[d + 3] = 255;
    }

    /// Fill the whole canvas with one colour.
    pub fn fill(&mut self, rgba: [u8; 4]) {
        for p in self.data.chunks_exact_mut(4) {
            p.copy_from_slice(&rgba);
        }
    }

    /// Copy a raw RGBA image over the whole canvas (the original's
    /// `FUN_004027E8` background blit).
    pub fn copy_from(&mut self, src: &[u8]) {
        let n = self.data.len().min(src.len());
        self.data[..n].copy_from_slice(&src[..n]);
    }

    /// Blit a sprite, skipping transparent pixels.
    ///
    /// This is the original's copy semantics: the destination pixel is
    /// overwritten, never blended.
    pub fn blit(&mut self, sprite: &[u8], w: usize, h: usize, x: i32, y: i32) {
        self.blit_with(sprite, w, h, x, y, |r, g, b, _| Some((r, g, b)));
    }

    /// Blit a sprite through a colour filter.
    ///
    /// The filter returns `None` for pixels that should be skipped.
    pub fn blit_with(
        &mut self,
        sprite: &[u8],
        w: usize,
        h: usize,
        x: i32,
        y: i32,
        f: impl Fn(u8, u8, u8, u8) -> Option<(u8, u8, u8)>,
    ) {
        let stride = w * 4;
        for sy in 0..h {
            let dy = y + sy as i32;
            if dy < 0 || dy >= VIEW_H {
                continue;
            }
            let row = sy * stride;
            if row + stride > sprite.len() {
                break;
            }
            for sx in 0..w {
                let dx = x + sx as i32;
                if dx < 0 || dx >= VIEW_W {
                    continue;
                }
                let i = row + sx * 4;
                let (r, g, b, a) = (
                    sprite[i],
                    sprite[i + 1],
                    sprite[i + 2],
                    sprite[i + 3],
                );
                if a == 0 {
                    continue;
                }
                let Some((nr, ng, nb)) = f(r, g, b, a) else {
                    continue;
                };
                let d = ((dy * VIEW_W + dx) * 4) as usize;
                self.data[d] = nr;
                self.data[d + 1] = ng;
                self.data[d + 2] = nb;
                self.data[d + 3] = 255;
            }
        }
    }

    /// Blit a single row of a sprite.
    pub fn blit_row(&mut self, sprite: &[u8], w: usize, row: usize, x: i32, y: i32) {
        self.blit_row_with(sprite, w, row, x, y, |r, g, b, _| Some((r, g, b)));
    }

    /// Blit a single row of a sprite through a colour filter.
    pub fn blit_row_with(
        &mut self,
        sprite: &[u8],
        w: usize,
        row: usize,
        x: i32,
        y: i32,
        f: impl Fn(u8, u8, u8, u8) -> Option<(u8, u8, u8)>,
    ) {
        let stride = w * 4;
        let start = row * stride;
        if start + stride > sprite.len() {
            return;
        }
        for sx in 0..w {
            let dx = x + sx as i32;
            if dx < 0 || dx >= VIEW_W || y < 0 || y >= VIEW_H {
                continue;
            }
            let i = start + sx * 4;
            let (r, g, b, a) = (sprite[i], sprite[i + 1], sprite[i + 2], sprite[i + 3]);
            if a == 0 {
                continue;
            }
            let Some((nr, ng, nb)) = f(r, g, b, a) else {
                continue;
            };
            let d = ((y * VIEW_W + dx) * 4) as usize;
            self.data[d] = nr;
            self.data[d + 1] = ng;
            self.data[d + 2] = nb;
            self.data[d + 3] = 255;
        }
    }

    /// Blit a sprite with the original's night palette.
    ///
    /// The original renders every **8 bpp** image (beach, logo, icons) in the
    /// menu with a darkened palette - `(c & 0xF8) >> 3` per channel. The
    /// 24 bpp images (font, shadows, cursor) keep their colours, which is why
    /// the menu text still shows its full rainbow.
    pub fn blit_night(&mut self, sprite: &[u8], w: usize, h: usize, x: i32, y: i32) {
        self.blit_with(sprite, w, h, x, y, |r, g, b, _| {
            Some(((r & 0xF8) >> 3, (g & 0xF8) >> 3, (b & 0xF8) >> 3))
        });
    }

    /// Blit a sprite with the original's greyed-out look (`FUN_0044724C`).
    pub fn blit_grey(
        &mut self,
        sprite: &[u8],
        w: usize,
        h: usize,
        x: i32,
        y: i32,
        highlight: bool,
    ) {
        self.blit_with(sprite, w, h, x, y, |r, g, b, _| {
            Some(crate::layout::grey_rgb(r, g, b, highlight))
        });
    }

    /// Blit a sprite with the original's "selected entry" brightening.
    pub fn blit_highlighted(
        &mut self,
        sprite: &[u8],
        w: usize,
        h: usize,
        x: i32,
        y: i32,
    ) {
        self.blit_with(sprite, w, h, x, y, |r, g, b, _| {
            Some((
                crate::layout::highlight_channel(r),
                crate::layout::highlight_channel(g),
                crate::layout::highlight_channel(b),
            ))
        });
    }

    /// Copy the canvas into an image, ready to be uploaded as a texture.
    pub fn to_image(&self) -> RgbaImage {
        RgbaImage::from_raw(VIEW_W as u32, VIEW_H as u32, self.data.clone())
            .unwrap_or_else(|| RgbaImage::new(VIEW_W as u32, VIEW_H as u32))
    }
}
