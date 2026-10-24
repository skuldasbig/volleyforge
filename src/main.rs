//! VolleyForge — an independent reimplementation of the volley game shipped in
//! `blobb/volley.exe` (Blobby Volley v1.7.4, 2001).
//!
//! Everything here was reconstructed from that binary's decompiled code; no
//! original source or third-party port was consulted. See README.md for the
//! mapping between recovered addresses and the modules in this crate.
//!
//! Modes:
//!   * `volleyforge`               — play
//!   * `volleyforge --selftest N`  — headless physics/log smoke test
//!   * `volleyforge --dump-assets` — decode `graph.pak` and write PNGs
//!
//! Logging: `logs/volleyforge.log` next to the executable. `VOLLEYFORGE_LOG`
//! selects the level (`info` default, `debug`, `trace`).

mod app;
mod assets;
mod ball;
mod canvas;
mod court;
mod fonts;
mod layout;
mod log;
mod logging;
mod pak;
mod player;
mod render;
mod rules;
mod screens;
mod settings;
mod sound;
#[cfg(target_arch = "wasm32")]
mod web_assets;

use crate::log::{error, info, warn};



// ---------------------------------------------------------------------------
// Format verification: what is actually inside each .pak?
// ---------------------------------------------------------------------------

/// Identify a payload by its leading bytes.
fn sniff(head: &[u8]) -> &'static str {
    if head.len() >= 2 && &head[0..2] == b"BM" {
        "BMP"
    } else if head.len() >= 8 && &head[0..8] == b"\x89PNG\r\n\x1a\n" {
        "PNG"
    } else if head.len() >= 4 && &head[0..4] == b"RIFF" {
        "RIFF (WAV/AVI)"
    } else if head.len() >= 3 && &head[0..3] == b"ID3" {
        "MP3/ID3"
    } else if head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0 {
        "MP3 frame"
    } else if head.len() >= 4 && &head[0..4] == b"\x01\x00\x00\x00" {
        "ICO/DIB"
    } else if head.len() >= 4 && &head[0..4] == b"\x89\x50\x4E\x47" {
        "PNG-ish"
    } else if head.is_empty() {
        "empty"
    } else {
        "unknown"
    }
}

