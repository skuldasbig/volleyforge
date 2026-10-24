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

/// State of the name input dialog.
#[derive(Clone, Debug)]
pub struct NameInputState {
    pub target: NameTarget,
    pub buffer: String,
    pub max_len: usize,
}

impl NameInputState {
    pub fn new(player: usize, current: &str) -> NameInputState {
        let (target, max_len) = match player {
            0 => (NameTarget::Player1, 12),
            1 => (NameTarget::Player2, 12),
            2 => (NameTarget::GameName, 12),
            _ => (NameTarget::HostAddress, 20),
        };
        NameInputState {
            target,
            buffer: current.to_string(),
            max_len,
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
                return back(ctx);
            }
            egui::Key::Enter => {
                info!(target = ?state.target, text = %state.buffer, "name confirmed");
                apply(state, ctx);
                return back(ctx);
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
fn back(ctx: &mut Ctx) -> Action {
    let _ = ctx;
    Action::Switch(Screen::Settings(
        crate::screens::settings::SettingsState::default(),
    ))
}

/// Store the typed text.
fn apply(state: &NameInputState, ctx: &mut Ctx) {
    match state.target {
        NameTarget::Player1 => ctx.settings.names[0] = state.buffer.clone(),
        NameTarget::Player2 => ctx.settings.names[1] = state.buffer.clone(),
        NameTarget::GameName | NameTarget::HostAddress => {}
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
