//! Runtime asset loading.
//!
//! Everything the game needs is decoded from the three `*.pak` containers at
//! startup and kept in memory:
//!
//! | container  | payload                                                    |
//! |------------|------------------------------------------------------------|
//! | `graph.pak`| 93 Windows BMPs - background, logo, menu art, sprites, font |
//! | `sound.pak`| 2 RIFF/WAVE sound effects                                  |
//! | `text.pak` | 3 localisation tables (French, German, English)             |
//!
//! The container format was recovered from the binary; see [`crate::pak`].

use std::fs;
use std::path::{Path, PathBuf};

use crate::court::{VIEW_H, VIEW_W};
use crate::log::{debug, error, info};
use crate::pak;

/// A set of equally sized frames, e.g. five blob animation frames.
#[derive(Debug, Clone)]
pub struct SpriteSet {
    pub name: String,
    pub frame_w: usize,
    pub frame_h: usize,
    pub count: usize,
    /// Bits per pixel of the source bitmap (8 or 24).
    pub bpp: usize,
    /// RGBA, frames back to back.
    pub pixels: Vec<u8>,
}

impl SpriteSet {
    pub fn frame(&self, index: usize) -> &[u8] {
        let stride = self.frame_w * self.frame_h * 4;
        let i = index.min(self.count.saturating_sub(1));
        &self.pixels[i * stride..(i + 1) * stride]
    }

    /// Apply the original's brightness based transparency rule: a pixel is
    /// transparent when the sum of its colour channels is below `threshold`.
    /// The palette's alpha byte is deliberately not consulted.
    pub fn with_threshold(&self, threshold: u32) -> SpriteSet {
        let stride = self.frame_w * self.frame_h * 4;
        let mut out = self.pixels.clone();
        for i in 0..self.count {
            let base = i * stride;
            for p in (0..stride).step_by(4) {
                let sum = out[base + p] as u32
                    + out[base + p + 1] as u32
                    + out[base + p + 2] as u32;
                out[base + p + 3] = if sum < threshold { 0 } else { 255 };
            }
        }
        SpriteSet {
            pixels: out,
            ..self.clone()
        }
    }
}

/// One localisation table: sections introduced by a `@name` line.
#[derive(Debug, Clone, Default)]
pub struct TextTable {
    pub sections: Vec<(String, Vec<String>)>,
}

impl TextTable {
    /// All entries of a section, in order.
    pub fn section(&self, name: &str) -> Option<&[String]> {
        self.sections
            .iter()
            .find(|(s, _)| s == name)
            .map(|(_, v)| v.as_slice())
    }

    /// Look an entry up by its position inside a section.
    pub fn entry(&self, section: &str, index: usize) -> Option<&str> {
        self.section(section)?.get(index).map(|s| s.as_str())
    }
}

/// Decode the original's text bytes.
///
/// The original is a Windows program: its text tables are in the system's
/// ANSI code page (CP1252), not UTF-8. Decoding them as UTF-8 destroys every
/// umlaut - the German "Zurück" arrives as "Zur?ck" and is then not drawn
/// at all, because the font only knows the upper case shape.
pub fn decode_text(raw: &[u8]) -> String {
    raw.iter()
        .map(|&b| match b {
            // The characters CP1252 puts into 0x80..0x9F (Latin-1 has the
            // control characters there).
            0x80 => '\u{20AC}',
            0x82 => '\u{201A}',
            0x83 => '\u{0192}',
            0x84 => '\u{201E}',
            0x85 => '\u{2026}',
            0x86 => '\u{2020}',
            0x87 => '\u{2021}',
            0x88 => '\u{02C6}',
            0x89 => '\u{2030}',
            0x8A => '\u{0160}',
            0x8B => '\u{2039}',
            0x8C => '\u{0152}',
            0x8E => '\u{017D}',
            0x91 => '\u{2018}',
            0x92 => '\u{2019}',
            0x93 => '\u{201C}',
            0x94 => '\u{201D}',
            0x95 => '\u{2022}',
            0x96 => '\u{2013}',
            0x97 => '\u{2014}',
            0x98 => '\u{02DC}',
            0x99 => '\u{2122}',
            0x9A => '\u{0161}',
            0x9B => '\u{203A}',
            0x9C => '\u{0153}',
            0x9E => '\u{017E}',
            0x9F => '\u{0178}',
            // Everything else is the same as Latin-1.
            other => other as char,
        })
        .collect()
}

