//! Application shell: the screen machine and the match.
//!
//! Every menu screen is drawn into an 800x600 software canvas, which is
//! uploaded as a texture - the same architecture the original used with its
//! DirectDraw surface. The screens themselves live in [`crate::screens`] and
//! know nothing about egui.

use web_time::{Duration, Instant};

use egui::{Color32, Context, Key, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};

use crate::assets::GameAssets;
use crate::canvas::Canvas;
use crate::court::{PHYSICS_HZ, VIEW_H, VIEW_W};
use crate::fonts::Font;
use crate::log::{error, info, trace, warn};
use crate::player::{Control, Intent};
use crate::render;
use crate::rules::Match;
use crate::screens::{self, Action, Ctx, Input, Screen};
use crate::settings::Settings;

/// What the application is doing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// In the web build: waiting for the user to upload / drop original assets.
    #[cfg(target_arch = "wasm32")]
    NeedAssets,
    /// The original's loading screen: the containers are being unpacked.
    Loading,
    /// A container is missing - the error screen is shown.
    Failed,
    /// The game.
    Playing,
}

pub struct VolleyForge {
    pub assets: GameAssets,
    pub font: Font,
    pub settings: Settings,
    pub screen: Screen,
    pub game: Match,
    /// Draws the match screen into the canvas.
    pub renderer: render::MatchRenderer,
    canvas: Canvas,
    texture: Option<TextureHandle>,
    paused: bool,
    fps: f32,
    acc: f64,
    last: Instant,
    /// Start of the session - the original's `GetTickCount()`.
    start: Instant,
    /// The language the settings file was last written with.
    saved_language: usize,
    /// The startup: the loader of the original's loading screen.
    pub loader: crate::assets::Loader,
    /// What the application is doing.
    pub phase: Phase,
    #[cfg(target_arch = "wasm32")]
    pub web_assets: crate::web_assets::WebAssetManager,
}

impl Default for VolleyForge {
    fn default() -> Self {
        Self::new()
    }
}

impl VolleyForge {
    pub fn new() -> Self {
        let settings = Settings::load(&crate::settings::settings_path());
        let saved_language = settings.language;

        VolleyForge {
            // The containers are unpacked step by step, while the loading
            // screen is up - see [`Phase::Loading`].
            assets: GameAssets::empty(),
            font: Font::from_groups(&[], &[]),
            settings,
            screen: Screen::Menu(screens::menu::MenuState::default()),
            game: Match::new(0x5EED_1234, None),
            renderer: render::MatchRenderer::new(),
            canvas: Canvas::new(),
            texture: None,
            paused: false,
            fps: 0.0,
            acc: 0.0,
            last: Instant::now(),
            start: Instant::now(),
            saved_language,
            #[cfg(not(target_arch = "wasm32"))]
            loader: crate::assets::Loader::new(crate::pak::default_dir()),
            #[cfg(not(target_arch = "wasm32"))]
            phase: Phase::Loading,
            #[cfg(target_arch = "wasm32")]
            loader: if let Some(cached) = crate::web_assets::load_cached_assets() {
                crate::assets::Loader::from_raw(cached)
            } else {
                crate::assets::Loader::new(std::path::PathBuf::new())
            },
            #[cfg(target_arch = "wasm32")]
            phase: if crate::web_assets::load_cached_assets().is_some() {
                Phase::Loading
            } else {
                Phase::NeedAssets
            },
            #[cfg(target_arch = "wasm32")]
            web_assets: crate::web_assets::WebAssetManager::new(),
        }
    }

    /// The original's `GetTickCount()`, in milliseconds.
    pub fn tick_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn advance_time(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        self.fps = (1.0 / dt.max(1e-6)) as f32;
        self.acc = (self.acc + dt).min(0.25);
        let step = 1.0 / PHYSICS_HZ as f64;
        while self.acc >= step {
            self.acc -= step;
            if matches!(self.screen, Screen::Game) {
                let before = self.game.state;
                self.game.step();
                // A match that just ended goes into the statistics table.
                if before != self.game.state {
                    if let crate::rules::MatchState::Finished(winner) = self.game.state {
                        self.record_match(winner);
                    }
                }
                self.game.trace_frame(self.tick_ms());
            }
        }
    }

