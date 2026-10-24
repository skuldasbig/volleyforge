//! Ball state and integration.
//!
//! Recovered from the original's `FUN_004441D0` (two sub-steps per frame):
//!   * position integrates velocity directly, no timestep scaling
//!   * a rising ball loses a little speed (drag), only while the serve flag
//!     `0x9782E0` is set
//!   * hitting the ground reflects with a 3/4 restitution on **both** axes
//!   * side walls mirror horizontally
//!   * the net snaps the ball to a fixed post position and mirrors `vx`
//!
//! All values are fixed point with 32 sub-units per pixel.

use crate::court::*;
use crate::log::{debug, info, trace, warn};

/// What happened to the ball during one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BallEvent {
    /// Nothing notable.
    None,
    /// Ground contact.
    Ground,
    /// Left or right wall.
    Wall,
    /// Net contact.
    Net,
}

impl BallEvent {
    fn label(&self) -> &'static str {
        match self {
            BallEvent::None => "none",
            BallEvent::Ground => "ground",
            BallEvent::Wall => "wall",
            BallEvent::Net => "net",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ball {
    /// Fixed-point position (1/32 px).
    pub x: i32,
    pub y: i32,
    /// Fixed-point velocity per sub-step.
    pub vx: i32,
    pub vy: i32,
    /// Original's `0x9782E0`: "the ball is in play".
    ///
    /// The flag is **cleared** at the start of every rally and **set** the
    /// moment the ball touches a blob (`FUN_00443BF0`). While it is clear
    /// the ball hangs in the air without gravity - that is the serve: the
    /// server has to jump up to the ball and play it.
    pub in_play: bool,
    /// The ball's previous position (0x9782F4 / 0x9782F8 in the
    /// original). FUN_00444100 needs it to detect that the ball crossed
    /// the net plane in this sub-step.
    pub prev_x: i32,
    pub prev_y: i32,
    /// Set by the rules module when the rally is over.
    pub dead: bool,
    /// Number of frames this ball has existed, for periodic dumps.
    pub age: u64,
}

impl Ball {
    /// Spawn on the centre line of `server`'s half, at the original's height.
    pub fn spawn(server: usize) -> Self {
        let x = (server as i32 * (VIEW_W / 2) + (VIEW_W / 4)) * SUBPX;
        let y = 0x2580; // 300 px
        info!(
            server = server + 1,
            x = x,
            y = y,
            px = to_px(x),
            py = to_px(y),
            "ball spawn"
        );
        Ball {
            x,
            y,
            vx: 0,
            vy: 0,
            in_play: false,
            prev_x: x,
            prev_y: y,
            dead: false,
            age: 0,
        }
    }

    pub fn px(&self) -> i32 {
        to_px(self.x)
    }

    pub fn py(&self) -> i32 {
        to_px(self.y)
    }

    /// Launch the ball with a velocity expressed in whole px per sub-step.
    pub fn launch(&mut self, vx_px: i32, vy_px: i32) {
        self.vx = vx_px * SUBPX;
        self.vy = vy_px * SUBPX;
        self.in_play = true;
        info!(
            vx_px,
            vy_px,
            x_px = self.px(),
            y_px = self.py(),
            "ball launched (in_play -> gravity on)"
        );
    }

    /// Apply a hit coming from `by_player`, sending the ball away from them.
    pub fn hit(&mut self, by_player: usize, to_x: i32, to_y: i32, speed_px: i32, lift_px: i32) {
        let dir = if by_player == 0 { 1 } else { -1 };
        self.vx = dir * speed_px * SUBPX;
        // The original's y counts from the bottom, so a ball that flies up
        // has a **positive** vy.
        self.vy = lift_px * SUBPX;
        self.in_play = true;
        self.dead = false;
        info!(
            by = by_player + 1,
            target_x = to_x,
            target_y = to_y,
            vx_px = dir * speed_px,
            lift_px,
            "ball hit"
        );
    }

    /// Advance one rendered frame: the original integrates `SUBSTEPS` times.
    pub fn step(&mut self) -> BallEvent {
        self.age += 1;
        let mut last = BallEvent::None;
        let mut hits = 0u32;
        for sub in 0..SUBSTEPS {
            let e = self.step_once(sub);
            if e != BallEvent::None {
                last = e;
                hits += 1;
                debug!(
                    substep = sub,
                    event = e.label(),
                    x_px = self.px(),
                    y_px = self.py(),
                    vx_px = self.vx / SUBPX,
                    vy_px = self.vy / SUBPX,
                    "ball collision"
                );
            }
        }

        trace!(
            frame = self.age,
            event = last.label(),
            substep_hits = hits,
            x = self.x,
            y = self.y,
            px = self.px(),
            py = self.py(),
            vx = self.vx,
            vy = self.vy,
            drag = self.in_play,
            prev_x = self.prev_x,
            prev_y = self.prev_y,
            "ball frame"
        );

        if self.age % 600 == 0 {
            debug!(
                age_frames = self.age,
                px = self.px(),
                py = self.py(),
                speed = (self.vx * self.vx + self.vy * self.vy) / (SUBPX * SUBPX),
                "ball periodic"
            );
        }

        // Guard rails: a bad tuning constant must not run the court off-screen.
        if self.x < -10_000 || self.x > from_px(VIEW_W) + 10_000 {
            warn!(
                x = self.x,
                px = self.px(),
                vx = self.vx,
                "ball escaped horizontally, clamping"
            );
            self.x = self.x.clamp(WALL_L, WALL_R);
            self.vx = -self.vx;
        }

        last
    }

    fn step_once(&mut self, sub: i32) -> BallEvent {
        self.x = self.x.wrapping_add(self.vx);
        self.y = self.y.wrapping_add(self.vy);

        // FUN_004441D0, verbatim:
        //     g = iVar5 + 2;  if (vy < 0) g -= (|vy| / 3 >> 5);
        //     if (in_play) vy -= g;
        // The gravity is a **ramp**: the sub-step counter runs from 1, so the
        // first sub-step pulls with 3 and the second with 4.
        let mut g = GRAVITY + sub;
        if self.vy < 0 {
            g -= (self.vy.unsigned_abs() as i32 / 3) >> 5;
        }
        if self.in_play {
            self.vy -= g;
        }

        let mut event = BallEvent::None;

        // Ground plane. The original rounds towards zero:
        //     vy = ((vy * -3) + 3) >> 2      vx = ((vx * 3) + 3) >> 2
        // where the `+3` is only added for negative values.
        if self.y < GROUND_Y {
            let before_vy = self.vy;
            let mut vy = self.vy * -3;
            if vy < 0 {
                vy += 3;
            }
            self.vy = vy >> 2;
            let mut vx = self.vx * 3;
            if vx < 0 {
                vx += 3;
            }
            self.vx = vx >> 2;
            self.y = GROUND_Y;
            if before_vy == 0 && self.vy == 0 {
                warn!("ground contact with zero vertical velocity — stall risk");
            }
            event = BallEvent::Ground;
        }

        // Side walls.
        if self.x < WALL_L {
            self.vx = -self.vx;
            self.x = WALL_L;
            event = BallEvent::Wall;
        } else if self.x > WALL_R {
            self.vx = -self.vx;
            self.x = WALL_R;
            event = BallEvent::Wall;
        }

        // Net. The original probes the ball centre with a +/-32 px offset and
        // only blocks while the ball is below the net top.
        let px = self.px();
        let py = self.py();
        if py - NET_PROBE_PX < NET_TOP_PX {
            if self.vx < 0 && NET_L_LO < px - NET_PROBE_PX && px - NET_PROBE_PX < NET_L_HI {
                self.x = NET_SNAP_RIGHT;
                self.vx = -self.vx;
                debug!(
                    px,
                    py,
                    probe = px - NET_PROBE_PX,
                    "net blocked from the right"
                );
                event = BallEvent::Net;
            } else if self.vx > 0
                && NET_R_LO < px + NET_PROBE_PX
                && px + NET_PROBE_PX < NET_R_HI
            {
                self.x = NET_SNAP_LEFT;
                self.vx = -self.vx;
                debug!(
                    px,
                    py,
                    probe = px + NET_PROBE_PX,
                    "net blocked from the left"
                );
                event = BallEvent::Net;
            }
        }

        // The original ends every sub-step with FUN_00444100 - the net
        // circle. It keeps the previous position and snaps the ball back
        // to the side it came from when it crosses the net plane low
        // enough.
        if self.net_circle() {
            event = BallEvent::Net;
        }
        event
    }

    /// FUN_00444100: the net circle.
    ///
    /// The ball keeps its previous position (0x9782F4 / 0x9782F8). When
    /// it passes the net plane (384 px) at a low height it is snapped back to
    /// the side it came from (360 px or 440 px) and x is mirrored.
    fn net_circle(&mut self) -> bool {
        let x = self.x;
        let y = self.y;
        let old_x = self.prev_x;
        let old_y = self.prev_y;

        let in_band = NET_CIRCLE_LO < x && x < NET_CIRCLE_HI;
        let crossed = (NET_PLANE < old_x) != (NET_PLANE < x) && self.in_play;

        let mut hit = false;
        if (in_band || crossed) && y < NET_CIRCLE_Y && old_y < NET_CIRCLE_OLD_Y {
            self.x = if old_x < NET_CIRCLE_OLD_X {
                NET_CIRCLE_SNAP_L
            } else {
                NET_CIRCLE_SNAP_R
            };
            self.y = old_y;
            self.vx = -self.vx;
            hit = true;
            debug!(
                x_px = to_px(x),
                old_x_px = to_px(old_x),
                y_px = to_px(y),
                snap_px = to_px(self.x),
                "net circle - the ball is pushed back to its side"
            );
        }

        // The previous position is always the current one at the end of the
        // sub-step (FUN_00444100:22-23).
        self.prev_x = self.x;
        self.prev_y = self.y;
        hit
    }
}