/// Decode every `.pak` in the asset directory and report what each chunk
/// really contains.
fn verify_formats() {
    use std::collections::BTreeMap;

    let dir = crate::pak::default_dir();

    for name in ["graph.pak", "sound.pak", "text.pak"] {
        let path = dir.join(name);
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(e) => {
                warn!(file = name, error = %e, "cannot read");
                continue;
            }
        };

        info!(
            file = name,
            bytes = raw.len(),
            raw_magic = ?sniff(&raw[..raw.len().min(16)]),
            "container file"
        );

        let chunks = pak::decode_chunks(&raw);
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        let mut samples: Vec<String> = Vec::new();

        for (i, c) in chunks.iter().enumerate() {
            let tag = sniff(&c[..c.len().min(16)]);
            *tally.entry(tag).or_insert(0) += 1;
            if samples.len() < 4 {
                let hex: String = c.iter().take(16).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                samples.push(format!("#{i} {tag} [{hex}]"));
            }
        }

        info!(
            file = name,
            chunks = chunks.len(),
            formats = ?tally,
            samples = ?samples,
            "chunk format census"
        );

        // Text payloads are the localisation tables; keep them verbatim.
        if name == "text.pak" {
            let out = std::env::current_dir().unwrap_or_default().join("assets_dump");
            let _ = std::fs::create_dir_all(&out);
            for (i, c) in chunks.iter().enumerate() {
                let p = out.join(format!("text_{i}.txt"));
                if let Err(e) = std::fs::write(&p, c) {
                    warn!(file = name, error = %e, "cannot write text chunk");
                } else {
                    info!(file = name, chunk = i, bytes = c.len(), path = ?p, "text chunk written");
                }
            }
        }

                // Per-group BMP header of the *raw* decompressed payload.
        if name == "graph.pak" {
            let mut last: Option<(usize, usize, usize)> = None;
            for (i, c) in chunks.iter().enumerate() {
                if c.len() < 54 {
                    continue;
                }
                let hex: String = c.iter().take(16).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                let w = u32::from_le_bytes([c[18], c[19], c[20], c[21]]) as usize;
                let h = u32::from_le_bytes([c[22], c[23], c[24], c[25]]) as usize;
                let bpp = u16::from_le_bytes([c[28], c[29]]) as usize;
                let size = u32::from_le_bytes([c[2], c[3], c[4], c[5]]) as usize;
                let off = u32::from_le_bytes([c[10], c[11], c[12], c[13]]) as usize;
                let sig = (w, h, bpp);
                if last != Some(sig) {
                    last = Some(sig);
                    info!(
                        chunk = i,
                        magic = ?sniff(&c[..4]),
                        raw_head = %hex,
                        bf_size = size,
                        bf_offbits = off,
                        bi_width = w,
                        bi_height = h,
                        bi_bitcount = bpp,
                        "group start"
                    );
                }
            }
        }

        // Integrity: every chunk carries its own length, so the declared size
        // can be compared against what the LZW decoder actually produced.
        // A mismatch would mean the bitstream was decoded incorrectly.
        let mut exact = 0usize;
        let mut off_by: Vec<String> = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            let declared = match sniff(&c[..c.len().min(4)]) {
                "BMP" if c.len() >= 6 => {
                    Some(u32::from_le_bytes([c[2], c[3], c[4], c[5]]) as usize)
                }
                "RIFF (WAV/AVI)" if c.len() >= 8 => {
                    Some(u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as usize + 8)
                }
                _ => None,
            };
            match declared {
                Some(d) if d == c.len() => exact += 1,
                Some(d) => {
                    if off_by.len() < 6 {
                        off_by.push(format!("#{i} declared {d} != actual {}", c.len()));
                    }
                }
                None => {}
            }
        }
        info!(
            file = name,
            chunks = chunks.len(),
            size_exact = exact,
            mismatches = ?off_by,
            "declared-vs-actual size check"
        );

        // Write the audio payloads out as real WAV files.
        if name == "sound.pak" {
            let out = std::env::current_dir().unwrap_or_default().join("assets_dump");
            let _ = std::fs::create_dir_all(&out);
            for (i, c) in chunks.iter().enumerate() {
                let p = out.join(format!("sound_{i}.wav"));
                if let Err(e) = std::fs::write(&p, c) {
                    warn!(file = name, error = %e, "cannot write wav");
                } else {
                    // walk the RIFF chunk list for a quick structural check
                    let mut pos = 12usize;
                    let mut parts: Vec<String> = Vec::new();
                    while pos + 8 <= c.len() {
                        let id = String::from_utf8_lossy(&c[pos..pos + 4]).to_string();
                        let sz = u32::from_le_bytes([
                            c[pos + 4],
                            c[pos + 5],
                            c[pos + 6],
                            c[pos + 7],
                        ]) as usize;
                        parts.push(format!("{id}:{sz}"));
                        pos += 8 + sz + (sz & 1);
                    }
                    info!(file = name, chunk = i, bytes = c.len(), parts = ?parts, "wav written");
                }
            }
        }
    }
}

/// Render the screen the original shows after the match is over
/// (`FUN_00449D3C`) to a PNG, without opening a window.
fn shot_match_over() {
    let assets = load_assets_or_exit();
    let Some(font) = assets.font() else {
        error!("no font sheet found");
        return;
    };

    let mut game = rules::Match::new(0x5EED_1234, None);
    game.players[0].x = court::PLAYER_SPAWN_X[0];
    game.players[0].y = court::PLAYER_SPAWN_Y;
    game.players[1].x = court::PLAYER_SPAWN_X[1];
    game.players[1].y = court::PLAYER_SPAWN_Y;
    game.players[0].score = 15;
    game.players[1].score = 7;
    game.state = rules::MatchState::Finished(0);

    let names = ["Player 1".to_string(), "Player 2".to_string()];
    let mut canvas = canvas::Canvas::new();
    let mut renderer = render::MatchRenderer::new();
    renderer.draw(&mut canvas, &assets, &font, &game, &names, None, 0, Some(3));
    render::draw_match_over(&mut canvas, &assets, &font, "Player 1", "gewinnt");

    let img = canvas.to_image();
    let _ = img.save("shot_matchover.png");
    info!("match over screen written");
}

