//! The settings screen.
//!
//! `FUN_004486FC` shows seven rows on the left, plus two 30x30 colour plates
//! that cycle the team colour of the two players. The rows are:
//!
//! ```text
//!   0  NAME:    (player 1)      4  DEFINE KEYS...
//!   1  CONTROL: (player 1)      5  SOUND:
//!   2  NAME:    (player 2)      6  OK
//!   3  CONTROL: (player 2)
//! ```
//!
//! The original's row positions come from `DAT_0044DB88`, the label indices
//! from `DAT_0044DBC0`, the colour plates from `DAT_0044DB80` / `DAT_0044DB84`.

use crate::canvas::Canvas;
use crate::layout::{self, SETTINGS_ITEM_Y, SETTINGS_X, SWATCH_SIZE, SWATCH_X, SWATCH_Y, TEAM_COLORS};
use crate::log::info;
use crate::screens::{draw_cursor, hit, Action, Ctx, Input, Screen};

/// State of the settings screen.
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    /// Index of the highlighted row (0..7).
    pub selected: usize,
}

/// The labels of the seven rows (`@settings` indices 2..8).
/// The `@settings` index of each row's label (the original's `DAT_0044DBC0`).
const LABEL_INDEX: [usize; 7] = [2, 3, 2, 3, 4, 5, 6];

/// Draw the settings screen and act on the input.
pub fn render(
    state: &mut SettingsState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);

    // --- the two colour plates --------------------------------------------
    for player in 0..2 {
        let top = layout::to_top_down(SWATCH_Y[player] - 3, SWATCH_SIZE);
        let color = TEAM_COLORS[ctx.settings.colors[player] as usize];
        // The plate itself: a 30x30 window onto the blob, in the team colour.
        if let Some(set) = ctx.assets.set(148, 101) {
            let frame = set.frame(1);
            let src_x = (148 - SWATCH_SIZE as usize) / 2;
            let src_y = (101 - SWATCH_SIZE as usize) / 2;
            for sy in 0..SWATCH_SIZE as usize {
                for sx in 0..SWATCH_SIZE as usize {
                    let i = ((src_y + sy) * 148 + src_x + sx) * 4;
                    if i + 4 > frame.len() {
                        break;
                    }
                    if frame[i + 3] == 0 {
                        continue;
                    }
                    let (r, g, b) = layout::tint_rgb(color, frame[i], frame[i + 1], frame[i + 2]);
                    canvas.put_pixel(SWATCH_X + sx as i32, top + sy as i32, r, g, b);
                }
            }
        }
        // the 1 px frame the original draws around the plate
        for x in SWATCH_X..SWATCH_X + SWATCH_SIZE {
            put(canvas, x, top, 0x40);
            put(canvas, x, top + SWATCH_SIZE - 1, 0x40);
        }
        for y in top..top + SWATCH_SIZE {
            put(canvas, SWATCH_X, y, 0x40);
            put(canvas, SWATCH_X + SWATCH_SIZE - 1, y, 0x40);
        }
    }

    // --- the small player icon of each block -------------------------------
    // The original draws the characters '<' and '>' here; those two slots of
    // the font are not characters at all but the two small player icons. They
    // are drawn **tinted** with the colour of the team (`FUN_0044A6F0`).
    for (player, marker) in ['<', '>'].iter().enumerate() {
        let y = layout::to_top_down(SWATCH_Y[player], layout::LINE_HEIGHT);
        ctx.font.draw_tinted(
            canvas,
            &marker.to_string(),
            SETTINGS_X,
            y,
            TEAM_COLORS[ctx.settings.colors[player] as usize],
        );
        // ... and the localised block title next to it (`@settings` 0 and 1).
        let title = ctx.text("settings", player);
        ctx.font.draw(
            canvas,
            &title,
            SWATCH_X + SWATCH_SIZE + 8,
            y,
            crate::fonts::TextStyle::Normal,
        );
    }

    // --- the seven rows ----------------------------------------------------
    let mut rects = [(0, 0, 0, 0); 7];
    for i in 0..7 {
        let y = layout::to_top_down(SETTINGS_ITEM_Y[i], layout::LINE_HEIGHT);
        // The left edge comes from the original's `DAT_0044DBA4`: the first
        // six rows sit at 220, the seventh one ("OK") at 370.
        let x = layout::SETTINGS_ITEM_X[i];
        let value = value_for(i, ctx);
        // The label indices come from the original (`DAT_0044DBC0` = 2,3,2,3,4,5,6)
        // and point into the `@settings` section of the active language.
        let label = ctx.text("settings", LABEL_INDEX[i]);
        let text = if value.is_empty() {
            label
        } else {
            format!("{}  {}", label, value)
        };
        let w = ctx.font.width(&text);
        let style = if state.selected == i {
            crate::fonts::TextStyle::Highlight
        } else {
            crate::fonts::TextStyle::Normal
        };
        ctx.font.draw(canvas, &text, x, y, style);
        rects[i] = (x, y, w, layout::LINE_HEIGHT);
    }

    draw_cursor(canvas, ctx.assets, input.mouse);

    // --- input -------------------------------------------------------------
    if input.pressed(egui::Key::ArrowUp) {
        state.selected = (state.selected + 6) % 7;
    }
    if input.pressed(egui::Key::ArrowDown) {
        state.selected = (state.selected + 1) % 7;
    }

    // The colour plates are mouse only, just like in the original.
    if input.clicked {
        for player in 0..2 {
            let top = layout::to_top_down(SWATCH_Y[player] - 3, SWATCH_SIZE);
            let mx = input.mouse.0;
            let my = input.mouse.1;
            if mx >= SWATCH_X - 4
                && mx <= SWATCH_X + SWATCH_SIZE + 4
                && my >= top - 4
                && my <= top + SWATCH_SIZE + 4
            {
                ctx.settings.cycle_color(player);
                return Action::None;
            }
        }
        for (i, rect) in rects.iter().enumerate() {
            if hit(*rect, input.mouse) {
                state.selected = i;
                return activate(state.selected, state, ctx);
            }
        }
    }

    if input.pressed(egui::Key::Enter) || input.pressed(egui::Key::Space) {
        return activate(state.selected, state, ctx);
    }
    if input.pressed(egui::Key::Escape) {
        info!("leaving the settings");
        return Action::Switch(Screen::Menu(crate::screens::menu::MenuState::default()));
    }
    Action::None
}

