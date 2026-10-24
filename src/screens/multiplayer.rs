//! The multiplayer screens.
//!
//! The original walks through three screens:
//!
//! ```text
//!   options  (FUN_0044A8E0)  ->  host settings  (FUN_0044ABA8)
//!                           ->  client settings (FUN_0044B0B0)
//!                           ->  session list   (FUN_0044B530)
//!                           ->  waiting        (FUN_0044B8B4)
//! ```
//!
//! The screens are the original's; the transport behind them is VolleyForge's
//! UDP replacement for DirectPlay (see [`crate::net`]). The screens only say
//! *what* should happen - the application owns the socket.

use crate::canvas::Canvas;
use crate::fonts::TextStyle;
use crate::layout::{
    self, CLIENT_ITEM_X, CLIENT_ITEM_Y, HOST_ITEM_X, HOST_ITEM_Y, LINE_HEIGHT, MP_ITEM_X,
    MP_ITEM_Y, TITLE_CENTER_X, TITLE_Y,
};
use crate::log::info;
use crate::screens::{draw_cursor, draw_row, hit, Action, Ctx, Input, Screen};

/// Which multiplayer screen is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MpScreen {
    Options,
    Host,
    Client,
    Sessions,
    Waiting,
}

/// State of the multiplayer screens.
#[derive(Clone, Debug)]
pub struct MpState {
    pub screen: MpScreen,
    /// Index of the highlighted row.
    pub selected: usize,
    /// Game name the host entered.
    pub game_name: String,
    /// Address of the host the guest wants to join.
    pub host_address: String,
    /// Sessions the guest found.
    pub sessions: Vec<String>,
    /// The control mode of the local player: 0 = keyboard, 1 = mouse.
    ///
    /// The original switches between the two with `(mode + 1) & 1`
    /// (`BV_Menu_0044ABA8.c:171-178`); the computer opponents are not
    /// offered in a network game.
    pub control: usize,
}

impl MpState {
    pub fn options() -> MpState {
        MpState {
            screen: MpScreen::Options,
            selected: 0,
            game_name: String::new(),
            host_address: String::new(),
            sessions: Vec::new(),
            control: 0,
        }
    }

    /// The host settings of the original (`FUN_0044ABA8`).
    pub fn host() -> MpState {
        MpState {
            screen: MpScreen::Host,
            selected: 0,
            game_name: String::new(),
            host_address: String::new(),
            sessions: Vec::new(),
            control: 0,
        }
    }

    /// The client settings of the original (`FUN_0044B0B0`).
    pub fn client() -> MpState {
        MpState {
            screen: MpScreen::Client,
            selected: 0,
            game_name: String::new(),
            host_address: String::new(),
            sessions: Vec::new(),
            control: 0,
        }
    }

    /// The screen that waits for the other computer.
    ///
    /// The original shows it while the host waits for a guest
    /// (`FUN_0044B8B4`) and while the guest waits for the host
    /// (`FUN_0044BC94`).
    pub fn waiting() -> MpState {
        MpState {
            screen: MpScreen::Waiting,
            selected: 0,
            game_name: String::new(),
            host_address: String::new(),
            sessions: Vec::new(),
            control: 0,
        }
    }

    /// The list of the running sessions.
    pub fn sessions() -> MpState {
        MpState {
            screen: MpScreen::Sessions,
            selected: 0,
            game_name: String::new(),
            host_address: String::new(),
            sessions: Vec::new(),
            control: 0,
        }
    }
}

