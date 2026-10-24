//! The credits screen of VolleyForge.
//!
//! The original ends the main menu with the credits of the shareware version
//! (`FUN_00448E70`, see [`crate::screens::original_credits`]). VolleyForge puts
//! its own screen in front of it, which names the project, the tools the
//! reimplementation was made with and the repository.
//!
//! The text follows the language of the game (0 = French, 1 = German,
//! 2 = English). It cannot come from the original's `text.pak`, because these
//! texts do not exist there - so the port keeps its own table.
//!
//! The screen stands for [`STAND_MS`] milliseconds and then hands over to the
//! original's credits; a key press skips it.

use web_time::Instant;

use crate::canvas::Canvas;
use crate::fonts::TextStyle;
use crate::log::info;
use crate::screens::{Action, Ctx, Input, Screen};

/// How long the screen stands before the original's credits follow.
pub const STAND_MS: u64 = 5000;

/// The title of the project.
const TITLE: &str = "VolleyForge";

/// The name of the game this is a reimplementation of.
const GAME: &str = "Blobby Volley";

/// The address of the project.
const REPOSITORY: &str = "github.com/skuldasbig/volleyforge";

/// The texts of the screen, one set per language (0 = French, 1 = German,
/// 2 = English).
///
/// The original's font has no French accents, so the French text is written
/// without them - the same way French capitals are usually written.
const TEXTS: [[&str; 5]; 3] = [
    // French
    [
        "Une reimplementation de",
        "en Rust",
        "Avec Ghidra, le plugin REA,",
        "MiniMax AI et beaucoup de cafe",
        "Appuyez sur une touche",
    ],
    // German
    [
        "Eine Neuimplementierung von",
        "in Rust",
        "Mit Ghidra, dem REA-Plugin,",
        "MiniMax AI und viel Kaffee",
        "Taste drücken",
    ],
    // English
    [
        "A reimplementation of",
        "in Rust",
        "With Ghidra, the REA plugin,",
        "MiniMax AI and lots of coffee",
        "Press any key",
    ],
];

/// State of the credits screen.
#[derive(Clone, Debug)]
pub struct CreditsState {
    /// When the screen was entered.
    pub entered: Instant,
}

impl Default for CreditsState {
    fn default() -> Self {
        CreditsState {
            entered: Instant::now(),
        }
    }
}

/// Draw the screen and act on the input.
pub fn render(
    state: &mut CreditsState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    // The menu screens of the original use the darkened copy of the beach -
    // that is also what makes the rainbow font readable.
    canvas.copy_from(&ctx.assets.background_night);

    let lang = ctx.settings.language.min(TEXTS.len() - 1);
    let t = TEXTS[lang];

    let font = &ctx.font;
    font.draw_centered(canvas, TITLE, 400, 150, TextStyle::Highlight);
    font.draw_centered(canvas, t[0], 400, 225, TextStyle::Normal);
    font.draw_centered(canvas, GAME, 400, 265, TextStyle::Highlight);
    font.draw_centered(canvas, t[1], 400, 305, TextStyle::Normal);
    font.draw_centered(canvas, t[2], 400, 375, TextStyle::Normal);
    font.draw_centered(canvas, t[3], 400, 415, TextStyle::Normal);
    font.draw_centered(canvas, REPOSITORY, 400, 470, TextStyle::Normal);
    font.draw_centered(canvas, t[4], 400, 545, TextStyle::Disabled);

    // A key press skips the screen, otherwise it stands for five seconds.
    let elapsed = state.entered.elapsed().as_millis() as u64;
    if !input.keys.is_empty() || input.clicked || elapsed >= STAND_MS {
        info!("the credits of VolleyForge are over, the original's follow");
        return Action::Switch(Screen::OriginalCredits(
            crate::screens::original_credits::OriginalCreditsState::default(),
        ));
    }
    Action::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language has the same five texts, and every one of them only
    /// uses characters the original's font can draw.
    #[test]
    fn every_text_can_be_drawn_with_the_original_font() {
        for language in TEXTS.iter() {
            for text in language.iter() {
                for c in text.chars() {
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
        }
    }

    /// The texts really differ per language.
    #[test]
    fn the_texts_follow_the_language() {
        assert_ne!(TEXTS[0][0], TEXTS[1][0]);
        assert_ne!(TEXTS[1][0], TEXTS[2][0]);
        assert_eq!(TEXTS[1][4], "Taste drücken");
    }

    /// The longest line still fits into the screen.
    #[test]
    fn the_longest_line_fits() {
        let longest = TEXTS
            .iter()
            .flatten()
            .map(|t| t.chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            longest as i32 * crate::layout::GLYPH_ADVANCE <= 800,
            "the longest line is too wide"
        );
    }
}
