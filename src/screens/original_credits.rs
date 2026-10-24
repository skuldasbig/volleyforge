//! The credits screen (`FUN_00448E70`).
//!
//! The original shows the credits at the end of the main menu: the "QUIT"
//! row and the ESC key end the menu loop, and the caller then plays the
//! credits before the program ends (`0x44706D: call 0x448E70`).
//!
//! The picture is chunk 90 of `graph.pak` (400x300, 8 bpp). `FUN_00448E70`
//! has three phases:
//!
//! 1. **Roll-in**: 200 steps, one every 33 ms (`+0x978234` = 0x21). Every row
//!    of the picture has its own x offset, so it slides in diagonally and
//!    leaves a smear behind.
//! 2. **Still**: the picture centred on a black surface, at most 4000 ms.
//! 3. **Dissolve**: the picture is eaten away block by block.
//!
//! A key press or a mouse click skips a phase, exactly like in the original.

use web_time::Instant;

use crate::canvas::Canvas;
use crate::log::info;
use crate::screens::{Action, Ctx, Input};

/// The width of the credits picture (chunk 90).
const IMAGE_W: usize = 400;
/// The height of the credits picture.
const IMAGE_H: usize = 300;
/// The picture's left edge when it has arrived (0xC8).
const CENTER_X: i32 = 200;
/// The picture's top edge in top-down coordinates (0x96).
const TOP_Y: i32 = 150;
/// The number of roll-in steps (`local_a44` starts at 200).
const STEPS: u32 = 200;
/// The delay between two steps in milliseconds (`+0x978234` = 0x21).
const STEP_MS: u64 = 33;
/// The step from which the rows start to wobble (0x64).
const WOBBLE_FROM: u32 = 100;
/// How long the picture stands in the middle (0xFA0 = 4000 ms).
const STILL_MS: u64 = 4000;
/// The blocks of the dissolve are 25x25 pixels.
const BLOCK: i32 = 25;
/// The blocks across the picture.
const BLOCKS_X: i32 = 16;
/// The blocks down the picture.
const BLOCKS_Y: i32 = 12;
/// The bottom edge of the picture in top-down coordinates (600 - 150 - 1).
const BOTTOM_Y: i32 = 449;

/// The phase the credits are in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalPhase {
    /// The picture slides in.
    RollIn,
    /// The picture stands in the middle.
    Still,
    /// The picture dissolves.
    Dissolve,
}

/// State of the credits screen.
#[derive(Clone, Debug)]
pub struct OriginalCreditsState {
    /// The phase the screen is in.
    pub phase: OriginalPhase,
    /// The step of the roll-in (200 -> 0).
    pub step: u32,
    /// When the screen was entered.
    pub entered: Instant,
    /// When the current phase was entered.
    pub phase_started: Instant,
    /// The height field of the dissolve (25x25).
    pub height: Vec<u16>,
    /// The noise of the dissolve, one value per block.
    pub noise: Vec<i32>,
    /// The noise mode of the dissolve (0..2).
    pub mode: u32,
    /// The random state of the original (`DAT_0044F040`).
    pub seed: u32,
    /// True once the surface was filled with the original's dark grey.
    pub surface_filled: bool,
}

impl Default for OriginalCreditsState {
    fn default() -> Self {
        let now = Instant::now();
        OriginalCreditsState {
            phase: OriginalPhase::RollIn,
            step: STEPS,
            entered: now,
            phase_started: now,
            height: Vec::new(),
            noise: Vec::new(),
            mode: 0,
            seed: 0,
            surface_filled: false,
        }
    }
}