/// Render the match screen to a PNG, without opening a window.
///
/// The pose is fixed so that the result can be compared with
/// `mockup_game.png`: player 1 at x = 200 with 7 points, player 2 at x = 600
/// with 3 points and the ball right above the net.
fn shot_game() {    let assets = load_assets_or_exit();
    let Some(font) = assets.font() else {
        error!("no font sheet found");
        return;
    };

    let mut game = rules::Match::new(0x5EED_1234, None);
    game.players[0].x = court::PLAYER_SPAWN_X[0];
    game.players[0].y = court::PLAYER_SPAWN_Y;
    game.players[0].score = 7;
    game.players[0].colour = 0; // red
    game.players[1].x = court::PLAYER_SPAWN_X[1];
    game.players[1].y = court::PLAYER_SPAWN_Y;
    game.players[1].score = 3;
    game.players[1].colour = 3; // green
    game.ball.x = 400 * court::SUBPX;
    // The original's ball position is a **bottom-up** canvas row: 170 puts
    // the ball's centre at 600 - 170 = 430 in top-down coordinates, which is
    // the pose of `mockup_game.png`.
    game.ball.y = 170 * court::SUBPX;
    // The ball is in flight, so the gravity flag is set.
    game.ball.in_play = true;
    game.state = rules::MatchState::Rally;

    let names = ["PLAYER 1".to_string(), "PLAYER 2".to_string()];
    let mut canvas = canvas::Canvas::new();
    let mut renderer = render::MatchRenderer::new();
    // 0x100 makes the ball marker white, exactly like the mockup; frame 3 is
    // the rotation the mockup used.
    renderer.draw(&mut canvas, &assets, &font, &game, &names, None, 0x100, Some(3));

    let img = canvas.to_image();
    let out = std::env::current_dir().unwrap_or_default().join("shot_game.png");
    match img.save(&out) {
        Ok(()) => info!(path = %out.display(), "match screen written"),
        Err(e) => error!(
            path = %out.display(),
            error = %e,
            "cannot write the match screen"
        ),
    }
}

/// Render one of the menu screens to a PNG, without opening a window.
///
/// This is the head-less twin of the application: same assets, same screen
/// code, same layout - only the input is missing.
fn shot_screen(name: &str) {
    let assets = load_assets_or_exit();
    let Some(font) = assets.font() else {
        error!("no font sheet found");
        return;
    };
    let mut settings = settings::Settings::load(&settings::settings_path());
    // `--lang <0|1|2>` overrides the saved language (0 = French, 1 = German,
    // 2 = English) - handy for rendering every language head-less.
    let mut lang_override = false;
    let cli_args: Vec<String> = std::env::args().collect();
    if let Some(pos) = cli_args.iter().position(|a| a == "--lang") {
        if let Some(v) = cli_args.get(pos + 1).and_then(|v| v.parse::<usize>().ok()) {
            settings.language = v % settings::LANGUAGE_COUNT;
            lang_override = true;
        }
    }
    let mut canvas = canvas::Canvas::new();

    let mut screen = match name {
        "menu" => screens::Screen::Menu(screens::menu::MenuState::default()),
        "settings" => screens::Screen::Settings(screens::settings::SettingsState::default()),
        "keydef" => screens::Screen::KeyDef(screens::keydef::KeyDefState::default()),
        "stats" => screens::Screen::Stats(screens::stats::StatsState::default()),
        "multiplayer" => screens::Screen::Multiplayer(screens::multiplayer::MpState::options()),
        "nameinput" => screens::Screen::NameInput(screens::dialog::NameInputState::new(
            0,
            &settings.names[0].clone(),
        )),
        "quitask" => screens::Screen::QuitAsk(screens::dialog::QuitAskState::default()),
        "credits" => screens::Screen::Credits(screens::credits::CreditsState::default()),
        "originalcredits" => screens::Screen::OriginalCredits(
            screens::original_credits::OriginalCreditsState {
                // The head-less shot shows the picture in the middle - the
                // roll-in would need 6.6 seconds of wall clock to get there.
                phase: screens::original_credits::OriginalPhase::Still,
                ..Default::default()
            },
        ),
        "game" => {
            shot_game();
            return;
        }
        "matchover" => {
            shot_match_over();
            return;
        }
        other => {
            error!(screen = other, "unknown screen");
            return;
        }
    };

    // The key definition asks for the six keys one after the other. Without an
    // input the head-less shot would only show the first label, so the first
    // five keys are pressed here - that is the last frame the original shows
    // on this screen (the sixth key takes it back to the settings menu).
    if name == "keydef" {
        for key in [
            egui::Key::X,
            egui::Key::C,
            egui::Key::A,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ] {
            let input = screens::Input {
                keys: vec![key],
                mouse: (0, 0),
                clicked: false,
            };
            let mut ctx = screens::Ctx {
                assets: &assets,
                font: &font,
                settings: &mut settings,
            };
            screens::render_screen(&mut screen, &mut canvas, &mut ctx, &input);
        }
    }

    // The final frame - it comes after the last key press, so the screen
    // shows the state it would really have.
    let input = screens::Input::default();
    {
        let mut ctx = screens::Ctx {
            assets: &assets,
            font: &font,
            settings: &mut settings,
        };
        screens::render_screen(&mut screen, &mut canvas, &mut ctx, &input);
    }

    let img = canvas.to_image();
    let out = std::env::current_dir().unwrap_or_default().join(if lang_override {
        format!("shot_{}_lang{}.png", name, settings.language)
    } else {
        format!("shot_{}.png", name)
    });
    match img.save(&out) {
        Ok(()) => info!(
            path = %out.display(),
            screen = name,
            w = img.width(),
            h = img.height(),
            "screen rendered"
        ),
        Err(e) => error!(error = %e, "cannot write the screenshot"),
    }
}

