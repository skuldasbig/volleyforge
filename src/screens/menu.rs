//! The main menu.
//!
//! The original's menu (`FUN_00447E0C`) draws **six** rows whose texts come
//! from the `@main` section of the *active* language:
//!
//! ```text
//!   piVar5 = *(int **)(param_1 + 0x9781fc + *(int *)(param_1 + 0x978208) * 4);
//!   (**(code **)(*piVar5 + 0xc))(piVar5, *(int *)(param_1 + 0x97cd8c) + uVar8, &local_10);
//! ```
//!
//! Row 4 is the language switch: it cycles the table index
//! (`(*(int *)(param_1 + 0x978208) + 1) % 3`) and the menu redraws itself - which
//! is why the fifth row reads „English Please" in German, „Francais" in English
//! and „Deutsch" in French.

use crate::canvas::Canvas;
use crate::court::VIEW_W;
use crate::fonts::TextStyle;
use crate::layout::{self, LOGO_POS, MENU_ITEM_Y, MENU_TEXT_CENTER_X};
use crate::log::{info, warn};
use crate::screens::{
    draw_cursor, draw_row, hit, multiplayer, settings as settings_screen, stats, Action, Ctx, Input,
    Screen,
};

/// The hint that F11 switches to the fullscreen, one text per language
/// (0 = French, 1 = German, 2 = English).
///
/// The original is always fullscreen and never mentions a key. VolleyForge
/// starts in a window, so the key stands in the upper right corner of the main
/// menu. The port's own texts cannot come from the original's `text.pak`,
/// because they do not exist there.
const FULLSCREEN_HINT: [&str; 3] = [
    "F11 Plein ecran",
    "F11 Vollbild",
    "F11 Fullscreen",
];

/// The row of the hint - the same one the logo stands on.
const HINT_Y: i32 = 8;

/// The distance of the hint to the right edge.
const HINT_MARGIN: i32 = 20;

/// State of the main menu.
#[derive(Clone, Debug, Default)]
pub struct MenuState {
    /// Index of the highlighted row.
    pub selected: usize,
}

/// The original's menu has six rows.
pub const ENTRY_COUNT: usize = 6;

/// The row that switches the language.
pub const LANGUAGE_ROW: usize = 4;

/// The `@main` texts, in the original's order. They are read from the active
/// language, so this list only documents the meaning of the rows.
pub const ENTRIES: [&str; ENTRY_COUNT] = [
    "Start Game",
    "Network Match",
    "Statistics",
    "Settings",
    "Francais",
    "Quit",
];

/// Draw the menu and act on the input.
pub fn render(
    state: &mut MenuState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    // --- background: the original's night copy of the beach ---------------
    canvas.copy_from(&ctx.assets.background_night);

    // --- logo -------------------------------------------------------------
    // The logo is an 8 bpp image, so the original paints it with the night
    // palette - just like the beach.
    if let Some(set) = ctx.assets.set(410, 230) {
        canvas.blit_night(set.frame(0), 410, 230, LOGO_POS.0, LOGO_POS.1);
    }

    // --- the six rows -----------------------------------------------------
    let mut rects = [(0, 0, 0, 0); ENTRY_COUNT];
    for i in 0..ENTRY_COUNT {
        let label = ctx.text("main", i);
        let y = layout::to_top_down(MENU_ITEM_Y[i], layout::LINE_HEIGHT);
        // "Network Match" cannot be used without a network transport, so it is
        // drawn greyed out - exactly what the original does without DirectPlay.
        let disabled = i == 1 && !ctx.assets.network_available;
        rects[i] = draw_row(
            canvas,
            ctx.font,
            &label,
            MENU_TEXT_CENTER_X,
            y,
            state.selected == i,
            disabled,
        );
    }

    // --- the fullscreen hint ------------------------------------------------
    // The original has no such line - it always runs in the fullscreen.
    // VolleyForge starts in a window, so the key stands in the upper right
    // corner, on the row of the logo.
    let hint = FULLSCREEN_HINT[ctx.settings.language.min(FULLSCREEN_HINT.len() - 1)];
    let hint_x = VIEW_W - ctx.font.width(hint) - HINT_MARGIN;
    ctx.font.draw(canvas, hint, hint_x, HINT_Y, TextStyle::Normal);

    // --- cursor ------------------------------------------------------------
    draw_cursor(canvas, ctx.assets, input.mouse);

    // --- input -------------------------------------------------------------
    if input.pressed(egui::Key::ArrowUp) {
        state.selected = (state.selected + ENTRY_COUNT - 1) % ENTRY_COUNT;
        info!(selected = state.selected, "menu selection moved up");
        crate::sound::sound_b();
    }
    if input.pressed(egui::Key::ArrowDown) {
        state.selected = (state.selected + 1) % ENTRY_COUNT;
        info!(selected = state.selected, "menu selection moved down");
        crate::sound::sound_b();
    }
    if input.clicked {
        for (i, rect) in rects.iter().enumerate() {
            if hit(*rect, input.mouse) {
                state.selected = i;
                return activate(state.selected, ctx);
            }
        }
    }
    if input.pressed(egui::Key::Enter) || input.pressed(egui::Key::Space) {
        return activate(state.selected, ctx);
    }
    if input.pressed(egui::Key::Escape) {
        // The original leaves the main menu with ESC
        // (`BV_Menu_00447E0C.c:107` jumps to the end of the menu loop), and
        // the credits follow before the program ends.
        info!("escape in the main menu, the credits follow");
        return Action::Switch(Screen::Credits(
            crate::screens::credits::CreditsState::default(),
        ));
    }
    Action::None
}