/// Draw the credits and act on the input.
pub fn render(
    state: &mut OriginalCreditsState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    let picture = ctx.assets.set(IMAGE_W, IMAGE_H).map(|s| s.frame(0));
    // The original leaves a phase on a key press or a mouse click.
    let aborted = !input.keys.is_empty() || input.clicked;

    match state.phase {
        OriginalPhase::RollIn => {
            if !state.surface_filled {
                // The original fills the surface with 0x01010101 (dark grey)
                // once and never clears it again. That is why the picture
                // leaves a smear behind while it slides in.
                canvas.fill([1, 1, 1, 255]);
                state.surface_filled = true;
            }
            let elapsed = state.entered.elapsed().as_millis() as u64;
            state.step = STEPS.saturating_sub((elapsed / STEP_MS) as u32);
            if let Some(pic) = picture {
                for row in 0..IMAGE_H {
                    // The original's picture runs bottom-up, the one in the
                    // port runs top-down.
                    let original_row = IMAGE_H - 1 - row;
                    let x = roll_in_offset(original_row, state.step);
                    canvas.blit_row(pic, IMAGE_W, row, x, TOP_Y + row as i32);
                }
            }
            if state.step == 0 || aborted {
                state.phase = OriginalPhase::Still;
                state.phase_started = Instant::now();
            }
        }
        OriginalPhase::Still => {
            // The original clears the surface and draws the picture centred.
            canvas.fill([0, 0, 0, 255]);
            if let Some(pic) = picture {
                canvas.blit(pic, IMAGE_W, IMAGE_H, CENTER_X, TOP_Y);
            }
            if state.phase_started.elapsed().as_millis() as u64 >= STILL_MS || aborted {
                state.phase = OriginalPhase::Dissolve;
                state.phase_started = Instant::now();
                state.height = height_field();
                state.mode = random(&mut state.seed, 3);
                state.noise = noise_table(state.mode, &mut state.seed);
            }
        }
        OriginalPhase::Dissolve => {
            if state.height.is_empty() {
                state.height = height_field();
                state.noise = noise_table(state.mode, &mut state.seed);
            }
            // The threshold of the original falls by one per millisecond, so
            // the picture dissolves from the outside in.
            let elapsed = state.phase_started.elapsed().as_millis() as u64;
            let base = 0x800i64 - elapsed as i64;
            let mut alive = false;
            for outer in 0..BLOCKS_Y {
                for inner in 0..BLOCKS_X {
                    let threshold =
                        base + state.noise[outer as usize * BLOCKS_X as usize + inner as usize] as i64;
                    for dx in 0..BLOCK {
                        for dy in 0..BLOCK {
                            let h = state.height[dy as usize * BLOCK as usize + dx as usize] as i64;
                            let x = CENTER_X + BLOCK * inner + dx;
                            // The original's surface runs bottom-up.
                            let y = BOTTOM_Y - BLOCK * outer - dy;
                            if threshold < h {
                                canvas.put_pixel(x, y, 0, 0, 0);
                            } else {
                                alive = true;
                            }
                        }
                    }
                }
            }
            if !alive || aborted {
                info!("the credits are over");
                return Action::Quit;
            }
        }
    }
    Action::None
}

/// The x offset of one picture row during the roll-in (`0x448F2D`..`0x448FB5`).
///
/// ```text
/// x = 200 + round( (step-100) * sqrt(row+1)
///                   + (step-100) * (sin(row*step/1000) * 5)^2 / 100 )
/// ```
///
/// The constants are the ones of the original: `1000.0` (0x4493A8), `5.0`
/// (0x4493AC) and `100.0` (0x4493B0).
fn roll_in_offset(row: usize, step: u32) -> i32 {
    if step < WOBBLE_FROM {
        return CENTER_X;
    }
    let c = (step - WOBBLE_FROM) as f64;
    let r = row as f64;
    let t = c * (r + 1.0).sqrt();
    let s = (r * step as f64 / 1000.0).sin() * 5.0;
    CENTER_X + (t + c * s * s / 100.0).round() as i32
}