/// The value shown next to a row.
///
/// The control modes and the sound switch are localised in the original too
/// (`@settings` 7..13), the player names are not.
fn value_for(row: usize, ctx: &Ctx) -> String {
    match row {
        0 => ctx.settings.display_name(0),
        1 => ctx.text("settings", 7 + ctx.settings.control[0].index()),
        2 => ctx.settings.display_name(1),
        3 => ctx.text("settings", 7 + ctx.settings.control[1].index()),
        5 => {
            if ctx.settings.sound_on {
                ctx.text("settings", 12)
            } else {
                ctx.text("settings", 13)
            }
        }
        _ => String::new(),
    }
}

/// Act on the chosen row.
fn activate(row: usize, state: &SettingsState, ctx: &mut Ctx) -> Action {
    info!(row, "settings row activated");
    match row {
        0 => {
            if ctx.settings.control[0].is_cpu() {
                cycle_difficulty(ctx, 0);
                Action::None
            } else {
                Action::Switch(Screen::NameInput(
                    crate::screens::dialog::NameInputState::new(0, &ctx.settings.names[0]),
                ))
            }
        }
        1 => {
            ctx.settings.cycle_control(0);
            Action::None
        }
        2 => {
            if ctx.settings.control[1].is_cpu() {
                cycle_difficulty(ctx, 1);
                Action::None
            } else {
                Action::Switch(Screen::NameInput(
                    crate::screens::dialog::NameInputState::new(1, &ctx.settings.names[1]),
                ))
            }
        }
        3 => {
            ctx.settings.cycle_control(1);
            Action::None
        }
        4 => Action::Switch(Screen::KeyDef(
            // The key definition is a subroutine of this screen: it comes
            // back here with the highlighted row unchanged.
            crate::screens::keydef::KeyDefState {
                step: 0,
                settings: state.clone(),
            },
        )),
        5 => {
            ctx.settings.sound_on = !ctx.settings.sound_on;
            info!(sound = ctx.settings.sound_on, "sound toggled");
            Action::None
        }
        6 => Action::Switch(Screen::Menu(crate::screens::menu::MenuState::default())),
        _ => Action::None,
    }
}

/// Take a computer opponent back to a human, or change its difficulty.
///
/// The original advances the control mode **until it leaves the computer
/// range** (`FUN_004486FC`, lines 126-135):
///
/// ```c
/// if ((mode - 2U) < 3) {          // is it a computer mode?
///     while ((mode - 2U) < 3) {    // while it still is:
///         mode = (mode + 1) % 5;   //   advance
///         if (mode == 1 && other == 1) mode = 2;   // not both on the mouse
///     }
/// }
/// ```
///
/// So a click on the name of Adam walks it through average, hard and finally
/// to the keyboard - the port used to stop at the three computer modes.
fn cycle_difficulty(ctx: &mut Ctx, player: usize) {
    while ctx.settings.control[player].is_cpu() {
        ctx.settings.cycle_control(player);
        info!(
            player,
            mode = %ctx.settings.control[player].label(),
            "computer opponent changed"
        );
    }
}

/// Write a single grey pixel.
fn put(canvas: &mut Canvas, x: i32, y: i32, v: u8) {
    canvas.put_pixel(x, y, v, v, v);
}