/// Parse a localisation table.
///
/// A line starting with `@` opens a section, every other line is one entry of
/// the current section - the same layout the original uses (`@main`,
/// `@settings`, `@keydef`, `@win`, ...).
pub fn parse_text_table(raw: &[u8]) -> TextTable {
    let text = decode_text(raw);
    let mut table = TextTable::default();
    let mut current: Option<(String, Vec<String>)> = None;

    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if let Some(name) = line.strip_prefix('@') {
            if let Some(prev) = current.take() {
                table.sections.push(prev);
            }
            current = Some((name.trim().to_string(), Vec::new()));
        } else if let Some((_, entries)) = current.as_mut() {
            if !line.is_empty() {
                // The original's texts use the two characters `\` and `n`
                // as the line break inside a string - the German quit
                // question is stored as "Spiel\nwirklich\nbeenden?". The
                // font has no backslash, so the escape has to become a
                // real line break here.
                entries.push(line.replace("\\n", "\n"));
            }
        }
    }
    if let Some(prev) = current {
        table.sections.push(prev);
    }
    table
}

/// Glyph index of a character in the original's 24x24 font sheet.
///
/// The sheet holds 51 glyphs in this order, confirmed by rendering the whole
/// alphabet out of the extracted bitmap:
///
/// | indices | characters |
/// |---------|------------|
/// | 0 – 9   | `0` … `9`   |
/// | 10 – 35 | `A` … `Z`   |
/// | 36 – 50 | `a` … `o`   |
///
/// Characters beyond `o` simply do not exist in the original font.
///
/// The full table lives in [`crate::fonts::glyph_index`]; this is the simple
/// variant for callers that do not care about the slanted lower case.
pub fn glyph_index(c: char) -> Option<usize> {
    crate::fonts::glyph_index(c).map(|(idx, _)| idx)
}

/// Everything loaded from the `*.pak` containers.
#[derive(Debug)]
pub struct GameAssets {
    /// 800x600 RGBA court background.
    pub background: Vec<u8>,
    /// The menu's night version of the background: the original pre-computes a
    /// second, darkened copy of the beach (`(c & 0xF8F8F8F8) >> 3`) and the
    /// menu screens use that one, while the match uses the bright original.
    pub background_night: Vec<u8>,
    /// All sprite groups, in container order.
    pub sprites: Vec<SpriteSet>,
    /// Raw WAV files, ready to be handed to an audio device.
    pub sounds: Vec<Vec<u8>>,
    /// The three localisation tables (French, German, English).
    pub tables: Vec<TextTable>,
    /// True when a network transport is available. The original greys out its
    /// "Network Match" entry when DirectPlay is missing; VolleyForge does the
    /// same until the UDP transport exists.
    pub network_available: bool,
}

impl GameAssets {
    /// An empty set, used when the containers cannot be read.
    pub fn empty() -> Self {
        GameAssets {
            background: vec![0; VIEW_W as usize * VIEW_H as usize * 4],
            background_night: vec![0; VIEW_W as usize * VIEW_H as usize * 4],
            sprites: Vec::new(),
            sounds: Vec::new(),
            tables: Vec::new(),
            network_available: false,
        }
    }

    /// Load and decode every container in `dir`, in one go.
    ///
    /// The application itself uses the [`Loader`], so the captions of the
    /// original's loading screen are really visible. This shortcut is for the
    /// head-less modes (`--shot`, `--selftest`), which need the assets right
    /// away.
    pub fn load(dir: &Path) -> Result<GameAssets, String> {
        info!(dir = %dir.display(), "loading original assets");
        let mut loader = Loader::new(dir.to_path_buf());
        while !loader.step()? {}
        info!(
            sprites = loader.assets.sprites.len(),
            sounds = loader.assets.sounds.len(),
            tables = loader.assets.tables.len(),
            background_bytes = loader.assets.background.len(),
            "all original assets loaded into memory"
        );
        Ok(loader.assets)
    }

    /// The sprite group with the given frame size.
    pub fn set(&self, frame_w: usize, frame_h: usize) -> Option<&SpriteSet> {        self.sprites
            .iter()
            .find(|s| s.frame_w == frame_w && s.frame_h == frame_h)
    }

    /// Every sprite group with the given frame size.
    pub fn sets(&self, frame_w: usize, frame_h: usize) -> Vec<&SpriteSet> {
        self.sprites
            .iter()
            .filter(|s| s.frame_w == frame_w && s.frame_h == frame_h)
            .collect()
    }