/// Draw the screen and act on the input.
pub fn render(
    state: &mut MpState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);

    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    let count = match state.screen {
        MpScreen::Options => {
            let title = ctx.text("mpsettings", 0);
            ctx.font.draw(
                canvas,
                &title,
                TITLE_CENTER_X - ctx.font.width(&title) / 2,
                layout::to_top_down(TITLE_Y, LINE_HEIGHT),
                TextStyle::Normal,
            );
            for i in 0..3 {
                let label = ctx.text("mpsettings", i + 1);
                let x = MP_ITEM_X[i];
                let y = layout::to_top_down(MP_ITEM_Y[i], LINE_HEIGHT);
                rects.push(draw_row(
                    canvas,
                    ctx.font,
                    &label,
                    x + ctx.font.width(&label) / 2,
                    y,
                    state.selected == i,
                    false,
                ));
            }
            3
        }
        MpScreen::Host => {
            let title = ctx.text("hostsettings", 0);
            ctx.font.draw(
                canvas,
                &title,
                TITLE_CENTER_X - ctx.font.width(&title) / 2,
                layout::to_top_down(TITLE_Y, LINE_HEIGHT),
                TextStyle::Normal,
            );
            for i in 0..4 {
                let label = ctx.text("hostsettings", i + 1);
                let x = HOST_ITEM_X[i];
                let y = layout::to_top_down(HOST_ITEM_Y[i], LINE_HEIGHT);
                let text = if i == 0 {
                    format!("{} {}", label, state.game_name)
                } else if i == 1 {
                    // The original shows the control mode of the local
                    // player (`@settings` 7 = keyboard, 8 = mouse).
                    format!("{} {}", label, ctx.text("settings", 7 + state.control))
                } else {
                    label
                };
                rects.push(draw_row(
                    canvas,
                    ctx.font,
                    &text,
                    x + ctx.font.width(&text) / 2,
                    y,
                    state.selected == i,
                    false,
                ));
            }
            4
        }
        MpScreen::Client => {
            let title = ctx.text("clientsettings", 0);
            ctx.font.draw(
                canvas,
                &title,
                TITLE_CENTER_X - ctx.font.width(&title) / 2,
                layout::to_top_down(TITLE_Y, LINE_HEIGHT),
                TextStyle::Normal,
            );
            for i in 0..4 {
                let label = ctx.text("clientsettings", i + 1);
                let x = CLIENT_ITEM_X[i];
                let y = layout::to_top_down(CLIENT_ITEM_Y[i], LINE_HEIGHT);
                let text = if i == 0 {
                    format!("{} {}", label, state.host_address)
                } else if i == 1 {
                    // The original shows the control mode of the local
                    // player (`@settings` 7 = keyboard, 8 = mouse).
                    format!("{} {}", label, ctx.text("settings", 7 + state.control))
                } else {
                    label
                };
                rects.push(draw_row(
                    canvas,
                    ctx.font,
                    &text,
                    x + ctx.font.width(&text) / 2,
                    y,
                    state.selected == i,
                    false,
                ));
            }
            4
        }
        MpScreen::Sessions => {
            let title = ctx.text("clientgamelist", 0);
            ctx.font.draw(
                canvas,
                &title,
                200,
                layout::to_top_down(250, LINE_HEIGHT),
                TextStyle::Normal,
            );
            if state.sessions.is_empty() {
                let empty = ctx.text("clientgamelist", 2);
                ctx.font.draw(canvas, &empty, 200, 200, TextStyle::Normal);
            } else {
                // The sessions the hosts announced - the original lists them
                // with the name of the session (`FUN_0044B530`).
                for (i, s) in state.sessions.iter().enumerate() {
                    let y = layout::to_top_down(250 - i as i32 * 40, LINE_HEIGHT);
                    rects.push(draw_row(
                        canvas,
                        ctx.font,
                        s,
                        300,
                        y,
                        state.selected == i,
                        false,
                    ));
                }
            }
            let back = ctx.text("clientgamelist", 1);
            rects.push(draw_row(
                canvas,
                ctx.font,
                &back,
                250,
                100,
                state.selected == state.sessions.len(),
                false,
            ));
            state.sessions.len() + 1
        }
        MpScreen::Waiting => {
            let title = ctx.text("hostwaiting", 0);
            ctx.font.draw(
                canvas,
                &title,
                TITLE_CENTER_X - ctx.font.width(&title) / 2,
                layout::to_top_down(TITLE_Y, LINE_HEIGHT),
                TextStyle::Normal,
            );
            let hint = ctx.text("hostwaiting", 1);
            ctx.font.draw(
                canvas,
                &hint,
                TITLE_CENTER_X - ctx.font.width(&hint) / 2,
                layout::to_top_down(TITLE_Y - 60, LINE_HEIGHT),
                TextStyle::Normal,
            );
            0
        }
    };

    draw_cursor(canvas, ctx.assets, input.mouse);

    // --- input -------------------------------------------------------------
    if input.pressed(egui::Key::ArrowUp) {
        state.selected = (state.selected + count.max(1) - 1) % count.max(1);
    }
    if input.pressed(egui::Key::ArrowDown) {
        state.selected = (state.selected + 1) % count.max(1);
    }
    if input.clicked {
        for (i, rect) in rects.iter().enumerate() {
            if hit(*rect, input.mouse) {
                state.selected = i;
                return activate(state, ctx);
            }
        }
    }
    if input.pressed(egui::Key::Enter) || input.pressed(egui::Key::Space) {
        return activate(state, ctx);
    }
    if input.pressed(egui::Key::Escape) {
        info!(screen = ?state.screen, "leaving the multiplayer screen");
        return match state.screen {
            MpScreen::Options => Action::Switch(Screen::Menu(
                crate::screens::menu::MenuState::default(),
            )),
            MpScreen::Host | MpScreen::Client | MpScreen::Sessions => {
                state.screen = MpScreen::Options;
                state.selected = 0;
                Action::None
            }
            MpScreen::Waiting => {
                state.screen = MpScreen::Host;
                state.selected = 0;
                Action::None
            }
        };
    }
    Action::None
}