    /// Write the result of a finished match into the statistics.
    ///
    /// The original keeps a table of the players (name, games played, games
    /// won, points) and shows it in the statistics screen (`@stats`). The
    /// port keeps the table in its own settings file.
    fn record_match(&mut self, winner: usize) {
        for i in 0..2 {
            let name = self.settings.names[i].clone();
            let won = u32::from(i == winner);
            let points = self.game.players[i].score.max(0) as u32;
            match self.settings.stats.iter().position(|r| r.name == name) {
                Some(k) => {
                    let rec = &mut self.settings.stats[k];
                    rec.games += 1;
                    rec.won += won;
                    rec.score += points;
                }
                None => self.settings.stats.push(crate::settings::StatsRecord {
                    name,
                    games: 1,
                    won,
                    score: points,
                }),
            }
        }
        self.save_settings();
        info!(
            winner = winner + 1,
            records = self.settings.stats.len(),
            "match result written to the statistics"
        );
    }

    /// Map a pointer position into game pixels.
    fn to_game(&self, pos: Pos2, rect: Rect) -> (i32, i32) {
        let s = rect.width() / VIEW_W as f32;
        (
            ((pos.x - rect.min.x) / s).round() as i32,
            ((pos.y - rect.min.y) / s).round() as i32,
        )
    }

    /// Write the settings back to disk.
    pub fn save_settings(&self) {
        self.settings.save(&crate::settings::settings_path());
    }

    /// Upload the canvas as a texture and draw it.
    ///
    /// `load_texture_cached` is egui's documented way to update a texture
    /// every frame: it re-uploads only when the pixels actually changed and
    /// never allocates a second texture.
    fn present(&mut self, ui: &mut egui::Ui, ctx: &Context, rect: Rect) {
        let data = self.canvas.data();
        let mid = (300 * VIEW_W as usize + 400) * 4;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [VIEW_W as usize, VIEW_H as usize],
            data,
        );
        let tex_id = egui::Id::new("volleyforge_screen");
        let tex = ctx.load_texture_cached(
            "volleyforge_screen",
            tex_id,
            image,
            TextureOptions::NEAREST,
        );
        // Keep a clone alive: dropping the last handle would free the texture.
        self.texture = Some(tex.clone());
        trace!(
            bg_len = self.assets.background_night.len(),
            px_r = data[0],
            px_g = data[1],
            px_b = data[2],
            mid_r = data[mid],
            mid_g = data[mid + 1],
            mid_b = data[mid + 2],
            font_slots = self.font.len(),
            tex_size = ?tex.size(),
            "canvas upload"
        );

        let p = ui.painter();
        // The UV of an image is normalised: (0,0) .. (1,1) covers the whole
        // texture. `Rect::EVERYTHING` is an *infinite* rectangle meant for
        // clip rects - using it as a UV samples outside the texture and the
        // image comes out black.
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        p.image(tex.id(), rect, uv, Color32::WHITE);
    }
}

