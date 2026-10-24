//! The key definition screen.
//!
//! `FUN_00447948` asks for the six keys **one after the other**: first the
//! three keys of player one, then the three of player two. There is no cursor,
//! no selection and no way to leave the screen - every step simply waits for
//! the next key press (`FUN_00447588`). A key that is already used by one of
//! the earlier slots is silently ignored and the screen waits for another one.
//!
//! The original stores Windows virtual key codes in the 24 byte block at
//! `0x97820C` (the first block of `settings.dat`) and prints them with its
//! own table (`FUN_004475AC`). VolleyForge keeps egui keys in its own
//! settings file, but the screen behaves and looks the same.

use crate::canvas::Canvas;
use crate::fonts::TextStyle;
use crate::layout::{self, LINE_HEIGHT};
use crate::log::{info, warn};
use crate::screens::settings::SettingsState;
use crate::screens::{Action, Ctx, Input, Screen};
use crate::settings::KeyBindings;

/// State of the key definition screen.
#[derive(Clone, Debug, Default)]
pub struct KeyDefState {
    /// The slot that is asked for next: 0..6 (player 0: 0..2, player 1: 3..5).
    pub step: usize,
    /// The settings screen this one was opened from. The original returns to
    /// the menu with its highlighted row unchanged.
    pub settings: SettingsState,
}

/// Canvas rows of the two player blocks (`FUN_00447948`).
const BLOCK_Y: [i32; 2] = [400, 250];

/// X of the action labels and X of the key names (`FUN_00447948`).
const LABEL_X: i32 = 100;
const VALUE_X: i32 = 500;

/// Distance between two rows of a player block.
const ROW_STEP: i32 = 32;

/// Number of slots: two players with three keys each.
const SLOTS: usize = 6;

/// Draw the screen and act on the input.
pub fn render(
    state: &mut KeyDefState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);

    // The original copies the background **once** and then draws the block
    // titles, the labels and the key names step by step - nothing is ever
    // cleared again. So we draw exactly the rows that the original has on
    // screen at this moment: the title of a block appears when the block
    // starts, a label when its slot is asked for and a key name after the
    // key was entered.
    let step = state.step;
    for player in 0..2 {
        if player * 3 > step {
            break;
        }
        let block_y = BLOCK_Y[player];
        ctx.font.draw(
            canvas,
            &ctx.text("keydef", player),
            LABEL_X,
            layout::to_top_down(block_y, LINE_HEIGHT),
            TextStyle::Normal,
        );
        for slot in 0..3 {
            let idx = player * 3 + slot;
            if idx > step {
                break;
            }
            let y = layout::to_top_down(block_y - ROW_STEP * (slot as i32 + 1), LINE_HEIGHT);
            ctx.font.draw(
                canvas,
                &ctx.text("keydef", slot + 2),
                LABEL_X,
                y,
                TextStyle::Normal,
            );
            if idx < step {
                let name = original_name(ctx.settings.keys.get(player, slot));
                ctx.font.draw(canvas, &name, VALUE_X, y, TextStyle::Normal);
            }
        }
    }

    // --- input -------------------------------------------------------------
    handle_input(state, &mut ctx.settings.keys, input)
}

/// Act on the input of one frame.
///
/// `FUN_00447588` waits for *any* key press. The original has no escape here:
/// ESC is an ordinary key code and is stored like every other key (and
/// printed as an empty line, because it is not in the name table).
fn handle_input(
    state: &mut KeyDefState,
    keys: &mut KeyBindings,
    input: &Input,
) -> Action {
    for key in &input.keys {
        // The original compares the new key with the slots 0..step-1 only
        // (0x447a48..0x447a6b), so a key may stay in its own slot.
        if keys.used_before(*key, state.step) {
            warn!(key = %key.name(), "that key is already in use");
            continue;
        }
        let player = state.step / 3;
        let slot = state.step % 3;
        keys.set(player, slot, *key);
        info!(player, slot, key = %key.name(), "key assigned");
        state.step += 1;
        if state.step >= SLOTS {
            // The original returns to the settings menu, which keeps its
            // state - the highlighted row is still the "DEFINE KEYS..." row.
            return Action::Switch(Screen::Settings(state.settings.clone()));
        }
        break; // one key per step
    }
    Action::None
}

