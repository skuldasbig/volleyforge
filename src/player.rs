//! Player state, input and the original's screen-projection quirk.
//!
//! Recovered from the frame loop in `FUN_0044BF98`. The original stores the
//! player's X directly in **whole pixels** but its jump height `Y` in the same
//! 1/32 px fixed point as the ball, then projects both onto the screen with a
//! height-dependent parallax shift:
//!
//! ```text
//! dx   = (Y - 100) >> 2   (or Y - 97 when Y < 100)
//! sx   = (X + dx + 0x18 - index * 0x18) & ~1
//! sy   = ((Y - 100) >> 3) + 0x61      (or Y - 0x5D when Y < 100)
//! ```
//!
//! That shift is what gives the blobs their "lift off the floor" look: the
//! higher the jump, the further the sprite is drawn from its ground anchor.

use crate::ball::Ball;
use crate::court::*;
use crate::log::{debug, info, trace, warn};

/// How a player is steered.
///
/// The original knows five modes (`+0x978224`, `0..4`): keyboard, mouse and
/// three computer opponents. The port collapses the three CPU levels into
/// [`Control::Cpu`] plus [`Player::skill`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    Keyboard,
    Mouse,
    Cpu,
}

/// Per-frame intent, exactly the three flags the original ships over the wire.
#[derive(Clone, Copy, Default, Debug)]
pub struct Intent {
    pub left: bool,
    pub right: bool,
    pub jump: bool,
}

/// Palette used by the original for team selection (8 entries).
pub const TEAM_COLOURS: [[u8; 3]; 8] = [
    [0xFF, 0x00, 0x00], // red
    [0xFF, 0x7F, 0x00], // orange
    [0xFF, 0xFF, 0x00], // yellow
    [0x00, 0xFF, 0x00], // green
    [0x00, 0xEC, 0xFF], // cyan
    [0x00, 0x00, 0xFF], // blue
    [0xC0, 0x00, 0xFF], // magenta
    [0xFF, 0x00, 0xD8], // pink
];

/// The original's jump constants (`BV_Phys_H_443e8.c`, lines 432ff).
///
/// The jump is a fixed-point integration: an acceleration that starts at
/// 10000 (1/256 px per frame²) and is reduced by 800 every frame — twice as
/// fast while the jump button is not held. The result is a leap of roughly
/// 260 px with its apex after 13 frames.
pub const JUMP_ACCEL: i32 = 10000;
/// How much the jump acceleration drops per frame.
pub const JUMP_ACCEL_DECAY: i32 = 800;
/// The canvas row of the player's feet on the floor (`player_y = 100`).
pub const GROUND_ROW: i32 = 100;
/// The landing threshold (`player_y < 0x65`).
pub const LAND_ROW: i32 = 101;

/// The blob's collision box, per animation frame.
///
/// `DAT_0044DAF4` — the vertical offset of the collision box's centre,
/// counted from the blob's lower edge (`player_y + 0x65`).
pub const COLLISION_BOX_Y: [i32; 5] = [36, 41, 45, 50, 54];
/// The collision radius squared (`_DAT_004440C8` = 3249.0), that is a
/// radius of exactly 57 pixels.
pub const COLLISION_RADIUS_SQ: f64 = 3249.0;

/// The **second** collision circle of the original (`_DAT_004440E0` = 4096),
/// that is a radius of 64 pixels. It counts the touch.
pub const TOUCH_RADIUS_SQ: f64 = 4096.0;

/// The vertical offsets of the touch box, per animation frame
/// (`DAT_0044DB1C` = 69, 71, 74, 76, 79).
pub const TOUCH_BOX_Y: [i32; 5] = [69, 71, 74, 76, 79];

/// Horizontal walk speed in px per physics frame.
///
/// The original: `player_x += 0xc` (`BV_Phys_H_443e8.c:407`), that is 12 px
/// per frame. The player may not leave their half: player 1 stops at
/// 0x168 (360 px), player 2 at 0x1b8 (440 px).
pub const WALK_SPEED: i32 = 12;