impl eframe::App for VolleyForge {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // --- the startup ----------------------------------------------------
        match self.phase {
            #[cfg(target_arch = "wasm32")]
            Phase::NeedAssets => {
                if let Some(raw) = self.web_assets.update(&ctx) {
                    self.loader = crate::assets::Loader::from_raw(raw);
                    self.phase = Phase::Loading;
                } else {
                    crate::web_assets::draw_web_asset_upload(ui, &mut self.web_assets);
                }
                ctx.request_repaint();
                return;
            }
            Phase::Loading => {
                draw_loading(ui, self.loader.caption());
                // One step per frame, so the captions of the original's
                // loading screen are really visible.
                match self.loader.step() {
                    Ok(true) => {
                        self.assets = std::mem::replace(&mut self.loader.assets, GameAssets::empty());
                        self.font = self.loader.font.take().unwrap_or_else(|| {
                            warn!("the font sheet is missing, the menus will be empty");
                            Font::from_groups(&[], &[])
                        });
                        self.phase = Phase::Playing;
                        info!("all containers are loaded, the game starts");
                    }
                    Ok(false) => {}
                    Err(e) => {
                        error!(error = %e, "the containers could not be loaded");
                        self.loader.error = Some(e);
                        self.phase = Phase::Failed;
                    }
                }
                ctx.request_repaint();
                return;
            }
            Phase::Failed => {
                let message = self.loader.error.clone().unwrap_or_default();
                draw_error(ui, &message);
                let leave = ctx.input(|i| {
                    i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
                });
                if leave {
                    error!("the game is left, a container is missing");
                    quit(1);
                }
                return;
            }
            Phase::Playing => {}
        }

        self.advance_time();

        let available = ui.available_size();
        // In the window the canvas keeps its 800x600 pixels and sits in the
        // upper left corner. In fullscreen it is scaled up as far as the 4:3
        // format allows and centred in the screen.
        let (scale, offset) = if self.settings.fullscreen {
            let s = (available.x / VIEW_W as f32)
                .min(available.y / VIEW_H as f32)
                .max(0.2);
            let size = Vec2::new(VIEW_W as f32 * s, VIEW_H as f32 * s);
            (
                s,
                Vec2::new(
                    (available.x - size.x) / 2.0,
                    (available.y - size.y) / 2.0,
                ),
            )
        } else {
            ((available.x / VIEW_W as f32).min(1.0).max(0.2), Vec2::ZERO)
        };
        let size = Vec2::new(VIEW_W as f32 * scale, VIEW_H as f32 * scale);
        let (allocated, _) = ui.allocate_exact_size(size, Sense::click());
        let rect = Rect::from_min_size(allocated.min + offset, size);

        // --- collect the input of this frame --------------------------------
        let mut input = Input::default();
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = e
                {
                    input.keys.push(*key);
                }
            }
        });
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
            input.mouse = self.to_game(pos, rect);
        }
        input.clicked = ctx.input(|i| i.pointer.any_pressed());

        // F11 switches between the window and the fullscreen - in every
        // screen, exactly like the key of the original's window.
        if input.keys.contains(&egui::Key::F11) {
            self.settings.fullscreen = !self.settings.fullscreen;
            info!(fullscreen = self.settings.fullscreen, "F11");
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                self.settings.fullscreen,
            ));
            self.save_settings();
            input.keys.retain(|k| *k != egui::Key::F11);
        }

        // The original replaces the Windows cursor with its own 48x48 arrow on
        // the menu screens (`FUN_00447AF8` draws it into the canvas). In the
        // match the Windows cursor stays, because the mouse steers the second
        // player there.
        ctx.set_cursor_icon(if matches!(self.screen, Screen::Game) {
            egui::CursorIcon::Default
        } else {
            egui::CursorIcon::None
        });

        match self.screen {
            Screen::Game => self.draw_game(ui, &ctx, rect),
            _ => self.draw_screen(ui, &ctx, rect, &input),
        }

        ctx.request_repaint_after(Duration::from_millis(8));
    }
}

