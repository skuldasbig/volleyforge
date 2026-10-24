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
    /// The network session of a network match, if one is running.
    ///
    /// The host simulates the match and sends its state; the guest mirrors
    /// it and sends its input. See [`crate::net`].
    pub net: Option<crate::net::Session>,
    /// The score the host sent last - the score packet is only sent when
    /// something changed.
    net_score_sent: Option<(u8, u8, u8, u32, bool)>,
    /// The names of the two players: the local one and the one of the peer.
    net_names: [String; 2],
    /// The name of the hosted session.
    ///
    /// The original keeps it in the memory of the form (`+0x97CDD1`), so it
    /// survives every screen - but not the program. The port does the same:
    /// the field lives in the application, the screens only show it.
    session_name: String,
    /// The address of the host the guest wants to join (`HOST - IP:`).
    host_address: String,
    /// The control mode of the local player in the network menus:
    /// 0 = keyboard, 1 = mouse.
    ///
    /// The original keeps it in `+0x97CDE0` (in the memory of the form, not
    /// in `settings.dat`) and switches it with `(mode + 1) & 1`
    /// (`BV_Menu_0044ABA8.c:171-178`).
    net_control: usize,
    /// When the guest searched the last time for running sessions.
    net_last_discover: u64,
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
            net: None,
            net_score_sent: None,
            net_names: [String::new(), String::new()],
            session_name: String::new(),
            host_address: String::new(),
            net_control: 0,
            net_last_discover: 0,
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
                // The guest does not simulate: the original's guest loop
                // (`FUN_0044BF98`) never steps the physics, it only draws
                // the state the host sends.
                if !self.net_is_guest() {
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
        self.net_send_score();
    }

    // -----------------------------------------------------------------------
    // The network
    // -----------------------------------------------------------------------

    /// Do what a screen asked for.
    ///
    /// The screens are pure: they only say what should happen. The
    /// application owns the socket and the match, so everything that changes
    /// the state of the game happens here.
    pub(crate) fn handle_action(&mut self, action: Action) {
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
            Action::StartHosting(name) => {
                // The original opens a session with a random name ("game N")
                // and waits for the other computer (`FUN_0044B8B4`).
                let name = if name.trim().is_empty() {
                    format!("game {}", crate::net::udp::now_ticks() % 1000)
                } else {
                    name
                };
                self.session_name = name.clone();
                match crate::net::Session::host(name.clone(), crate::net::DEFAULT_PORT) {
                    Ok(mut session) => {
                        session.set_colour(self.settings.colors[0] as u8);
                        let addr = session
                            .local_addr()
                            .map(|a| a.to_string())
                            .unwrap_or_default();
                        let published = session.name().to_string();
                        self.net = Some(session);
                        self.screen = Screen::Multiplayer(screens::multiplayer::MpState::waiting());
                        info!(name = %published, %addr, "the session is open");
                    }
                    Err(e) => {
                        crate::log::error!(
                            error = %e,
                            port = crate::net::DEFAULT_PORT,
                            "the port is not free - no session can be opened"
                        );
                    }
                }
            }
            Action::FindSessions => {
                // The original enumerates the running sessions here
                // (`FUN_0044B530`); VolleyForge broadcasts a request.
                if self.create_guest_session() {
                    let sent = self.net.as_mut().map(|s| s.discover().is_ok()).unwrap_or(false);
                    if !sent {
                        warn!("the discovery broadcast failed");
                    }
                    self.screen = Screen::Multiplayer(screens::multiplayer::MpState::sessions());
                    info!("the session list is searched");
                }
            }
            Action::JoinSession(index) => {
                let mut joined = false;
                if let Some(session) = self.net.as_mut() {
                    if let Some(found) = session.found.get(index).cloned() {
                        match session.join(&found) {
                            Ok(()) => {
                                info!(host = %found.name, %found.addr, "the join request is sent");
                                joined = true;
                            }
                            Err(e) => warn!(error = %e, "the join request failed"),
                        }
                    }
                }
                if joined {
                    self.screen = Screen::Multiplayer(screens::multiplayer::MpState::waiting());
                }
            }
            Action::CycleNetControl => {
                // The original switches between keyboard and mouse:
                // `(mode + 1) & 1` (`BV_Menu_0044ABA8.c:171-178`).
                self.net_control = (self.net_control + 1) & 1;
                info!(
                    mode = if self.net_control == 0 { "keyboard" } else { "mouse" },
                    "the control mode of the network game changed"
                );
            }
            Action::NameConfirmed { target, text } => {
                // The dialog only knows the text; where it belongs is decided
                // here.
                match target {
                    crate::screens::dialog::NameTarget::Player1 => {
                        self.settings.names[0] = text;
                        self.screen = Screen::Settings(screens::settings::SettingsState::default());
                    }
                    crate::screens::dialog::NameTarget::Player2 => {
                        self.settings.names[1] = text;
                        self.screen = Screen::Settings(screens::settings::SettingsState::default());
                    }
                    crate::screens::dialog::NameTarget::GameName => {
                        // Back to the host screen, with the new name. The
                        // application keeps it, so it is still there after
                        // the next match.
                        self.session_name = text;
                        let mut mp = screens::multiplayer::MpState::host();
                        mp.game_name = self.session_name.clone();
                        info!(name = %self.session_name, "the name of the session");
                        self.screen = Screen::Multiplayer(mp);
                    }
                    crate::screens::dialog::NameTarget::HostAddress => {
                        // The original's client screen asks for the host's IP
                        // (`HOST - IP:`) and connects to it **directly**,
                        // without asking the lobby first.
                        self.host_address = text.trim().to_string();
                        let address = self.host_address.clone();
                        let mut mp = screens::multiplayer::MpState::client();
                        mp.host_address = address.clone();
                        self.screen = Screen::Multiplayer(mp);
                        match crate::net::parse_address(&address) {
                            Ok(addr) => {
                                if self.create_guest_session() {
                                    let sent = self
                                        .net
                                        .as_mut()
                                        .map(|s| s.join_addr(addr).is_ok())
                                        .unwrap_or(false);
                                    if sent {
                                        info!(%addr, "the join request is sent");
                                        self.screen = Screen::Multiplayer(
                                            screens::multiplayer::MpState::waiting(),
                                        );
                                    } else {
                                        warn!(%addr, "the join request failed");
                                    }
                                }
                            }
                            Err(e) => warn!(error = %e, address = %address, "the address is not usable"),
                        }
                    }
                }
            }
            Action::Quit => {
                info!("quitting");
                self.save_settings();
                std::process::exit(0);
            }
        }
    }

    /// Keep the name of the session and the address of the host in the
    /// multiplayer screens.
    ///
    /// The original keeps both in the memory of the form (`+0x97CDD1` for the
    /// name of the session), so they survive every screen - but not the
    /// program. The port does the same: the fields live in the application,
    /// the screens only show them.
    fn sync_multiplayer_fields(&mut self) {
        let kind = match &self.screen {
            Screen::Multiplayer(mp) => Some(mp.screen),
            _ => None,
        };
        match kind {
            Some(crate::screens::multiplayer::MpScreen::Host) => {
                if self.session_name.is_empty() {
                    // The original fills an empty name with a random
                    // "game N" (`BV_Menu_0044ABA8.c:57-64`).
                    let n = crate::rules::Rng::new(self.tick_ms() as u32 | 1).below(100);
                    self.session_name = format!("game {n}");
                    info!(name = %self.session_name, "the session gets a random name");
                }
                if let Screen::Multiplayer(mp) = &mut self.screen {
                    mp.game_name = self.session_name.clone();
                    mp.control = self.net_control;
                }
            }
            Some(crate::screens::multiplayer::MpScreen::Client) => {
                if let Screen::Multiplayer(mp) = &mut self.screen {
                    mp.host_address = self.host_address.clone();
                    mp.control = self.net_control;
                }
            }
            _ => {}
        }
    }

    /// Create the session of the guest and remember it.
    fn create_guest_session(&mut self) -> bool {
        match crate::net::Session::guest(crate::net::DEFAULT_PORT) {
            Ok(mut session) => {
                session.set_name(self.settings.display_name(1));
                session.set_colour(self.settings.colors[1] as u8);
                self.net = Some(session);
                true
            }
            Err(e) => {
                crate::log::error!(error = %e, "no socket for the session");
                false
            }
        }
    }

    /// Are we the host of a network match?
    pub(crate) fn net_is_host(&self) -> bool {
        self.net
            .as_ref()
            .map(|s| s.role() == crate::net::Role::Host)
            .unwrap_or(false)
    }

    /// Are we the guest of a network match?
    pub(crate) fn net_is_guest(&self) -> bool {
        self.net
            .as_ref()
            .map(|s| s.role() == crate::net::Role::Guest)
            .unwrap_or(false)
    }

    /// Read the network and apply what arrived.
    ///
    /// The original polls DirectPlay inside the rally handler
    /// (`FUN_00443E8`) and inside the guest's frame loop
    /// (`FUN_0044BF98`); the port polls the socket once per drawn frame.
    pub(crate) fn net_tick(&mut self) {
        let events = match self.net.as_mut() {
            Some(session) => session.poll(),
            None => Vec::new(),
        };
        for event in events {
            match event {
                crate::net::Event::Found(_) => {
                    // The list of the found sessions is shown by the screen.
                }
                crate::net::Event::Join { name, .. } => {
                    // The host: the guest is there, the match starts.
                    info!(guest = %name, "the guest joined - the match starts");
                    self.net_names[1] = name;
                    self.start_network_match(0);
                }
                crate::net::Event::Welcome {
                    slot,
                    host_colour,
                    host_name,
                } => {
                    // The guest: the host took us, the match starts.
                    info!(host = %host_name, "the host took us - the match starts");
                    self.net_names[0] = host_name;
                    self.game.players[0].colour = host_colour as usize;
                    self.start_network_match(slot);
                }
                crate::net::Event::State(state) => {
                    self.game.apply_net_state(&state);
                    if let Some(session) = self.net.as_ref() {
                        if let Some(ping) = session.ping() {
                            crate::log::debug!(ms = ping.as_millis() as u64, "round trip time");
                        }
                    }
                }
                crate::net::Event::Score {
                    p0, p1, server, over, ..
                } => {
                    self.game.apply_net_score(p0, p1, server, over);
                }
                crate::net::Event::Input { jump, left, right } => {
                    // The host applies the guest's input to the guest's blob
                    // (`BV_Phys_H_443e8.c:233-237`).
                    self.game.players[1].intent = Intent {
                        jump,
                        left,
                        right,
                    };
                }
                crate::net::Event::Sound(index) => {
                    crate::sound::play_index(index as usize);
                }
                crate::net::Event::Colour { player, colour } => {
                    self.game.players[player].colour = colour as usize;
                }
                crate::net::Event::Lost => {
                    warn!("the connection is lost");
                    self.net = None;
                    self.screen = Screen::Menu(screens::menu::MenuState::default());
                }
            }
        }

        // The session list of the guest's screen.
        if let Screen::Multiplayer(mp) = &mut self.screen {
            if let Some(session) = self.net.as_ref() {
                mp.sessions = session.found.iter().map(|s| s.name.clone()).collect();
            }
        }

        // The guest repeats the search while the session list is open, so
        // that a host that starts later is found as well.
        if let Screen::Multiplayer(mp) = &self.screen {
            if mp.screen == crate::screens::multiplayer::MpScreen::Sessions && self.net_is_guest() {
                let now = self.tick_ms();
                if now.saturating_sub(self.net_last_discover) > 500 {
                    self.net_last_discover = now;
                    if let Some(session) = self.net.as_mut() {
                        let _ = session.discover();
                    }
                }
            }
        }

        // Leaving the multiplayer screens closes the session.
        if self.net.is_some() && !matches!(self.screen, Screen::Multiplayer(_) | Screen::Game) {
            if let Some(mut session) = self.net.take() {
                session.say_goodbye();
                info!("the session is closed");
            }
        }
    }

    /// Start a network match.
    ///
    /// The local player keeps the settings, the other blob is steered by the
    /// other computer - the original's control mode `50`
    /// (`BV_Menu_0044B8B4.c:122` and `BV_Menu_0044BC94.c:64`).
    fn start_network_match(&mut self, local: usize) {
        self.game = Match::new(0x5EED_1234, None);
        self.apply_network_settings(local);
        self.net_score_sent = None;
        self.net_names[local] = self.settings.display_name(local);
        self.screen = Screen::Game;
        // Both sides announce their team colour, exactly like the original
        // (`BV_Menu_0044B8B4.c:124` and `BV_Menu_0044BC94.c:65`).
        let colour = self.settings.colors[local] as u8;
        if let Some(session) = self.net.as_mut() {
            let _ = session.send_colour(local, colour);
        }
        info!(player = local + 1, "network match started");
    }

    /// Copy the settings of the **local** player into the match and hand the
    /// other blob to the network.
    fn apply_network_settings(&mut self, local: usize) {
        crate::sound::set_enabled(self.settings.sound_on);
        // In a network game the local player is steered by keyboard or by
        // mouse - the mode of the network menu (`+0x97CDE0`), **not** the one
        // from the settings screen: there are no computer opponents in the
        // network menus (`BV_Menu_0044ABA8.c:171-178`).
        let control = if self.net_control == 0 {
            Control::Keyboard
        } else {
            Control::Mouse
        };
        self.game.players[local].control = control;
        self.game.players[local].skill = 0;
        self.game.players[local].colour = self.settings.colors[local] as usize;
        self.game.players[1 - local].control = Control::Network;
        self.game.players[1 - local].skill = 0;
        self.game.players[1 - local].colour = self.settings.colors[1 - local] as usize;
    }

    /// The host tells the guest which sound to play.
    ///
    /// The original plays the sound locally **and** sends it over the
    /// network (`BV_MessageByIndex.c:8-14`); the guest never touches the
    /// ball, so it only ever hears the sounds of the host.
    pub(crate) fn net_send_sound(&mut self) {
        if !self.net_is_host() {
            return;
        }
        let Some(index) = self.game.sound_request.take() else {
            return;
        };
        if let Some(session) = self.net.as_mut() {
            let _ = session.send_sound(index);
        }
    }

    /// The host sends the state of the match.
    ///
    /// The original sends it at the end of every frame
    /// (`BV_Phys_H_443e8.c:735-747`).
    pub(crate) fn net_send_state(&mut self) {
        if !self.net_is_host() {
            return;
        }
        let state = self.game.net_state();
        if let Some(session) = self.net.as_mut() {
            if let Err(e) = session.send_state(&state) {
                crate::log::debug!(error = %e, "the state could not be sent");
            }
        }
    }

    /// The guest sends its input to the host.
    ///
    /// The original sends it every frame (`BV_Menu_0044BF98.c:318-336`).
    pub(crate) fn net_send_input(&mut self) {
        if !self.net_is_guest() {
            return;
        }
        let intent = self.game.players[1].intent;
        if let Some(session) = self.net.as_mut() {
            let _ = session.send_input(intent.jump, intent.left, intent.right);
        }
    }

    /// The host sends the score when something changed.
    ///
    /// The original sends it when a rally starts (`BV_Phys_H_443e8.c:106`),
    /// when a point is scored (`:604`) and when the match ends
    /// (`BV_Menu_00445890.c:61`).
    pub(crate) fn net_send_score(&mut self) {
        if !self.net_is_host() {
            return;
        }
        let p0 = self.game.players[0].score as u8;
        let p1 = self.game.players[1].score as u8;
        let rallies = self.game.rallies_started;
        // Before the first serve nothing has happened yet - the original
        // sends its first score packet when the rally starts.
        if rallies == 0 {
            return;
        }
        let over = matches!(self.game.state, crate::rules::MatchState::Finished(_));
        let snapshot = (p0, p1, self.game.server as u8, rallies, over);
        if self.net_score_sent == Some(snapshot) {
            return;
        }
        let first = self.net_score_sent.is_none();
        self.net_score_sent = Some(snapshot);
        // During the first rally the service is not settled yet, so the
        // original announces no server at all (`0xFF`,
        // `BV_Phys_H_443e8.c:112-115`).
        let server = if first {
            0xFF
        } else {
            self.game.server as u8
        };
        let match_point = self.game.is_match_point();
        if let Some(session) = self.net.as_mut() {
            let _ = session.send_score(p0, p1, server, match_point, over);
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
        // The name of the session and the address of the host live in the
        // application, so that they survive the screens.
        self.sync_multiplayer_fields();
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

        self.handle_action(action);

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
                // A network player is steered by the other computer: its
                // intent arrives over the wire (`Cmd::Input`) and must not
                // be overwritten here.
                Control::Network => {}
            }
        }

        // --- the network ----------------------------------------------------
        // The host sends the state of the match, the guest its input - the
        // original does it at the end of its frame as well.
        self.net_send_input();
        self.net_send_state();
        self.net_send_sound();

        let names = if self.net.is_some() {
            // In a network match the names are the ones of the two players:
            // the local one and the one the other computer sent.
            self.net_names.clone()
        } else {
            [
                self.settings.display_name(0),
                self.settings.display_name(1),
            ]
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The name of the session survives the screens and the match.
    ///
    /// The original keeps it in the memory of the form (`+0x97CDD1`), so it is
    /// still there after the match has been left - only the program end wipes
    /// it.
    #[test]
    fn the_name_of_the_session_survives_the_match() {
        let mut app = VolleyForge::new();

        // The host screen gets a random name, exactly like the original
        // (`BV_Menu_0044ABA8.c:57-64`).
        app.screen = Screen::Multiplayer(screens::multiplayer::MpState::host());
        app.sync_multiplayer_fields();
        let generated = match &app.screen {
            Screen::Multiplayer(mp) => mp.game_name.clone(),
            _ => panic!("the host screen is expected"),
        };
        assert!(
            generated.starts_with("game "),
            "the random name is missing: {generated}"
        );

        // The player changes the name.
        app.handle_action(Action::NameConfirmed {
            target: crate::screens::dialog::NameTarget::GameName,
            text: "Meine Runde".to_string(),
        });

        // The match is played and left again.
        app.screen = Screen::Menu(screens::menu::MenuState::default());
        app.screen = Screen::Multiplayer(screens::multiplayer::MpState::host());
        app.sync_multiplayer_fields();
        match &app.screen {
            Screen::Multiplayer(mp) => assert_eq!(
                mp.game_name, "Meine Runde",
                "the name must survive the match"
            ),
            _ => panic!("the host screen is expected"),
        }
    }

    /// The control mode of the network menus switches between keyboard and
    /// mouse - the original does `(mode + 1) & 1`
    /// (`BV_Menu_0044ABA8.c:171-178`).
    #[test]
    fn the_control_mode_switches_between_keyboard_and_mouse() {
        fn mode(app: &VolleyForge) -> usize {
            match &app.screen {
                Screen::Multiplayer(mp) => mp.control,
                _ => panic!("the host screen is expected"),
            }
        }
        let mut app = VolleyForge::new();
        app.screen = Screen::Multiplayer(screens::multiplayer::MpState::host());
        app.sync_multiplayer_fields();
        assert_eq!(mode(&app), 0, "the keyboard comes first");

        app.handle_action(Action::CycleNetControl);
        app.sync_multiplayer_fields();
        assert_eq!(mode(&app), 1, "the mouse comes next");

        app.handle_action(Action::CycleNetControl);
        app.sync_multiplayer_fields();
        assert_eq!(mode(&app), 0, "and back to the keyboard");
    }

    /// In a network match the local player is steered by the mode of the
    /// network menu - not by the one of the settings screen, where a
    /// computer opponent could be selected.
    #[test]
    fn the_network_match_uses_the_mode_of_the_network_menu() {
        let mut app = VolleyForge::new();
        app.settings.control[0] = crate::settings::Control::CpuHard;
        app.net_control = 1;
        app.game = Match::new(0x5EED_1234, None);
        app.apply_network_settings(0);
        assert_eq!(app.game.players[0].control, Control::Mouse);
        assert_eq!(
            app.game.players[1].control,
            Control::Network,
            "the other blob is steered over the wire"
        );
    }
}