/// Blit RGBA pixels into an image, skipping transparent ones.
fn blit(img: &mut image::RgbaImage, rgba: &[u8], w: usize, h: usize, x: i32, y: i32) {
    for sy in 0..h {
        for sx in 0..w {
            let i = (sy * w + sx) * 4;
            if i + 4 > rgba.len() {
                break;
            }
            if rgba[i + 3] == 0 {
                continue;
            }
            let dx = x + sx as i32;
            let dy = y + sy as i32;
            if dx < 0 || dy < 0 || dx >= 800 || dy >= 600 {
                continue;
            }
            img.put_pixel(
                dx as u32,
                dy as u32,
                image::Rgba([rgba[i], rgba[i + 1], rgba[i + 2], 255]),
            );
        }
    }
}

/// Font calibration: the alphabet once without overlap and once with the
/// original's 20 pixel advance, to verify the glyph order and the spacing.
fn font_calibration() {
    let assets = load_assets_or_exit();
    let mut cal = image::RgbaImage::from_pixel(800, 120, image::Rgba([0, 0, 0, 255]));
    let alpha = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut x = 4i32;
    for ch in alpha.chars() {
        if let Some(idx) = assets::glyph_index(ch) {
            if let Some(set) = assets.set(24, 24) {
                blit(&mut cal, set.frame(idx), 24, 24, x, 4);
            }
        }
        x += layout::GLYPH_ADVANCE;
    }
    let mut x = 4i32;
    for ch in alpha.chars() {
        if let Some(idx) = assets::glyph_index(ch) {
            if let Some(set) = assets.set(24, 24) {
                blit(&mut cal, set.frame(idx), 24, 24, x, 40);
            }
        }
        x += layout::GLYPH_ADVANCE;
    }
    let p = std::env::current_dir().unwrap_or_default().join("font_calibration.png");
    let _ = cal.save(&p);
    info!(path = %p.display(), "font calibration written");
}