impl VolleyForge {
    /// Draw one of the menu screens.
    fn draw_screen(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        rect: Rect,
        input: &Input,
    ) {
        // --- let the screen react -------------------------------------------
        let action = {
            let Self {
                assets,
                font,
                settings,
                screen,
                canvas,
                ..
            } = self;
            let mut ctx_bundle = Ctx {
                assets,
                font,
                settings,
            };
            screens::render_screen(screen, canvas, &mut ctx_bundle, input)
        };

        match action {
            Action::None => {}
            Action::Switch(next) => {
                info!(screen = next.name(), "screen changed");
                // The original writes the 24 byte key block into
                // `settings.dat`; the port persists the settings as soon as
                // the key definition is done.
                if matches!(&self.screen, Screen::KeyDef(_))
                    && matches!(&next, Screen::Settings(_))
                {
                    self.save_settings();
                }
                self.screen = next;
            }
            Action::StartMatch => {
                info!("starting a match");
                self.game = Match::new(0x5EED_1234, None);
                // The team colours and the control modes are settings of the
                // original (`+0x97828B` / `+0x978224`), so the match has to
                // start with what the player picked in the settings screen.
                self.apply_settings_to_match();
                self.screen = Screen::Game;
            }
            Action::Quit => {
                info!("quitting");
                self.save_settings();
                std::process::exit(0);
            }
        }

        // The language switch writes straight to the settings, so persist it
        // as soon as it changes.
        if self.settings.language != self.saved_language {
            self.saved_language = self.settings.language;
            self.save_settings();
        }

        // --- upload the canvas ----------------------------------------------
        self.present(ui, ctx, rect);

        let p = ui.painter();
        p.text(
            rect.min + Vec2::new(8.0, rect.height() - 18.0),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "VolleyForge  |  {:.0} fps  |  ESC = back",
                self.fps
            ),
            egui::FontId::proportional(12.0),
            Color32::from_gray(170),
        );
        // Per frame logging is on the trace level, otherwise the log file
        // would grow by megabytes per minute.
        crate::log::trace!(screen = self.screen.name(), "menu screen drawn");
    }

    /// Draw the running match.
    ///
    /// The match is composed into the same 800x600 canvas the menus use, so
    /// it shows the original graphics: the beach, the two blobs in their team
    /// colours, the ball and the LED scoreboard.
    fn draw_game(&mut self, ui: &mut egui::Ui, ctx: &Context, rect: Rect) {
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if pressed(Key::F11) {
            self.paused = !self.paused;
            info!(paused = self.paused, "pause toggled");
        }
        // The original shows its own screen after the match
        // (`FUN_00449D3C`) and waits for a key press before it goes on.
        if matches!(self.game.state, crate::rules::MatchState::Finished(_))
            && ctx.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
            })
        {
            info!("match over - back to the main menu");
            self.save_settings();
            self.screen = Screen::Menu(screens::menu::MenuState::default());
            return;
        }
        // The original serves the next rally automatically after about
        // two seconds (FUN_004443E8:665-669, 751-757), there is no key
        // to start it early.
        if pressed(Key::Escape) {
            // The original asks before it leaves the match: the rally
            // handler returns -1 when the flag at `+0x97CE54` is set
            // (`BV_Phys_H_443e8.c:670-684`) and the game shows the
            // question "@nachfrage" ("Spiel wirklich beenden?"). Answering
            // it puts the game back into the main menu.
            info!("quit asked");
            self.screen = Screen::QuitAsk(screens::dialog::QuitAskState);
            return;
        }
        if pressed(Key::R) {
            self.game = Match::new(0x5EED_1234, None);
            self.apply_settings_to_match();
        }
        if pressed(Key::C) {
            self.game.drive_cpu(1);
        }

        // The pointer position in game pixels - the mouse marker needs it.
        let mouse = ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| self.to_game(p, rect));
        let mouse_down = ctx
            .input(|i| i.pointer.button_down(egui::PointerButton::Primary));

        // --- input per player ------------------------------------------------
        // The original reads a table of virtual key codes (`+0x97CE39`) for
        // the keyboard players and the mouse for the mouse players: the
        // target is `mouse_x / 2 + player_index * 400`, so one mouse steers
        // each player inside their own half. The left mouse button is the
        // jump.
        for i in 0..2 {
            match self.game.players[i].control {
                Control::Keyboard => {
                    let (left, right, jump) = (
                        self.settings.keys.get(i, 0),
                        self.settings.keys.get(i, 1),
                        self.settings.keys.get(i, 2),
                    );
                    self.game.set_intent(i, Intent {
                        left: ctx.input(|x| x.key_down(left)),
                        right: ctx.input(|x| x.key_down(right)),
                        jump: ctx.input(|x| x.key_down(jump)),
                    });
                }
                Control::Mouse => {
                    // The original's dead zone: the player only walks when
                    // the target is more than 7 pixels away.
                    if let Some((mx, _)) = mouse {
                        self.game.players[i].follow_mouse(i, mx);
                    }
                    self.game.players[i].intent.jump = mouse_down;
                }
                Control::Cpu => self.game.set_intent(i, Intent::default()),
            }
        }

        let names = [
            self.settings.display_name(0),
            self.settings.display_name(1),
        ];
        let tick = self.tick_ms();

        self.renderer.draw(
            &mut self.canvas,
            &self.assets,
            &self.font,
            &self.game,
            &names,
            mouse,
            tick,
            None,
        );

        // The original shows its own screen when the match is over
        // (`FUN_00449D3C`): the picture is darkened and the winner's name
        // appears above the word "wins".
        if let crate::rules::MatchState::Finished(winner) = self.game.state {
            let name = match self.game.players[winner].control {
                Control::Cpu => match self.game.players[winner].skill {
                    0 => "Adam".to_string(),
                    1 => "Bert".to_string(),
                    _ => "Carl".to_string(),
                },
                _ => names[winner].clone(),
            };
            let wins = self
                .assets
                .table(self.settings.language)
                .and_then(|t| t.entry("win", 0))
                .unwrap_or_default()
                .to_string();
            crate::render::draw_match_over(
                &mut self.canvas,
                &self.assets,
                &self.font,
                &name,
                &wins,
            );
        }

        self.present(ui, ctx, rect);
    }

    /// Copy the settings that belong to a match into the running match:
    /// the team colours and the control modes.
    pub fn apply_settings_to_match(&mut self) {
        // The original keeps the sound switch at `+0x97822C` and only plays
        // a sound when it is 1 (`BV_MessageByIndex.c:5`).
        crate::sound::set_enabled(self.settings.sound_on);
        for i in 0..2 {
            self.game.players[i].colour = self.settings.colors[i] as usize;
            let (control, skill) = match self.settings.control[i] {
                crate::settings::Control::Keyboard => (Control::Keyboard, 0),
                crate::settings::Control::Mouse => (Control::Mouse, 0),
                crate::settings::Control::CpuEasy => (Control::Cpu, 0),
                crate::settings::Control::CpuAvg => (Control::Cpu, 1),
                crate::settings::Control::CpuHard => (Control::Cpu, 2),
            };
            self.game.players[i].control = control;
            self.game.players[i].skill = skill;
        }
        info!(
            p1 = %self.settings.control[0].label(),
            p2 = %self.settings.control[1].label(),
            "match configured from the settings"
        );
    }
}