#[derive(Clone, Debug)]
pub struct Player {
    /// Ground anchor X in whole pixels.
    pub x: i32,
    /// The blob's lower edge as a **canvas row counted from the bottom**
    /// (bottom-up). 100 means "on the floor"; the jump raises the row.
    pub y: i32,
    /// Jump velocity in 1/256 px per frame (the original's `+0x1C`).
    pub vel: i32,
    /// Jump acceleration in 1/256 px per frame² (the original's `+0x18`).
    pub accel: i32,
    /// Ball touches taken during the current rally.
    pub touch: i32,
    /// Blunder counter of the easy computer opponent (`+0x48`).
    ///
    /// The original loads it whenever the ball is rising and spends it by
    /// aiming at the net instead of the ball.
    pub blunder: i32,
    pub score: i32,
    pub colour: usize,
    pub control: Control,
    /// Difficulty of a computer opponent: 0 = easy (Adam), 1 = average
    /// (Bert), 2 = hard (Carl).
    pub skill: u32,
    /// Latest intent, mirrored from the input layer.
    pub intent: Intent,
    /// Animation frame, the original's counter `+0x20`.
    ///
    /// It is a **pendulum** between 0 and 4 (`FUN_004443E8:463-471`): it
    /// advances by `anim_dir` and flips the direction at 4, and it freezes
    /// at 0 while the player stands still. The counter drives the sprite
    /// **and** the height of the collision boxes.
    pub frame: usize,
    /// Direction of the animation counter, the original's `+0x24`:
    /// -1, 0 (frozen) or +1.
    pub anim_dir: i32,
    /// The original's contact flag `+0x38`: set in the frame in which the
    /// ball touches this blob, cleared at the start of the next frame.
    pub contact: bool,
    /// The original's latch `+0x39`: it remembers the contact flag of the
    /// previous frame, so the touch counter only rises on the **edge** of a
    /// contact and not on every frame the ball overlaps the blob.
    pub contact_latch: bool,
    /// The original's `+0x40`: frames since the contact began. Holding the
    /// ball on the blob for more than 40 frames is a fault as well.
    pub contact_frames: u32,
    /// Set while the player is airborne.
    pub airborne: bool,
    /// Frames during which this player cannot hit the ball again.
    ///
    /// The original does not need this: the ball is deflected out of the
    /// blob's collision circle, so it cannot hit the same blob twice in a
    /// row. The port has no collision circle yet, so the hit is locked for a
    /// moment instead.
    pub cooldown: u32,
}

impl Player {
    pub fn new(index: usize) -> Self {
        Player {
            x: PLAYER_SPAWN_X[index],
            y: GROUND_ROW,
            vel: 0,
            accel: 0,
            touch: 0,
            blunder: 0,
            score: 0,
            colour: index,
            control: if index == 0 {
                Control::Keyboard
            } else {
                Control::Mouse
            },
            skill: 0,
            intent: Intent::default(),
            frame: 0,
            anim_dir: 1,
            contact: false,
            contact_latch: false,
            contact_frames: 0,
            airborne: false,
            cooldown: 0,
        }
    }

    /// Half of the court this player defends, in whole pixels.
    ///
    /// The original clamps the players to their half: player 1 at most
    /// 0x168 (360 px), player 2 at least 0x1b8 (440 px).
    pub fn home_half(&self, index: usize) -> (i32, i32) {
        // The original clamps the players to their half (`BV_Phys_H_443e8.c`
        // :408-413): player 1 at most 0x168 (360 px), player 2 at least
        // 0x1B8 (440 px). The outer edges are 0 and 800 - the 32/768 of the
        // port were the **ball's** walls.
        if index == 0 {
            (0, 360)
        } else {
            (440, 800)
        }
    }

    pub fn reset_for_rally(&mut self, index: usize) {
        info!(
            player = index + 1,
            from_x = self.x,
            from_score = self.score,
            "rally reset"
        );
        self.x = PLAYER_SPAWN_X[index];
        self.y = GROUND_ROW;
        self.vel = 0;
        self.accel = 0;
        self.touch = 0;
        self.blunder = 0;
        self.airborne = false;
        self.frame = 0;
        self.contact = false;
        self.contact_latch = false;
        self.contact_frames = 0;
        self.cooldown = 0;
    }

    /// Walking starts the animation cycle again (`FUN_004443E8:414-417`):
    /// only a frozen counter (frame 0, direction 0) is restarted.
    fn start_walk_animation(&mut self) {
        if !self.airborne && self.frame == 0 && self.anim_dir == 0 {
            self.anim_dir = 1;
        }
    }