/// Render the font on a white background, step by step, so that what is
/// broken in the sheet becomes visible instead of blending into the scene.
fn font_test() {
    let dir = crate::pak::default_dir();
    let assets = match assets::GameAssets::load(&dir) {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, "cannot load assets");
            return;
        }
    };
    let Some(set) = assets.set(24, 24) else {
        error!("no 24x24 font group found");
        return;
    };
    info!(
        group = %set.name,
        count = set.count,
        w = set.frame_w,
        h = set.frame_h,
        "font group"
    );

    let labels = vec![
        "Start Game".to_string(),
        "Network Match".to_string(),
        "Statistics".to_string(),
        "Settings".to_string(),
        "Francais".to_string(),
        "Quit".to_string(),
    ];

    // Row 1: as the menu draws it (the sprite set already carries the
    // transparency rule). Row 2: the very same glyphs, but with the alpha
    // channel forced opaque, so the true letter shape shows.
    let mut img = image::RgbaImage::from_pixel(800, 200, image::Rgba([255, 255, 255, 255]));

    let mut y = 4i32;
    for label in &labels {
        let mut x = 4i32;
        for ch in label.chars() {
            if let Some(idx) = assets::glyph_index(ch) {
                if idx < set.count {
                    blit(&mut img, set.frame(idx), 24, 24, x, y);
                }
            }
            x += 24;
        }
        y += 24;
    }

    // Second block: opaque copy of the same glyphs.
    let mut opaque = set.pixels.clone();
    for p in (3..opaque.len()).step_by(4) {
        opaque[p] = 255;
    }
    let flat = assets::SpriteSet {
        name: set.name.clone(),
        frame_w: set.frame_w,
        frame_h: set.frame_h,
        count: set.count,
        bpp: set.bpp,
        pixels: opaque,
    };
    let mut y = 4i32;
    for label in &labels {
        let mut x = 400i32;
        for ch in label.chars() {
            if let Some(idx) = assets::glyph_index(ch) {
                if idx < flat.count {
                    blit(&mut img, flat.frame(idx), 24, 24, x, y);
                }
            }
            x += 24;
        }
        y += 24;
    }

    let p = std::env::current_dir().unwrap_or_default().join("font_test.png");
    let _ = img.save(&p);
    info!(path = %p.display(), "font test written");

    // The raw, unfiltered frames of the 24 bpp group, drawn 3x on white.
    // This is the ground truth for what the sheet actually contains.
    if let Ok(decoded) = pak::Assets::from_bytes(&read_raw_pak(&dir, "graph.pak")) {
        for g in &decoded.groups {
            if g.frame_w != 24 || g.frame_h != 24 || g.bpp != 24 {
                continue;
            }
            let mut sheet = image::RgbaImage::from_pixel(17 * 72, 3 * 72, image::Rgba([255, 255, 255, 255]));
            for i in 0..g.count.min(51) {
                let col = i % 17;
                let row = i / 17;
                let stride = g.frame_w * g.frame_h * 4;
                let frame = &g.pixels[i * stride..(i + 1) * stride];
                for sy in 0..24 {
                    for sx in 0..24 {
                        let k = (sy * 24 + sx) * 4;
                        if frame[k + 3] == 0 {
                            continue;
                        }
                        let px = col * 72 + sx * 3;
                        let py = row * 72 + sy * 3;
                        if px + 3 < sheet.width() as usize && py + 3 < sheet.height() as usize {
                            for dy in 0..3 {
                                for dx in 0..3 {
                                    let mut d = sheet.get_pixel((px + dx) as u32, (py + dy) as u32).0;
                                    d[0] = frame[k];
                                    d[1] = frame[k + 1];
                                    d[2] = frame[k + 2];
                                    d[3] = 255;
                                    sheet.put_pixel((px + dx) as u32, (py + dy) as u32, image::Rgba(d));
                                }
                            }
                        }
                    }
                }
            }
            let p2 = std::env::current_dir().unwrap_or_default().join("font_raw_sheet.png");
            let _ = sheet.save(&p2);
            info!(path = %p2.display(), "raw 24bpp sheet written");
            break;
        }
    }
}

