//! The match screen.
//!
//! The original composes the court into the same 800x600 DirectDraw surface
//! the menus use. The draw order is the one of the game loop (`BV_Setup2.c`):
//!
//!  1. the bright beach background
//!  2. the two blinking 5x5 position markers
//!  3. the LED scoreboard - two digits per player
//!  4. the ground shadow under each player
//!  5. the 69x17 contact shadow of the ball
//!  6. the ball itself
//!  7. the two blobs
//!  8. the two player names
//!
//! Every position below comes from the original's own drawing routines; see
//! `WISSENSSTAND.md` part 0.7. The original works in a **bottom-up** surface,
//! so a canvas row is converted with [`crate::layout::to_top_down`].

use crate::assets::GameAssets;
use crate::canvas::Canvas;
use crate::court::*;
use crate::fonts::{Font, TextStyle};
use crate::layout::{self, to_top_down, SCOREBOARD_X};
use crate::rules::Match;

/// The blob: 148x101 pixels, five animation frames.
const BLOB_W: usize = 148;
const BLOB_H: usize = 101;
/// The ground shadow: 129x25 pixels, ten frames.
const SHADOW_W: usize = 129;
const SHADOW_H: usize = 25;
/// The ball's contact shadow on the sand: 69x17 pixels, one frame.
const ELEMENT_W: usize = 69;
const ELEMENT_H: usize = 17;
/// The ball: 64x64 pixels, sixteen frames.
const BALL_W: usize = 64;
const BALL_H: usize = 64;

/// Lower edge of the scoreboard digits, counted from the bottom of the screen.
const SCORE_CANVAS_Y: i32 = 540;
/// Lower edge of the player name, counted from the bottom of the screen.
const NAME_CANVAS_Y: i32 = 0x23A;
/// The ones digit sits one glyph further right than the tens digit.
const SCORE_ONES_DX: i32 = 24;
/// The serve indicator (an exclamation mark) sits 48 pixels right of the
/// scoreboard's left edge (`FUN_00443694`, the third cell at `+0x1A5EC0`).
const SCORE_SERVE_DX: i32 = 48;
/// The blinking match point mark sits 4 pixels further right
/// (`FUN_00443694`, the fourth cell at `+0x1A5ED0`).
const SCORE_MATCH_DX: i32 = 52;
/// Edge length of the blinking position markers.
const MARKER: i32 = 5;
/// The marker is a block of five pixels, so it spans centre - 2 .. centre + 2.
const MARKER_HALF: i32 = 2;

/// Where the win message is drawn, counted from the top.
const WIN_MESSAGE_Y: i32 = 240;

/// Draws the match screen.
///
/// The blob frames are tinted with the team colour. The original does this
/// once per colour change (`FUN_0044A784`) and keeps the ten tinted frames
/// in memory, so the frame loop never multiplies a colour.
#[derive(Debug, Default)]
pub struct MatchRenderer {
    /// The five blob frames of each player, pre-tinted.
    tinted: [Vec<Vec<u8>>; 2],
    /// The team colour each cache was built for.
    colours: [usize; 2],
}

/// The screen the original shows when the match is over (`FUN_00449D3C`).
///
/// The whole picture is darkened (every colour channel halved), the box in
/// the middle (x 200..600, y 200..400) is quartered. The winner's name is
/// drawn at x = 300, y = 320 and the "wins" text at x = 300, y = 270 - both
/// in the original's bottom-up coordinates.
pub fn draw_match_over(
    canvas: &mut Canvas,
    assets: &GameAssets,
    font: &Font,
    winner_name: &str,
    wins_text: &str,
) {
    canvas.darken((200, 200, 600, 400));

    // The original draws a 130x360 picture of the graph.pak at x = 135.
    if let Some(set) = assets.set(130, 360) {
        canvas.blit(set.frame(0), 130, 360, 135, 96);
    }

    // The winner's name and the "wins" text.
    font.draw(canvas, winner_name, 300, 600 - 320 - 24, TextStyle::Normal);
    font.draw(canvas, wins_text, 300, 600 - 270 - 24, TextStyle::Normal);
}

impl MatchRenderer {
    pub fn new() -> Self {
        MatchRenderer {
            tinted: [Vec::new(), Vec::new()],
            colours: [usize::MAX; 2],
        }
    }

    /// Tint the blob frames of one player with its team colour.
    fn retint(&mut self, assets: &GameAssets, player: usize, colour: usize) {
        if self.colours[player] == colour && !self.tinted[player].is_empty() {
            return;
        }
        let Some(blob) = assets.set(BLOB_W, BLOB_H) else {
            return;
        };
        let rgb = layout::TEAM_COLORS[colour % layout::TEAM_COLORS.len()];
        let stride = BLOB_W * BLOB_H * 4;
        let mut frames = Vec::with_capacity(blob.count);
        for f in 0..blob.count {
            let src = blob.frame(f);
            let mut out = vec![0u8; stride];
            for p in 0..stride / 4 {
                let (r, g, b) = layout::tint_rgb(rgb, src[p * 4], src[p * 4 + 1], src[p * 4 + 2]);
                out[p * 4] = r;
                out[p * 4 + 1] = g;
                out[p * 4 + 2] = b;
                out[p * 4 + 3] = src[p * 4 + 3];
            }
            frames.push(out);
        }
        self.tinted[player] = frames;
        self.colours[player] = colour;
    }

