//! Small modal dialogs: the name input and the quit question.

use crate::canvas::Canvas;
use crate::fonts::TextStyle;
use crate::layout::{self, LINE_HEIGHT, TITLE_CENTER_X, TITLE_Y};
use crate::log::info;
use crate::screens::{draw_cursor, Action, Ctx, Input, Screen};

/// What a name input is used for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameTarget {
    /// The name of player one.
    Player1,
    /// The name of player two.
    Player2,
    /// The name of the hosted game.
    GameName,
    /// The address of the host to join.
    HostAddress,
}

/// Where the name input returns to.
///
/// The two player names come from the settings screen, the game name from
/// the host screen and the address from the client screen of the
/// multiplayer menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameReturn {
    /// The settings screen.
    Settings,
    /// The host screen of the multiplayer menu.
    HostScreen,
    /// The client screen of the multiplayer menu.
    ClientScreen,
}

/// State of the name input dialog.
#[derive(Clone, Debug)]
pub struct NameInputState {
    pub target: NameTarget,
    pub buffer: String,
    pub max_len: usize,
    /// The screen the dialog returns to.
    pub return_to: NameReturn,
}

impl NameInputState {
    pub fn new(player: usize, current: &str) -> NameInputState {
        let (target, max_len, return_to) = match player {
            0 => (NameTarget::Player1, 12, NameReturn::Settings),
            1 => (NameTarget::Player2, 12, NameReturn::Settings),
            2 => (NameTarget::GameName, 12, NameReturn::HostScreen),
            _ => (NameTarget::HostAddress, 20, NameReturn::ClientScreen),
        };
        NameInputState {
            target,
            buffer: current.to_string(),
            max_len,
            return_to,
        }
    }
}

/// Draw the dialog and act on the input.
pub fn render_name_input(
    state: &mut NameInputState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);

    let title = match state.target {
        NameTarget::Player1 | NameTarget::Player2 => ctx.text("settings", 2),
        NameTarget::GameName => ctx.text("hostsettings", 1),
        NameTarget::HostAddress => ctx.text("clientsettings", 1),
    };
    ctx.font.draw(
        canvas,
        &title,
        TITLE_CENTER_X - ctx.font.width(&title) / 2,
        layout::to_top_down(TITLE_Y, LINE_HEIGHT),
        TextStyle::Normal,
    );

    // the text being typed, with a blinking cursor behind it
    ctx.font.draw(
        canvas,
        &state.buffer,
        TITLE_CENTER_X - ctx.font.width(&state.buffer) / 2,
        layout::to_top_down(TITLE_Y - 60, LINE_HEIGHT),
        TextStyle::Highlight,
    );
    let cursor_x = TITLE_CENTER_X - ctx.font.width(&state.buffer) / 2 + state.buffer.chars().count() as i32 * 20;
    canvas.put_pixel(cursor_x, layout::to_top_down(TITLE_Y - 60, LINE_HEIGHT) + 4, 255, 255, 255);

    draw_cursor(canvas, ctx.assets, input.mouse);

    // --- input -------------------------------------------------------------
    for key in &input.keys {
        match key {
            egui::Key::Escape => {
                info!("cancelled the name input");
                return back(state);
            }
            egui::Key::Enter => {
                info!(target = ?state.target, text = %state.buffer, "name confirmed");
                // The application stores the text - the dialog does not know
                // whether it belongs into the settings or into the
                // multiplayer screens.
                return Action::NameConfirmed {
                    target: state.target,
                    text: state.buffer.clone(),
                };
            }
            egui::Key::Backspace => {
                state.buffer.pop();
            }
            _ => {
                if let Some(ch) = key_to_char(*key) {
                    if state.buffer.chars().count() < state.max_len {
                        state.buffer.push(ch);
                    }
                }
            }
        }
    }
    Action::None
}

/// Where the dialog returns to.
fn back(state: &NameInputState) -> Action {
    match state.return_to {
        NameReturn::Settings => Action::Switch(Screen::Settings(
            crate::screens::settings::SettingsState::default(),
        )),
        NameReturn::HostScreen => Action::Switch(Screen::Multiplayer(
            crate::screens::multiplayer::MpState::host(),
        )),
        NameReturn::ClientScreen => Action::Switch(Screen::Multiplayer(
            crate::screens::multiplayer::MpState::client(),
        )),
    }
}

/// State of the quit question.
///
/// The original asks this question when the escape key is pressed **inside
/// the match**: the rally handler returns -1 (`BV_Phys_H_443e8.c:670-684`),
/// the match is left and the game is back in the **main menu** - the
/// question itself comes from the table `@nachfrage` ("Spiel wirklich
/// beenden?"). Answering "no" continues the match.
#[derive(Clone, Debug, Default)]
pub struct QuitAskState;

/// Draw the quit question.
pub fn render_quit_ask(
    _state: &mut QuitAskState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);
    let text = ctx.text("nachfrage", 0);
    ctx.font.draw_centered(
        canvas,
        &text,
        TITLE_CENTER_X,
        layout::to_top_down(TITLE_Y, LINE_HEIGHT),
        TextStyle::Highlight,
    );
    draw_cursor(canvas, ctx.assets, input.mouse);

    if input.pressed(egui::Key::Enter) || input.pressed(egui::Key::Y) {
        // The original leaves the match and is back in the main menu - it
        // does not close the application.
        info!("quit confirmed - back to the main menu");
        return Action::Switch(Screen::Menu(
            crate::screens::menu::MenuState::default(),
        ));
    }
    if input.pressed(egui::Key::Escape) || input.pressed(egui::Key::N) {
        info!("quit cancelled");
        return Action::Switch(Screen::Game);
    }
    Action::None
}