/// Read a container straight from disk, ignoring the embedded copy.
fn read_raw_pak(dir: &std::path::Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    logging::init();
    log::install_panic_hook();
    logging::banner(env!("CARGO_PKG_VERSION"));

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--dump-assets") {
        dump_assets();
        return Ok(());
    }
    if args.iter().any(|a| a == "--dump-bg-bmp") {
        dump_bg_bmp();
        return Ok(());
    }
    if args.iter().any(|a| a == "--verify") {
        verify_formats();
        return Ok(());
    }
    if args.iter().any(|a| a == "--fonttest") {
        font_test();
        return Ok(());
    }
    if args.iter().any(|a| a == "--font") {
        font_calibration();
        return Ok(());
    }
    if let Some(pos) = args.iter().position(|a| a == "--shot") {
        // "--shot" alone renders the main menu, "--shot <screen>" picks one.
        let name = args
            .get(pos + 1)
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .unwrap_or_else(|| "menu".to_string());
        shot_screen(&name);
        return Ok(());
    }
    if args.iter().any(|a| a == "--selftest") {
        let n: u64 = args
            .windows(2)
            .find(|w| w[0] == "--selftest")
            .and_then(|w| w[1].parse().ok())
            .unwrap_or(5_000);
        selftest(n);
        return Ok(());
    }

    court::log_recovered_constants();

    info!("opening window");
    // The window starts in the state the settings file remembers.
    let fullscreen = settings::Settings::load(&settings::settings_path()).fullscreen;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VolleyForge")
            .with_inner_size([800.0, 600.0])
            .with_resizable(true)
            .with_fullscreen(fullscreen)
            .with_min_inner_size([520.0, 400.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "VolleyForge",
        native_options,
        Box::new(|_cc| Ok(Box::new(app::VolleyForge::new()))),
    );

    match &result {
        Ok(()) => info!("window closed normally"),
        Err(e) => warn!(error = %e, "eframe returned an error"),
    }
    result
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(::log::LevelFilter::Debug).ok();
    court::log_recovered_constants();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("document missing");
        let canvas = document
            .get_element_by_id("volleyforge_canvas")
            .expect("canvas #volleyforge_canvas missing")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("not a canvas element");

        let runner = eframe::WebRunner::new();
        if let Err(e) = runner
            .start(
                canvas,
                web_options,
                Box::new(|_cc| Ok(Box::new(app::VolleyForge::new()))),
            )
            .await
        {
            log::error!("failed to start eframe: {e:?}");
        }
    });
}

/// Write the raw first chunk of `graph.pak` (the 800x600 background bitmap,
/// including its 256 colour palette) to disk.
fn dump_bg_bmp() {
    let dir = crate::pak::default_dir();
    let data = match std::fs::read(dir.join("graph.pak")) {
        Ok(d) => d,
        Err(e) => {
            error!(error = %e, "graph.pak not found");
            return;
        }
    };
    let chunks = pak::decode_chunks(&data);
    if chunks.is_empty() {
        error!("no chunks decoded");
        return;
    }
    let out = std::env::current_dir()
        .unwrap_or_default()
        .join("assets_dump")
        .join("chunk0.bmp");
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    match std::fs::write(&out, &chunks[0]) {
        Ok(()) => info!(path = %out.display(), bytes = chunks[0].len(), "wrote the raw background chunk"),
        Err(e) => error!(error = %e, "could not write chunk0.bmp"),
    }
}