/// The loading screen of the original (`FUN_004429CC`).
///
/// The original draws the caption with its own font - but the font is a part
/// of `graph.pak`, so the port uses the system font until the containers are
/// unpacked. The game's font is only used from the main menu on.
fn draw_loading(ui: &mut egui::Ui, caption: &str) {
    let size = ui.available_size();
    ui.vertical_centered(|ui| {
        ui.add_space(size.y * 0.35);
        ui.label(
            egui::RichText::new(caption)
                .size(20.0)
                .color(egui::Color32::WHITE),
        );
    });
}

/// The error screen: one of the containers is missing.
///
/// The message is in English on purpose: the language of the game is not
/// known yet - `text.pak` may be the very file that is missing.
fn draw_error(ui: &mut egui::Ui, message: &str) {
    let size = ui.available_size();
    ui.vertical_centered(|ui| {
        ui.add_space(size.y * 0.25);
        for line in message.lines() {
            ui.label(
                egui::RichText::new(line)
                    .size(16.0)
                    .color(egui::Color32::WHITE),
            );
        }
    });
}

/// Leave the application.
fn quit(code: i32) -> ! {
    #[cfg(target_arch = "wasm32")]
    {
        // The browser has no process to exit. The web version carries the
        // containers inside the wasm module, so it never gets here.
        panic!("the game cannot continue");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::process::exit(code);
    }
}