/// Act on the chosen row.
fn activate(state: &mut MpState, _ctx: &mut Ctx) -> Action {
    info!(screen = ?state.screen, row = state.selected, "multiplayer row activated");
    match state.screen {
        MpScreen::Options => match state.selected {
            0 => {
                state.screen = MpScreen::Host;
                state.selected = 0;
                Action::None
            }
            1 => {
                state.screen = MpScreen::Client;
                state.selected = 0;
                Action::None
            }
            _ => Action::Switch(Screen::Menu(
                crate::screens::menu::MenuState::default(),
            )),
        },
        MpScreen::Host => match state.selected {
            0 => Action::Switch(Screen::NameInput(
                crate::screens::dialog::NameInputState::new(2, &state.game_name),
            )),
            1 => {
                // The original switches the control mode of the local player
                // between keyboard and mouse: `(mode + 1) & 1`
                // (`BV_Menu_0044ABA8.c:171-178`).
                Action::CycleNetControl
            }
            2 => {
                // The original opens the session here and waits for the
                // guest (`FUN_0044B8B4`).
                info!(name = %state.game_name, "the session is opened");
                Action::StartHosting(state.game_name.clone())
            }
            _ => {
                state.screen = MpScreen::Options;
                state.selected = 0;
                Action::None
            }
        },
        MpScreen::Client => match state.selected {
            0 => Action::Switch(Screen::NameInput(
                crate::screens::dialog::NameInputState::new(3, &state.host_address),
            )),
            1 => {
                // The original switches the control mode of the local player
                // between keyboard and mouse: `(mode + 1) & 1`
                // (`BV_Menu_0044B0B0.c`, dieselbe Zeile).
                Action::CycleNetControl
            }
            2 => {
                // The original enumerates the running sessions here
                // (`FUN_0044B530`).
                info!("the session list is searched");
                Action::FindSessions
            }
            _ => {
                state.screen = MpScreen::Options;
                state.selected = 0;
                Action::None
            }
        },
        MpScreen::Sessions => {
            if state.selected < state.sessions.len() {
                info!(session = state.selected, "a session is chosen");
                Action::JoinSession(state.selected)
            } else {
                state.screen = MpScreen::Options;
                state.selected = 0;
                Action::None
            }
        }
        MpScreen::Waiting => Action::None,
    }
}