/// The character a key produces, if it is a plain letter or digit.
fn key_to_char(key: egui::Key) -> Option<char> {
    match key {
        egui::Key::A => Some('A'),
        egui::Key::B => Some('B'),
        egui::Key::C => Some('C'),
        egui::Key::D => Some('D'),
        egui::Key::E => Some('E'),
        egui::Key::F => Some('F'),
        egui::Key::G => Some('G'),
        egui::Key::H => Some('H'),
        egui::Key::I => Some('I'),
        egui::Key::J => Some('J'),
        egui::Key::K => Some('K'),
        egui::Key::L => Some('L'),
        egui::Key::M => Some('M'),
        egui::Key::N => Some('N'),
        egui::Key::O => Some('O'),
        egui::Key::P => Some('P'),
        egui::Key::Q => Some('Q'),
        egui::Key::R => Some('R'),
        egui::Key::S => Some('S'),
        egui::Key::T => Some('T'),
        egui::Key::U => Some('U'),
        egui::Key::V => Some('V'),
        egui::Key::W => Some('W'),
        egui::Key::X => Some('X'),
        egui::Key::Y => Some('Y'),
        egui::Key::Z => Some('Z'),
        egui::Key::Num0 => Some('0'),
        egui::Key::Num1 => Some('1'),
        egui::Key::Num2 => Some('2'),
        egui::Key::Num3 => Some('3'),
        egui::Key::Num4 => Some('4'),
        egui::Key::Num5 => Some('5'),
        egui::Key::Num6 => Some('6'),
        egui::Key::Num7 => Some('7'),
        egui::Key::Num8 => Some('8'),
        egui::Key::Num9 => Some('9'),
        egui::Key::Space => Some(' '),
        egui::Key::Period => Some('.'),
        egui::Key::Minus => Some('-'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::multiplayer::MpScreen;

    fn press(keys: &[egui::Key]) -> Input {
        Input {
            keys: keys.to_vec(),
            mouse: (0, 0),
            clicked: false,
        }
    }

    /// The name of the session belongs to the host screen - not to the
    /// settings menu.
    #[test]
    fn the_game_name_returns_to_the_host_screen() {
        let assets = crate::assets::GameAssets::empty();
        let font = crate::fonts::Font::from_groups(&[], &[]);
        let mut settings = crate::settings::Settings::default();
        let mut canvas = crate::canvas::Canvas::new();
        let mut state = NameInputState::new(2, "");

        {
            let mut ctx = Ctx {
                assets: &assets,
                font: &font,
                settings: &mut settings,
            };
            let action = render_name_input(
                &mut state,
                &mut canvas,
                &mut ctx,
                &press(&[egui::Key::G, egui::Key::A, egui::Key::M, egui::Key::E]),
            );
            assert!(
                matches!(action, Action::None),
                "the dialog stays open while typing"
            );
        }
        {
            let mut ctx = Ctx {
                assets: &assets,
                font: &font,
                settings: &mut settings,
            };
            let action =
                render_name_input(&mut state, &mut canvas, &mut ctx, &press(&[egui::Key::Enter]));
            match action {
                Action::NameConfirmed { target, text } => {
                    assert_eq!(target, NameTarget::GameName);
                    assert_eq!(text, "GAME");
                }
                other => panic!("expected a confirmed name, got {other:?}"),
            }
        }
    }

    /// Cancelling the name of the session goes back to the host screen.
    #[test]
    fn cancelling_the_game_name_returns_to_the_host_screen() {
        let assets = crate::assets::GameAssets::empty();
        let font = crate::fonts::Font::from_groups(&[], &[]);
        let mut settings = crate::settings::Settings::default();
        let mut canvas = crate::canvas::Canvas::new();
        let mut state = NameInputState::new(2, "game");
        let mut ctx = Ctx {
            assets: &assets,
            font: &font,
            settings: &mut settings,
        };
        let action = render_name_input(&mut state, &mut canvas, &mut ctx, &press(&[egui::Key::Escape]));
        match action {
            Action::Switch(Screen::Multiplayer(mp)) => {
                assert_eq!(mp.screen, MpScreen::Host);
            }
            other => panic!("expected the host screen, got {other:?}"),
        }
    }

    /// The two player names still go into the settings.
    #[test]
    fn the_player_names_return_to_the_settings() {
        let assets = crate::assets::GameAssets::empty();
        let font = crate::fonts::Font::from_groups(&[], &[]);
        let mut settings = crate::settings::Settings::default();
        let mut canvas = crate::canvas::Canvas::new();
        let mut state = NameInputState::new(0, "");
        let mut ctx = Ctx {
            assets: &assets,
            font: &font,
            settings: &mut settings,
        };
        let action = render_name_input(&mut state, &mut canvas, &mut ctx, &press(&[egui::Key::Enter]));
        match action {
            Action::NameConfirmed { target, .. } => assert_eq!(target, NameTarget::Player1),
            other => panic!("expected a confirmed name, got {other:?}"),
        }
    }
}
