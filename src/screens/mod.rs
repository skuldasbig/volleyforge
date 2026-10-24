//! The screen state machine.
//!
//! The original runs every menu as a small loop: draw the screen, read one
//! input event, act on it, repeat. VolleyForge does the same, but each screen
//! is a pure function that draws into the [`Canvas`] and returns an
//! [`Action`], so it can be rendered head-less (`--shot`) and tested.
//!
//! The layout of every screen was recovered from `volley.exe`; see
//! [`crate::layout`].

pub mod credits;
pub mod dialog;
pub mod keydef;
pub mod menu;
pub mod multiplayer;
pub mod original_credits;
pub mod settings;
pub mod stats;

use crate::assets::GameAssets;
use crate::canvas::Canvas;
use crate::fonts::{Font, TextStyle};
use crate::settings::Settings;

/// What the user did this frame.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Keys pressed during this frame, in the order they were pressed.
    pub keys: Vec<egui::Key>,
    /// Mouse position in game pixels.
    pub mouse: (i32, i32),
    /// True when the left mouse button was pressed this frame.
    pub clicked: bool,
}

impl Input {
    pub fn pressed(&self, key: egui::Key) -> bool {
        self.keys.contains(&key)
    }
}

/// What a screen wants the application to do next.
#[derive(Clone, Debug)]
pub enum Action {
    /// Stay where we are.
    None,
    /// Switch to another screen.
    Switch(Screen),
    /// Start a local match.
    StartMatch,
    /// Open a network session under the given name and wait for a guest.
    StartHosting(String),
    /// Look for the running sessions.
    FindSessions,
    /// Join the session at the given index of the session list.
    JoinSession(usize),
    /// Switch the control mode of the local player in the network menus.
    ///
    /// The original switches between keyboard and mouse with
    /// `(mode + 1) & 1` (`BV_Menu_0044ABA8.c:171-178`); the computer
    /// opponents are not offered in a network game.
    CycleNetControl,
    /// A name was confirmed in the name input dialog.
    ///
    /// The dialog does not know where the text belongs: the two player names
    /// go into the settings, the game name and the host address into the
    /// multiplayer screens. The application sorts it out.
    NameConfirmed {
        target: dialog::NameTarget,
        text: String,
    },
    /// Leave the application.
    Quit,
}

/// All screens of the application.
#[derive(Clone, Debug)]
pub enum Screen {
    Menu(menu::MenuState),
    Settings(settings::SettingsState),
    KeyDef(keydef::KeyDefState),
    Stats(stats::StatsState),
    Multiplayer(multiplayer::MpState),
    NameInput(dialog::NameInputState),
    QuitAsk(dialog::QuitAskState),
    /// The credits of VolleyForge, in front of the original's ones.
    Credits(credits::CreditsState),
    /// The credits of the original (`FUN_00448E70`).
    OriginalCredits(original_credits::OriginalCreditsState),
    /// The running match.
    Game,
}

impl Screen {
    pub fn name(&self) -> &'static str {
        match self {
            Screen::Menu(_) => "menu",
            Screen::Settings(_) => "settings",
            Screen::KeyDef(_) => "keydef",
            Screen::Stats(_) => "stats",
            Screen::Multiplayer(_) => "multiplayer",
            Screen::NameInput(_) => "nameinput",
            Screen::QuitAsk(_) => "quitask",
            Screen::Credits(_) => "credits",
            Screen::OriginalCredits(_) => "originalcredits",
            Screen::Game => "game",
        }
    }
}

/// Draw the current screen and hand the input to it.
pub fn render_screen(
    screen: &mut Screen,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    match screen {
        Screen::Menu(s) => menu::render(s, canvas, ctx, input),
        Screen::Settings(s) => settings::render(s, canvas, ctx, input),
        Screen::KeyDef(s) => keydef::render(s, canvas, ctx, input),
        Screen::Stats(s) => stats::render(s, canvas, ctx, input),
        Screen::Multiplayer(s) => multiplayer::render(s, canvas, ctx, input),
        Screen::NameInput(s) => dialog::render_name_input(s, canvas, ctx, input),
        Screen::QuitAsk(s) => dialog::render_quit_ask(s, canvas, ctx, input),
        Screen::Credits(s) => credits::render(s, canvas, ctx, input),
        Screen::OriginalCredits(s) => original_credits::render(s, canvas, ctx, input),
        Screen::Game => Action::None,
    }
}

/// Everything a screen needs to draw itself.
pub struct Ctx<'a> {
    pub assets: &'a GameAssets,
    pub font: &'a Font,
    pub settings: &'a mut Settings,
}

impl Ctx<'_> {
    /// A localised text from the **active** language.
    ///
    /// The original fetches every string the same way: the table of the active
    /// language (`+0x9781FC + language*4`) and then the entry of a section,
    /// e.g. `@main + 2` for the third menu row.
    pub fn text(&self, section: &str, index: usize) -> String {
        self.assets
            .table(self.settings.language)
            .and_then(|t| t.entry(section, index))
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
}

/// Draw the original mouse cursor (48x48) at the pointer position.
///
/// The original blits the sprite with its **top left corner** on the pointer:
/// `FUN_00447AF8` writes the sprite rows to `mouse_y - 0x2F .. mouse_y` of the
/// bottom-up surface. That is the pointer row *downwards* in top-down
/// coordinates - the arrow points up and to the left, and its tip sits three
/// pixels inside the sprite, so it lands right on the pointer.
///
/// The `- 0x2F` offset must **not** be applied here: VolleyForge's canvas is
/// top-down and so is the pointer position, which is already converted by
/// [`crate::app::VolleyForge::to_game`].
pub fn draw_cursor(canvas: &mut Canvas, assets: &GameAssets, mouse: (i32, i32)) {
    if let Some(set) = assets.set(48, 48) {
        canvas.blit(set.frame(0), 48, 48, mouse.0, mouse.1);
    }
}
/// Draw a centred menu row and return the rectangle it occupies.
pub fn draw_row(
    canvas: &mut Canvas,
    font: &Font,
    text: &str,
    center_x: i32,
    y: i32,
    selected: bool,
    disabled: bool,
) -> (i32, i32, i32, i32) {
    let w = font.width(text);
    let x = center_x - w / 2;
    let style = if disabled {
        TextStyle::Disabled
    } else if selected {
        TextStyle::Highlight
    } else {
        TextStyle::Normal
    };
    font.draw(canvas, text, x, y, style);
    (x, y, w, crate::layout::GLYPH_SIZE)
}

/// Hit test a rectangle that was produced by [`draw_row`].
pub fn hit(rect: (i32, i32, i32, i32), mouse: (i32, i32)) -> bool {
    let (x, y, w, h) = rect;
    mouse.0 >= x - 4 && mouse.0 <= x + w + 4 && mouse.1 >= y - 4 && mouse.1 <= y + h + 4
}