    /// The localisation table of one language (0 = French, 1 = German, 2 = English).
    ///
    /// The original keeps the three tables of `text.pak` at `+0x9781FC + i*4` and
    /// picks the active one with `+0x978208`.
    pub fn table(&self, language: usize) -> Option<&TextTable> {
        self.tables.get(language)
    }

    /// The original's 55 slot font sheet: 51 letters plus 4 decorative
    /// elements that live in the second 24x24 group.
    pub fn font(&self) -> Option<crate::fonts::Font> {
        let groups = self.sets(24, 24);
        let letters = groups.iter().find(|s| s.bpp == 24)?;
        let decorations = groups.iter().find(|s| s.bpp == 8)?;
        Some(crate::fonts::Font::from_groups(
            &letters.pixels,
            &decorations.pixels,
        ))
    }

    /// The English localisation table.
    pub fn english(&self) -> Option<&TextTable> {
        self.tables.get(2)
    }
}

/// The captions of the original's loading screen.
///
/// The original changes the caption of its loading window while it unpacks the
/// containers (`FUN_004429CC`).
pub const LOAD_CAPTIONS: [&str; 5] = [
    "Initialisierung",
    "Entpacke Graphiken",
    "Entpacke Zeichensatz",
    "Sounds",
    "Texte",
];

/// The three containers the game needs.
pub const CONTAINERS: [&str; 3] = ["graph.pak", "sound.pak", "text.pak"];

/// The raw byte payloads of the three containers.
#[derive(Clone, Debug, Default)]
pub struct RawContainers {
    pub graph: Vec<u8>,
    pub sound: Vec<u8>,
    pub text: Vec<u8>,
}