    /// One physics frame of player movement.
    pub fn step(&mut self, index: usize) {
        let (lo, hi) = self.home_half(index);
        let x0 = self.x;

        if self.cooldown > 0 {
            self.cooldown -= 1;
        }

        // The original walks first and clamps afterwards
        // (`BV_Phys_H_443e8.c:407-413`), so the player does stop at the
        // boundary - it just moves into it first.
        if self.intent.right {
            self.x += WALK_SPEED;
        }
        if index == 0 && self.x > 360 {
            self.x = 360;
        }
        if self.x > 800 {
            self.x = 800;
        }
        if self.intent.left {
            self.x -= WALK_SPEED;
        }
        if index == 1 && self.x < 440 {
            self.x = 440;
        }
        if self.x < 0 {
            self.x = 0;
        }
        if x0 != self.x {
            trace!(
                player = index + 1,
                x0,
                x = self.x,
                dir = if self.intent.left { "left" } else { "right" },
                "player walk"
            );
        } else if self.intent.left || self.intent.right {
            trace!(
                player = index + 1,
                x = self.x,
                limit_lo = lo,
                limit_hi = hi,
                "player blocked at half boundary"
            );
        }

        // --- the jump (FUN_00443E8, lines 432ff) ---------------------------
        if self.intent.jump && !self.airborne {
            self.airborne = true;
            self.accel = JUMP_ACCEL;
            self.vel = 0;
            // The original restarts the animation unless it stands at 4
            // (`FUN_004443E8:437`).
            if self.frame != 4 {
                self.anim_dir = 1;
            }
            debug!(
                player = index + 1,
                x = self.x,
                accel = JUMP_ACCEL,
                "jump start"
            );
        }

        if self.airborne {
            // vel += accel;  y += (vel < 0 ? vel + 0xFF : vel) >> 8;
            self.vel = self.vel.wrapping_add(self.accel);
            let mut v = self.vel;
            if v < 0 {
                v = v.wrapping_add(0xff);
            }
            self.y = self.y.wrapping_add(v >> 8);

            // Only the fraction and the sign survive into the next frame.
            let masked = (self.vel as u32 & 0x8000_00ff) as i32;
            self.vel = if masked < 0 {
                (masked.wrapping_sub(1) | -256).wrapping_add(1)
            } else {
                masked
            };

            self.accel = self.accel.wrapping_sub(JUMP_ACCEL_DECAY);
            if !self.intent.jump {
                // Without the jump key the gravity works twice as hard.
                self.accel = self.accel.wrapping_sub(JUMP_ACCEL_DECAY);
            }

            if self.y < LAND_ROW {
                self.airborne = false;
                self.y = GROUND_ROW;
                // The original resets the animation and its direction on
                // landing (`FUN_004443E8:459-460`).
                self.frame = 0;
                self.anim_dir = 1;
                debug!(
                    player = index + 1,
                    x = self.x,
                    "landing"
                );
            }
        }

        // --- the animation counter (FUN_004443E8:463-471) ------------------
        // The counter is a **pendulum**: it advances by `anim_dir` and flips
        // the direction at 4, so it runs 0,1,2,3,4,3,2,1,0. At 0 with the
        // direction -1 it freezes (the direction becomes 0) until the player
        // walks again - that is how a standing blob keeps its first frame.
        //
        // The counter drives the sprite **and** the height of the collision
        // boxes (`DAT_0044DAF4[anim]`, `DAT_0044DB1C[anim]`), so it must not
        // be a monotonic counter.
        if self.frame == 0 && self.anim_dir == -1 {
            self.anim_dir = 0;
        } else {
            let next = self.frame as i32 + self.anim_dir;
            self.frame = next.clamp(0, 4) as usize;
            if self.frame == 4 {
                self.anim_dir = -self.anim_dir;
            }
        }

        if self.x < lo || self.x > hi {
            warn!(
                player = index + 1,
                x = self.x,
                lo,
                hi,
                "player outside its half"
            );
        }
    }

    /// Screen X, reproducing the original's projection (including the
    /// `& ~1` even-pixel snap and the per-index offset).
    pub fn screen_x(&self, index: usize) -> i32 {
        let dx = if self.y >= GROUND_ROW {
            (self.y - GROUND_ROW) >> 2
        } else {
            (self.y - 0x61) >> 2
        };
        (self.x + dx + 0x18 - index as i32 * 0x18) & !1
    }

    /// Screen Y, reproducing the original's projection.
    ///
    /// This is the row of the player's **shadow**; the blob itself is drawn
    /// on the raw `player_y` row.
    pub fn screen_y(&self) -> i32 {
        let dy = if self.y >= GROUND_ROW {
            self.y - GROUND_ROW
        } else {
            self.y - 0x5D
        };
        (dy >> 3) + 0x61
    }

    /// Register a successful touch. Returns `true` when the rally is lost
    /// because the touch limit was exceeded (the original counts to 4).
    pub fn register_touch(&mut self, index: usize) -> bool {
        self.touch += 1;
        let fault = self.touch > TOUCH_LIMIT;
        if fault {
            warn!(
                player = index + 1,
                touches = self.touch,
                limit = TOUCH_LIMIT,
                "fourth touch — rally fault against this player"
            );
        } else {
            debug!(
                player = index + 1,
                touches = self.touch,
                limit = TOUCH_LIMIT,
                "ball touch"
            );
        }
        fault
    }

    /// Steer the player with the mouse, mirroring the original
    /// (`BV_Setup2.c:197ff`).
    ///
    /// The target is `mouse_x / 2 + player_index * 400`: one mouse steers
    /// every mouse controlled player inside their own half. The player only
    /// walks when the target is more than 7 pixels away.
    pub fn follow_mouse(&mut self, index: usize, mouse_x: i32) {
        // The original's dead zone is **nine** pixels
        // (`BV_Setup2.c:199-204`: `x + 9 < ziel` / `ziel < x - 9`).
        let target = mouse_x / 2 + index as i32 * 400;
        self.intent.left = target < self.x - 9;
        self.intent.right = self.x + 9 < target;
        trace!(
            player = index + 1,
            mouse_x,
            target,
            x = self.x,
            "mouse follow"
        );
    }
}