    /// Draw the whole court into `canvas`.
    ///
    /// `names` holds the two names to show above the scoreboards, `mouse` the
    /// pointer position in game pixels (only needed for the mouse marker) and
    /// `tick_ms` the millisecond counter the original takes from
    /// `GetTickCount`. `ball_frame` overrides the ball's rotation frame; the
    /// original derives it from the tick count.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        assets: &GameAssets,
        font: &Font,
        game: &Match,
        names: &[String; 2],
        mouse: Option<(i32, i32)>,
        tick_ms: u64,
        ball_frame: Option<usize>,
    ) {
        // 1. The match uses the *bright* beach; only the menus use the
        //    darkened copy.
        canvas.copy_from(&assets.background);

        // 2. The two blinking 5x5 position markers.
        self.draw_markers(canvas, game, mouse, tick_ms);

        // 3. The LED scoreboard.
        self.draw_scoreboard(canvas, font, game, tick_ms);

        // 4. The ground shadows.
        for player in 0..2 {
            self.draw_shadow(canvas, assets, game, player);
        }

        // 5. The ball's contact shadow on the sand.
        self.draw_element(canvas, assets, game);

        // 6. The ball.
        self.draw_ball(canvas, assets, game, tick_ms, ball_frame);

        // 7. The two blobs.
        for player in 0..2 {
            self.retint(assets, player, game.players[player].colour);
            self.draw_blob(canvas, game, player);
        }

        // 8. The player names.
        for player in 0..2 {
            self.draw_name(canvas, font, names, player);
        }

    }

    /// The two blinking position markers.
    ///
    /// The original writes `-(GetTickCount() >> 8 & 1)` into the five pixels
    /// of the ball marker, so it is white while that bit is set and black
    /// otherwise. The ball marker sits at the **top** of the screen and
    /// follows the ball; the player marker sits at the **bottom** and shows
    /// the pointer position of a mouse controlled player.
    fn draw_markers(
        &self,
        canvas: &mut Canvas,
        game: &Match,
        mouse: Option<(i32, i32)>,
        tick_ms: u64,
    ) {
        let ball_white = (tick_ms >> 8) & 1 == 1;
        let player_white = (tick_ms >> 7) & 1 == 1;

        // Ball marker: canvas rows 0x24E..0x252, column = ball x +/- 2.
        let ball_x = game.ball.px();
        for row in 0x24E..0x24E + MARKER {
            let y = VIEW_H - row - 1;
            for dx in -MARKER_HALF..=MARKER_HALF {
                let c = if ball_white { 255 } else { 0 };
                canvas.put_pixel(ball_x + dx, y, c, c, c);
            }
        }

        // Player marker: canvas rows 5..9, column = pointer x / 2 + player
        // offset. Only a player that is steered by the mouse gets one.
        if let Some((mx, _my)) = mouse {
            for player in 0..2 {
                if !matches!(
                    game.players[player].control,
                    crate::player::Control::Mouse
                ) {
                    continue;
                }
                let cx = (mx >> 1) + player as i32 * 400;
                for row in 5..5 + MARKER {
                    let y = VIEW_H - row - 1;
                    for dx in -MARKER_HALF..=MARKER_HALF {
                        let c = if player_white { 255 } else { 0 };
                        canvas.put_pixel(cx + dx, y, c, c, c);
                    }
                }
            }
        }
    }

    /// The LED scoreboard: two digits per player, the tens digit at
    /// `SCOREBOARD_X`, the ones digit 24 pixels further right.
    ///
    /// The original draws four cells per player (`FUN_00443694`): the tens
    /// digit at `+0x1A5E00`, the ones digit 24 pixels further right
    /// (`+0x1A5E60`), the **serve indicator** at `+0x1A5EC0` (48 pixels) and
    /// the **blinking match point mark** at `+0x1A5ED0` (52 pixels). Both
    /// marks use the exclamation mark, font slot 37
    /// (`0x9589E8 + 37 * 0x900 = 0x96D6E8`).
    fn draw_scoreboard(&self, canvas: &mut Canvas, font: &Font, game: &Match, tick_ms: u64) {
        let y = to_top_down(SCORE_CANVAS_Y, crate::layout::GLYPH_SIZE);
        for player in 0..2 {
            let score = game.players[player].score.max(0);
            let tens = (score / 10) % 10;
            let ones = score % 10;
            let x = SCOREBOARD_X[player];
            font.draw(canvas, &tens.to_string(), x, y, TextStyle::Normal);
            font.draw(
                canvas,
                &ones.to_string(),
                x + SCORE_ONES_DX,
                y,
                TextStyle::Normal,
            );
            // The exclamation mark of the player who has the service.
            if game.serve_indicator() == Some(player) {
                font.draw(canvas, "!", x + SCORE_SERVE_DX, y, TextStyle::Normal);
                // The match point blinks a second mark next to it: the
                // original draws it while `GetTickCount() & 0x100 == 0`,
                // so it appears for 256 ms and disappears for 256 ms.
                if game.is_match_point() && (tick_ms & 0x100) == 0 {
                    font.draw(canvas, "!", x + SCORE_MATCH_DX, y, TextStyle::Normal);
                }
            }
        }
    }

    /// The ground shadow below a player.
    ///
    /// The original draws it at the player's projected position
    /// (`local_38[0..1]`), that is 64 pixels to the left of the projection
    /// and on the projected row.
    fn draw_shadow(&self, canvas: &mut Canvas, assets: &GameAssets, game: &Match, player: usize) {
        let Some(set) = assets.set(SHADOW_W, SHADOW_H) else {
            return;
        };
        let p = &game.players[player];
        // The original uses the shadow frames 0..4 for player one and 5..9
        // for player two (`BV_Setup2.c:263`: `... + 0x9782ac + 5`).
        let frame = p.frame % 5 + player * 5;
        let x = p.screen_x(player) - 0x40;
        let y = to_top_down(p.screen_y(), SHADOW_H as i32);
        canvas.blit(set.frame(frame), SHADOW_W, SHADOW_H, x, y);
    }

    /// The 69x17 patch the ball leaves on the sand.
    ///
    /// The original does not attach it to the ball - it is the ball's
    /// **ground projection** (`BV_Setup2.c:168-181`): with `height` the
    /// ball's height above the floor,
    ///   * the column is `ball_x + height / 3` (the court's perspective is
    ///     oblique: the closer to the viewer, the further to the right),
    ///   * the row is `(height >> 3) + 97` counted from the bottom.
    fn draw_element(&self, canvas: &mut Canvas, assets: &GameAssets, game: &Match) {
        let Some(set) = assets.set(ELEMENT_W, ELEMENT_H) else {
            return;
        };
        let height = game.ball.py() - 100;
        let rounded = if height < 0 { height + 7 } else { height };
        let column = game.ball.px() + height / 3;
        let row = (rounded >> 3) + 97;
        let x = column - 0x22;
        let y = VIEW_H - (row - 8) - ELEMENT_H as i32;
        canvas.blit(set.frame(0), ELEMENT_W, ELEMENT_H, x, y);
    }

    /// The ball itself, centred on its position.
    ///
    /// The original picks the rotation frame from the tick count
    /// (`GetTickCount() >> 6 & 0xF`).
    ///
    /// The original's canvas is **bottom-up**: the ball's position is a canvas
    /// row and `FUN_00442F04` draws the 64x64 sprite with its centre on that
    /// row (`param_3 - 0x20` plus 64 rows upwards). In top-down coordinates
    /// that is `600 - ball_y - 32`.
    fn draw_ball(
        &self,
        canvas: &mut Canvas,
        assets: &GameAssets,
        game: &Match,
        tick_ms: u64,
        ball_frame: Option<usize>,
    ) {
        let Some(set) = assets.set(BALL_W, BALL_H) else {
            return;
        };
        let frame = ball_frame.unwrap_or_else(|| ((tick_ms >> 6) as usize) & 0xF);
        let x = game.ball.px() - (BALL_W as i32) / 2;
        let y = VIEW_H - game.ball.py() - (BALL_H as i32) / 2;
        canvas.blit(set.frame(frame), BALL_W, BALL_H, x, y);
    }

    /// One blob.
    ///
    /// The original draws it centred on the player's x (`FUN_004430AC` starts
    /// at `param_2 - 0x4A`) with its lower edge on the player's row.
    fn draw_blob(&self, canvas: &mut Canvas, game: &Match, player: usize) {
        let frames = &self.tinted[player];
        if frames.is_empty() {
            return;
        }
        let p = &game.players[player];
        let frame = p.frame % frames.len();
        let x = p.x - 0x4A;
        // The original draws the blob on the **raw** player row
        // (`FUN_004430AC`); only the shadow uses the projected row.
        let y = to_top_down(p.y, BLOB_H as i32);
        canvas.blit(&frames[frame], BLOB_W, BLOB_H, x, y);
    }

    /// The name above a scoreboard, centred on it.
    fn draw_name(&self, canvas: &mut Canvas, font: &Font, names: &[String; 2], player: usize) {
        let name = &names[player];
        if name.is_empty() {
            return;
        }
        let x = SCOREBOARD_X[player] - font.width(name) / 2;
        let y = to_top_down(NAME_CANVAS_Y, crate::layout::GLYPH_SIZE);
        font.draw(canvas, name, x, y, TextStyle::Normal);
    }
}