/// Decode `graph.pak` and write every sheet out as a PNG for inspection.
fn dump_assets() {
    use image::RgbaImage;

    let dir = crate::pak::default_dir();
    let pak = dir.join("graph.pak");
    info!(file = %pak.display(), "decoding graph.pak");

    let assets = match pak::Assets::load(&pak) {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, "graph.pak could not be decoded");
            std::process::exit(1);
        }
    };

    let out = std::env::current_dir()
        .unwrap_or_default()
        .join("assets_dump");
    let _ = std::fs::create_dir_all(&out);
    info!(dir = %out.display(), "writing decoded sheets");

    // background: full 800x600
    let rgba = pak::to_rgba(&assets.background, 800, 600, 0);
    match RgbaImage::from_raw(800, 600, rgba) {
        Some(img) => {
            if let Err(e) = img.save(out.join("background.png")) {
                error!(error = %e, "background save failed");
            } else {
                info!("wrote background.png (800x600)");
            }
        }
        None => error!("background buffer size mismatch"),
    }

    // each sheet laid out as one long strip
    // every sprite group as a horizontal strip
    for g in &assets.groups {
        let w = g.frame_w * g.count;
        let mut strip = Vec::with_capacity(w * g.frame_h * 4);
        for i in 0..g.count {
            // The unpacker derives transparency from brightness, never from
            // the alpha byte: `b0 + b1 + b2 < threshold`.
            let threshold = if g.frame_w == 64 && g.frame_h == 64 { 0xB4 } else { 0x28 };
            strip.extend_from_slice(&pak::to_rgba(g.frame(i), g.frame_w, g.frame_h, threshold));
        }
        match RgbaImage::from_raw(w as u32, g.frame_h as u32, strip) {
            Some(img) => {
                let name = format!("{}.png", g.name);
                if let Err(e) = img.save(out.join(&name)) {
                    error!(name, error = %e, "save failed");
                } else {
                    info!(name, frames = g.count, w, h = g.frame_h, bpp = g.bpp, "wrote group");
                }
            }
            None => error!(name = %g.name, "strip size mismatch"),
        }

        // And the same strip without any transparency rule, so the real
        // pixels stay visible.
        let mut raw_strip = Vec::with_capacity(w * g.frame_h * 4);
        for i in 0..g.count {
            raw_strip.extend_from_slice(&pak::to_rgba(g.frame(i), g.frame_w, g.frame_h, 0));
        }
        if let Some(img) = RgbaImage::from_raw(w as u32, g.frame_h as u32, raw_strip) {
            let _ = img.save(out.join(format!("{}_raw.png", g.name)));
        }

        // The untouched source: every chunk is a complete Windows BMP, so it
        // can be written out byte for byte.
        for (i, bmp_bytes) in g.raw_frames.iter().enumerate() {
            let p = out.join(format!("{}_{i}.bmp", g.name));
            if let Err(e) = std::fs::write(&p, bmp_bytes) {
                warn!(name = %g.name, error = %e, "cannot write source BMP");
            }
        }
        info!(name = %g.name, frames = g.count, "wrote source BMPs");
    }
    // yellow-green cursor, so scan for that exact hue and report its bounding
    // box - the icon sheet row must contain it.
    let (mut min_y, mut max_y, mut min_x, mut max_x) = (usize::MAX, 0usize, usize::MAX, 0usize);
    let mut hits = 0u32;
    for y in 0..600 {
        for x in 0..800 {
            let i = (y * 800 + x) * 4;
            let r = assets.background[i + 2] as i32;
            let g = assets.background[i + 1] as i32;
            let b = assets.background[i] as i32;
            if g > 180 && r > 120 && b < 90 && g - b > 110 {
                hits += 1;
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    info!(
        hits,
        y = ?[min_y, max_y],
        x = ?[min_x, max_x],
        "yellow-green (cursor) pixels"
    );

    info!("asset dump finished");
}

/// Load the original's containers for a head-less run.
///
/// The head-less modes need the assets right away, so a missing container is a
/// hard error there: the message goes to the console and the program ends with
/// the exit code 1.
fn load_assets_or_exit() -> assets::GameAssets {
    let dir = crate::pak::default_dir();
    match assets::GameAssets::load(&dir) {
        Ok(a) => a,
        Err(e) => {
            error!(dir = %dir.display(), error = %e, "cannot load the original containers");
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}



/// Run the match headlessly for a fixed number of frames.
fn selftest(frames: u64) {
    court::log_recovered_constants();
    // The headless run must not play sounds.
    sound::set_enabled(false);

    info!(frames, "selftest: starting");
    let mut game = cpu_match(0x5EED_1234);

    let mut rallies = 0u32;
    let mut points = 0u32;
    let mut matches = 0u32;

    for f in 0..frames {
        // No "serve now" from outside: the selftest runs the same path as
        // the game - the next rally starts by itself once the afterglow is
        // over (`BV_Phys_H_443e8.c:665-669, 751-757`).
        game.step();
        game.trace_frame(0);

        match game.state {
            rules::MatchState::ServeReady => rallies += 1,
            rules::MatchState::Finished(_) => {
                matches += 1;
                info!(
                    matches,
                    final_score = ?[game.players[0].score, game.players[1].score],
                    "selftest observed a finished match"
                );
                game = cpu_match(0x5EED_1234 + f as u32);
            }
            rules::MatchState::Rally => {}
        }
        if game.last_point.is_some() {
            points += 1;
            game.last_point = None;
        }
    }

    info!(
        frames,
        rallies,
        points,
        matches,
        score = ?[game.players[0].score, game.players[1].score],
        "selftest finished"
    );
    if rallies == 0 {
        error!("selftest never produced a rally");
    }
}

/// A match in which **both** sides are computer opponents: player 1 is Carl
/// (hard), player 2 is Bert (average).
///
/// The selftest uses this helper for every new match, otherwise the second
/// match would be played by a human who never presses a key - the ball
/// would hang in the air and wait for the serve forever.
fn cpu_match(seed: u32) -> rules::Match {
    let mut m = rules::Match::new(seed, None);
    m.players[0].control = player::Control::Cpu;
    m.players[0].skill = 2;
    m.players[1].control = player::Control::Cpu;
    m.players[1].skill = 1;
    m
}