/// The height field of the dissolve (`0x4491AF`..`0x4491EE`).
///
/// ```text
/// height[y][x] = round( sqrt((x-12)^2 + (y-12)^2) / 17 * 2048 )
/// ```
///
/// The constants are `17.0` (0x4493B4) and `2048.0` (0x4493B8).
fn height_field() -> Vec<u16> {
    let mut field = Vec::with_capacity((BLOCK * BLOCK) as usize);
    for y in 0..BLOCK {
        for x in 0..BLOCK {
            let d = (((x - 12) as f64).powi(2) + ((y - 12) as f64).powi(2)).sqrt();
            field.push((d / 17.0 * 2048.0).round() as u16);
        }
    }
    field
}

/// The noise of the dissolve, one value per block (`FUN_00448DD8`).
///
/// The original computes it once for the whole dissolve, so the noise of a
/// block does not change while the picture dissolves.
fn noise_table(mode: u32, seed: &mut u32) -> Vec<i32> {
    let mut table = Vec::with_capacity((BLOCKS_X * BLOCKS_Y) as usize);
    for outer in 0..BLOCKS_Y {
        for inner in 0..BLOCKS_X {
            table.push(noise(mode, inner as u32, outer as u32, seed));
        }
    }
    table
}

/// The noise of one block (`FUN_00448DD8`).
///
/// * mode 0: `2000 - round(st0)` - the original reads the x87 register here.
///   Its value is not statically known, so VolleyForge uses the original's own
///   random generator instead, which keeps the value in the same range.
///   **This is an assumption, not a recovered fact.**
/// * mode 1: `(y + x) * 100`
/// * mode 2: `(x*3 ^ y<<2) * 50`
fn noise(mode: u32, x: u32, y: u32, seed: &mut u32) -> i32 {
    match mode {
        0 => 2000 - random(seed, 2000) as i32,
        1 => ((y + x) * 100) as i32,
        _ => (((x * 3) ^ (y << 2)) * 50) as i32,
    }
}

/// The random generator of the original (`FUN_00402AB0`).
///
/// ```text
/// seed = seed * 0x8088405 + 1
/// return (n * seed) >> 32
/// ```
fn random(seed: &mut u32, n: u32) -> u32 {
    *seed = seed.wrapping_mul(0x8088_405).wrapping_add(1);
    ((n as u64 * *seed as u64) >> 32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picture slides in from the right and ends up centred.
    #[test]
    fn the_picture_slides_in_from_the_right() {
        // The first step is the one where the picture is furthest away.
        let first = roll_in_offset(0, STEPS);
        let last = roll_in_offset(0, 100);
        assert!(first > CENTER_X, "the picture starts on the right");
        assert_eq!(last, CENTER_X, "the picture ends up centred");
        // The lower rows are further away than the upper ones, so the
        // picture slides in diagonally.
        assert!(
            roll_in_offset(299, STEPS) > roll_in_offset(0, STEPS),
            "the picture slides in diagonally"
        );
    }

    /// The height field grows with the distance from the middle of a block.
    #[test]
    fn the_height_field_is_radial() {
        let field = height_field();
        assert_eq!(field.len(), (BLOCK * BLOCK) as usize);
        let middle = field[12 * BLOCK as usize + 12];
        let corner = field[0];
        assert_eq!(middle, 0, "the middle of a block has no height");
        assert!(corner > middle, "the corners are higher");
    }

    /// The random generator of the original starts with zero and produces
    /// values below `n`.
    #[test]
    fn the_random_generator_matches_the_original() {
        let mut seed = 0u32;
        assert_eq!(random(&mut seed, 3), 0, "the first value is zero");
        assert_eq!(seed, 1, "the seed starts at zero");
        for _ in 0..100 {
            let v = random(&mut seed, 2000);
            assert!(v < 2000);
        }
    }

    /// The three noise modes of the original.
    #[test]
    fn the_noise_modes_match_the_original() {
        let mut seed = 0u32;
        assert_eq!(noise(1, 3, 4, &mut seed), 700, "(y+x)*100");
        assert_eq!(noise(2, 3, 4, &mut seed), 1250, "(x*3 ^ y<<2)*50");
        assert!(noise(0, 3, 4, &mut seed) <= 2000);
    }
}