/// Read one of the original's containers.
///
/// The desktop version reads it from the folder of the executable.
pub fn read_container(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (dir, name);
        Err(format!("{name} cannot be read from filesystem on wasm"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = dir.join(name);
        fs::read(&path).map_err(|e| {
            error!(file = name, path = %path.display(), error = %e, "container missing");
            format!(
                "{name} is missing.\n\nExpected it in:\n{}\n\n[ENTER] Quit",
                dir.display()
            )
        })
    }
}

/// The stepwise loader of the original's startup.
///
/// The original shows a loading window whose caption changes while it unpacks
/// the containers. The port does the same: one step per frame, so the captions
/// are really visible - the whole load takes about a tenth of a second.
pub struct Loader {
    /// Where the containers are expected: the folder of the executable.
    dir: PathBuf,
    /// In-memory containers (used in WebAssembly or tests).
    pub raw: Option<RawContainers>,
    /// The step that comes next.
    step: usize,
    /// The assets collected so far.
    pub assets: GameAssets,
    /// The font - ready from the "Entpacke Zeichensatz" step on.
    pub font: Option<crate::fonts::Font>,
    /// The message of the error screen, if a container is missing.
    pub error: Option<String>,
}

impl Loader {
    /// A loader for the containers in `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Loader {
            dir,
            raw: None,
            step: 0,
            assets: GameAssets::empty(),
            font: None,
            error: None,
        }
    }

    /// A loader that reads from in-memory raw containers.
    #[allow(dead_code)]
    pub fn from_raw(raw: RawContainers) -> Self {
        Loader {
            dir: PathBuf::new(),
            raw: Some(raw),
            step: 0,
            assets: GameAssets::empty(),
            font: None,
            error: None,
        }
    }

    /// The caption of the current step.
    pub fn caption(&self) -> &'static str {
        LOAD_CAPTIONS[self.step.min(LOAD_CAPTIONS.len() - 1)]
    }

    /// Do the next step. `Ok(true)` means: everything is loaded.
    pub fn step(&mut self) -> Result<bool, String> {
        match self.step {
            0 => {
                if let Some(raw) = &self.raw {
                    if raw.graph.is_empty() || raw.sound.is_empty() || raw.text.is_empty() {
                        return Err("One or more original containers are empty".to_string());
                    }
                    info!("all in-memory containers are available");
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    for name in CONTAINERS {
                        let path = self.dir.join(name);
                        if !path.is_file() {
                            let message = format!(
                                "{name} is missing.\n\nExpected it in:\n{}\n\n[ENTER] Quit",
                                self.dir.display()
                            );
                            error!(file = name, dir = %self.dir.display(), "container missing");
                            return Err(message);
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    return Err("no containers provided".to_string());
                }
            }
            1 => {
                let bytes = if let Some(raw) = &self.raw {
                    raw.graph.clone()
                } else {
                    read_container(&self.dir, "graph.pak")?
                };
                let decoded = pak::Assets::from_bytes(&bytes)?;
                self.assets = build_graphics(decoded);
            }
            2 => {
                // The font is a part of the graphics container - the original
                // cuts it out in its own step.
                self.font = self.assets.font();
                if self.font.is_none() {
                    return Err("the font sheet is missing in graph.pak".to_string());
                }
            }
            3 => {
                let bytes = if let Some(raw) = &self.raw {
                    raw.sound.clone()
                } else {
                    read_container(&self.dir, "sound.pak")?
                };
                let sounds: Vec<Vec<u8>> = pak::decode_chunks(&bytes)
                    .iter()
                    .filter(|c| c.len() > 4 && &c[0..4] == b"RIFF")
                    .cloned()
                    .collect();
                info!(count = sounds.len(), "sound effects ready");
                crate::sound::set_buffers(sounds.clone());
                self.assets.sounds = sounds;
            }
            4 => {
                let bytes = if let Some(raw) = &self.raw {
                    raw.text.clone()
                } else {
                    read_container(&self.dir, "text.pak")?
                };
                let tables: Vec<TextTable> = pak::decode_chunks(&bytes)
                    .iter()
                    .map(|c| parse_text_table(c))
                    .collect();
                for (i, t) in tables.iter().enumerate() {
                    info!(
                        table = i,
                        sections = t.sections.len(),
                        "localisation table ready"
                    );
                }
                self.assets.tables = tables;
            }
            _ => {}
        }
        self.step += 1;
        Ok(self.step >= LOAD_CAPTIONS.len())
    }
}

/// Decode the graphics container into the assets.
fn build_graphics(decoded: pak::Assets) -> GameAssets {
    let mut sprites: Vec<SpriteSet> = Vec::new();
    for g in &decoded.groups {
        // Transparency rule of the original:
        //   * the 64x64 sprites (the ball) use a bright cut-off of 0xB4
        //   * everything else uses the plain 0x28 cut-off
        // The font is drawn through `crate::fonts`, which applies the
        // original's own `(pixel & 0xE0E0E0E0)` rule to the letter strokes.
        // The credits document is the exception: `FUN_00448E70` copies its
        // pixels raw, without a transparency test, so it stays opaque.
        let threshold = if g.frame_w == 64 && g.frame_h == 64 {
            0xB4
        } else if g.frame_w == 400 && g.frame_h == 300 {
            0
        } else {
            0x28
        };
        let set = SpriteSet {
            name: g.name.clone(),
            frame_w: g.frame_w,
            frame_h: g.frame_h,
            count: g.count,
            bpp: g.bpp,
            // The sheets are stored as BGRA; the canvas works in RGBA.
            // Without this conversion red and blue would be swapped and
            // the blue sky would turn into a red sunset.
            pixels: {
                let mut rgba = Vec::with_capacity(g.pixels.len());
                for i in 0..g.count {
                    rgba.extend_from_slice(&pak::to_rgba(
                        g.frame(i),
                        g.frame_w,
                        g.frame_h,
                        0,
                    ));
                }
                rgba
            },
        }
        .with_threshold(threshold);
        debug!(
            name = %set.name,
            frames = set.count,
            w = set.frame_w,
            h = set.frame_h,
            bpp = set.bpp,
            "sprite group ready"
        );
        sprites.push(set);
    }

    // The raw sheet is BGRA, the canvas works in RGBA.
    let background = pak::to_rgba(&decoded.background, 800, 600, 0);

    // The menu uses a pre-computed, darkened copy of the beach:
    //     (c & 0xF8F8F8F8) >> 3
    // (`FUN_00445AA0` writes it to `+0x3A983C`, the match uses the bright
    // original at `+0x1D4C3C`).
    let background_night: Vec<u8> = background
        .chunks(4)
        .flat_map(|p| {
            [
                (p[0] & 0xF8) >> 3,
                (p[1] & 0xF8) >> 3,
                (p[2] & 0xF8) >> 3,
                p[3],
            ]
        })
        .collect();

    GameAssets {
        background,
        background_night,
        sprites,
        sounds: Vec::new(),
        tables: Vec::new(),
        network_available: false,
    }
}