/// The name of a key as the original prints it (`FUN_004475AC`).
///
/// The original prints the Windows virtual key with its own table. A key that
/// is not in that table - ESC for example - is printed as an empty string.
fn original_name(key: egui::Key) -> String {
    match key {
        egui::Key::Backspace => "backspc",
        egui::Key::ShiftLeft | egui::Key::ShiftRight => "Shift",
        egui::Key::ControlLeft | egui::Key::ControlRight => "Ctrl",
        egui::Key::AltLeft | egui::Key::AltRight => "Alt",
        egui::Key::CapsLock => "CapsLk",
        egui::Key::Space => "Space",
        egui::Key::PageUp => "pgup",
        egui::Key::PageDown => "pgdn",
        egui::Key::End => "end",
        egui::Key::Home => "home",
        egui::Key::ArrowLeft => "left",
        egui::Key::ArrowUp => "up",
        egui::Key::ArrowRight => "right",
        egui::Key::ArrowDown => "down",
        egui::Key::Insert => "ins",
        egui::Key::Delete => "del",
        // Digits and letters are printed as they are.
        _ => {
            let name = key.name();
            if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                name
            } else {
                // Every other key is not in the original's table and is
                // printed as an empty string.
                ""
            }
        }
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keys: &[egui::Key]) -> Input {
        Input {
            keys: keys.to_vec(),
            mouse: (0, 0),
            clicked: false,
        }
    }

    /// Six presses fill the six slots and return to the settings menu.
    #[test]
    fn asks_for_the_six_keys_one_after_the_other() {
        let mut state = KeyDefState::default();
        let mut keys = KeyBindings::default();
        let sequence = [
            egui::Key::Q,
            egui::Key::W,
            egui::Key::E,
            egui::Key::R,
            egui::Key::T,
            egui::Key::Z,
        ];
        for (i, key) in sequence.iter().enumerate() {
            let action = handle_input(&mut state, &mut keys, &press(&[*key]));
            assert_eq!(state.step, i + 1, "slot {i} was not filled");
            if i < 5 {
                assert!(matches!(action, Action::None), "the screen stays open");
            } else {
                // after the sixth key the original returns to the menu
                match action {
                    Action::Switch(Screen::Settings(_)) => {}
                    other => panic!("expected the settings screen, got {other:?}"),
                }
            }
        }
        assert_eq!(keys.get(0, 0), egui::Key::Q);
        assert_eq!(keys.get(0, 2), egui::Key::E);
        assert_eq!(keys.get(1, 0), egui::Key::R);
        assert_eq!(keys.get(1, 2), egui::Key::Z);
    }

    /// A key that an earlier slot uses is ignored and the screen waits.
    #[test]
    fn ignores_a_key_that_is_already_used() {
        let mut state = KeyDefState::default();
        let mut keys = KeyBindings::default();
        handle_input(&mut state, &mut keys, &press(&[egui::Key::Q]));
        assert_eq!(state.step, 1);
        // Q is now the key of slot 0 and must not be taken again
        handle_input(&mut state, &mut keys, &press(&[egui::Key::Q]));
        assert_eq!(state.step, 1, "a duplicate key was taken");
        handle_input(&mut state, &mut keys, &press(&[egui::Key::W]));
        assert_eq!(state.step, 2);
    }

    /// The slot that is filled right now is not part of the comparison, so a
    /// key may stay in its own slot.
    #[test]
    fn keeps_the_same_key_in_its_own_slot() {
        let mut state = KeyDefState::default();
        let mut keys = KeyBindings::default();
        // slot 1 holds C by default - pressing C again may keep it
        assert_eq!(keys.get(0, 1), egui::Key::C);
        handle_input(&mut state, &mut keys, &press(&[egui::Key::Q]));
        handle_input(&mut state, &mut keys, &press(&[egui::Key::C]));
        assert_eq!(state.step, 2, "the key of its own slot was rejected");
        assert_eq!(keys.get(0, 1), egui::Key::C);
    }

    /// The settings menu comes back with its highlighted row.
    #[test]
    fn returns_to_the_settings_menu_with_its_state() {
        let mut state = KeyDefState {
            step: 0,
            settings: SettingsState { selected: 4 },
        };
        let mut keys = KeyBindings::default();
        let mut action = Action::None;
        for key in [
            egui::Key::Q,
            egui::Key::W,
            egui::Key::E,
            egui::Key::R,
            egui::Key::T,
            egui::Key::Z,
        ] {
            action = handle_input(&mut state, &mut keys, &press(&[key]));
        }
        match action {
            Action::Switch(Screen::Settings(s)) => assert_eq!(s.selected, 4),
            other => panic!("expected the settings screen, got {other:?}"),
        }
    }

    /// The keys are printed with the original's own names (`FUN_004475AC`).
    #[test]
    fn prints_the_keys_the_way_the_original_does() {
        assert_eq!(original_name(egui::Key::ArrowLeft), "left");
        assert_eq!(original_name(egui::Key::ArrowRight), "right");
        assert_eq!(original_name(egui::Key::ArrowUp), "up");
        assert_eq!(original_name(egui::Key::ArrowDown), "down");
        assert_eq!(original_name(egui::Key::Backspace), "backspc");
        assert_eq!(original_name(egui::Key::CapsLock), "CapsLk");
        assert_eq!(original_name(egui::Key::PageUp), "pgup");
        assert_eq!(original_name(egui::Key::A), "A");
        assert_eq!(original_name(egui::Key::Num7), "7");
        // a key that is not in the original's table is printed as an empty
        // line
        assert_eq!(original_name(egui::Key::Escape), "");
        assert_eq!(original_name(egui::Key::F1), "");
    }
}