/// Act on the chosen entry.
///
/// The original plays a sound before it opens the screen
/// (`BV_Menu_00447E0C.c:110-135`): sound **A** for "match start" and
/// "quit", sound **B** for the other entries.
fn activate(index: usize, ctx: &mut Ctx) -> Action {
    info!(entry = index, "menu entry activated");
    match index {
        0 => {
            crate::sound::sound_a();
            Action::StartMatch
        }
        1 => {
            if ctx.assets.network_available {
                crate::sound::sound_b();
                Action::Switch(Screen::Multiplayer(multiplayer::MpState::options()))
            } else {
                warn!("network match selected, but networking is not implemented yet");
                Action::None
            }
        }
        2 => {
            crate::sound::sound_b();
            Action::Switch(Screen::Stats(stats::StatsState::default()))
        }
        3 => {
            crate::sound::sound_b();
            Action::Switch(Screen::Settings(settings_screen::SettingsState::default()))
        }
        4 => {
            crate::sound::sound_b();
            // The original: `*(int *)(param_1 + 0x978208) = (… + 1) % 3`
            ctx.settings.cycle_language();
            Action::None
        }
        5 => {
            crate::sound::sound_a();
            // The original ends the menu loop here (`do ... while (local_c != 5)`)
            // and the caller plays the credits before the program exits
            // (`0x44706D: call 0x448E70`). VolleyForge shows its own credits
            // first and hands over to the original's ones.
            Action::Switch(Screen::Credits(
                crate::screens::credits::CreditsState::default(),
            ))
        }
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hint exists in every language and only uses characters the
    /// original's font can draw.
    #[test]
    fn the_fullscreen_hint_can_be_drawn() {
        for hint in FULLSCREEN_HINT.iter() {
            for c in hint.chars() {
                if c.is_whitespace() {
                    // the space has no glyph, it only advances the cursor
                    continue;
                }
                assert!(
                    crate::fonts::glyph_index(c).is_some(),
                    "'{c}' is not in the original's font"
                );
            }
        }
        assert_ne!(FULLSCREEN_HINT[0], FULLSCREEN_HINT[1]);
        assert_ne!(FULLSCREEN_HINT[1], FULLSCREEN_HINT[2]);
        assert_eq!(FULLSCREEN_HINT[1], "F11 Vollbild");
    }

    /// The hint fits into the corner next to the logo.
    #[test]
    fn the_fullscreen_hint_fits() {
        let longest = FULLSCREEN_HINT
            .iter()
            .map(|h| h.chars().count())
            .max()
            .unwrap_or(0) as i32;
        assert!(longest * crate::layout::GLYPH_ADVANCE + HINT_MARGIN <= VIEW_W);
    }
}
