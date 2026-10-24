//! Rally and match rules.
//!
//! Recovered from `FUN_00445890` (match setup / scoring loop) and the
//! collision block in `FUN_00443E8`:
//!
//! ```text
//! score[server] += 1        when the rally resolves in the server's favour
//! server = 1 - server       when it resolves against them
//! rally returns -1          when the match is over
//!
//! loop while  (s1 < 15 && s2 < 15) || |s1 - s2| < 2
//! ```
//!
//! i.e. the match ends once somebody has at least 15 points **and** leads by
//! at least 2 — exactly the rule stated in the original's `Anleitung.txt`.
//! A fourth ball touch inside one rally is a fault.

use crate::ball::{Ball, BallEvent};
use crate::court::*;
use crate::log::{debug, info, trace, warn};
use crate::player::{
    Control, Intent, Player, COLLISION_BOX_Y, COLLISION_RADIUS_SQ, TOUCH_BOX_Y, TOUCH_RADIUS_SQ,
};

/// The gravity constant of the original's landing point prediction,
/// stored negated: `_DAT_004439E4` = -3.5.
///
/// 3.5 is the mean of the two sub-step gravities 3 and 4 that
/// `FUN_004441D0` applies per frame.
const CPU_PREDICT_G: f64 = -3.5;

/// The value `FUN_004438F4` returns when the ball never reaches the
/// requested height (`0xFFFFD8F0`).
const CPU_PREDICT_INVALID: i32 = -10000;

/// The original's two prediction heights in whole pixels: the normal target
/// (200 px) and the hard opponent's second target (350 px).
const CPU_PREDICT_H: i32 = 200;
const CPU_PREDICT_H2: i32 = 350;

/// `_DAT_004440CC` (0x44440CC): the tolerance of the original's touch test
/// `ball_vy*dy + ball_vx*dx <= _DAT_004440CC * dist * speed`.
///
/// The binary holds the float `0x33333333` there, that is practically zero:
/// the ball must not be moving **away** from the blob.
const TOUCH_TOLERANCE: f64 = 4.172_325_134_277_344e-8;

/// `_DAT_004440DC` = 3481: the push circle's push loop ends when the ball
/// is more than 59 px away from the collision box again.
const PUSH_EXIT_RADIUS_SQ: f64 = 3481.0;

/// `_DAT_004440E4` = 4356: the same for the touch circle (66 px).
const TOUCH_EXIT_RADIUS_SQ: f64 = 4356.0;

/// The original shows the finished rally for about two seconds before the next
/// serve (FUN_004443E8:665-669, 751-757). 60 frames at 30 fps.
const AFTERGLOW_FRAMES: u32 = 60;

/// The strength of a hit in fixed units: `_DAT_004440D8` (8.0) times
/// `_DAT_004440C4` (32.0), that is 8 px per sub-step, always the same -
/// only the direction of the unit vector varies.
const PUSH_STRENGTH: f64 = 8.0 * 32.0;

/// How a rally ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RallyResult {
    /// Rally still running.
    Running,
    /// Point awarded to the given player index.
    Point(usize),
    /// The ball came to rest on the floor.
    Dead,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchState {
    /// Waiting for the player to confirm the next serve.
    ServeReady,
    /// Ball in play.
    Rally,
    /// Somebody reached the winning condition.
    Finished(usize),
}

/// Deterministic little LCG. The original used `Random(2)` for the first
/// serve; we keep it deterministic so a rally can be reproduced exactly.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u32);

impl Rng {
    pub const fn new(seed: u32) -> Self {
        Rng(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        // Numerical Recipes LCG, 32-bit.
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

pub struct Match {
    pub players: [Player; 2],
    pub ball: Ball,
    pub state: MatchState,
    pub server: usize,
    /// Whose floor the ball last rested on, if any.
    pub resting_on: Option<usize>,
    /// Frames the ball has been resting.
    pub rest_frames: u32,
    /// Frames since the serve started. The original measures the serve phase
    /// in milliseconds (`GetTickCount`), the port in frames at 200 Hz.
    pub serve_frames: u32,
    /// Number of physics frames since the match was created - only used by
    /// the frame dump ([`Match::trace_frame`]).
    pub frame_number: u64,
    /// The opening rally of a match is **not** scored: the original calls
    /// the rally with the "first serve" flag (`BV_Phys_I_45890.c:33`).
    pub first_rally: bool,
    /// Frames the finished rally stays on screen before the next serve. The
    /// original waits about two seconds and both blobs back on the floor
    /// (`FUN_004443E8:665-669, 751-757`).
    pub afterglow_frames: u32,
    /// How many rallies have been served in this match.
    ///
    /// The original hides the serve indicator during the **first** rally
    /// (`BV_Phys_H_443e8.c:616-627`): the flag is only set when
    /// `local_12 == '\0'`, and `local_12` is the "first serve" flag that
    /// `FUN_00445890.c:33` passes for the very first rally. In that rally
    /// the service is not settled yet - if the server loses it, the
    /// service changes without a point.
    pub rallies_started: u32,
    rng: Rng,
    /// Human-controlled player index, if any.
    pub human: Option<usize>,
    /// Match finished, used to freeze the ball.
    pub last_point: Option<usize>,
    /// The server the **host** announced, if this is a network match.
    ///
    /// The original's guest learns the server from the score packet: byte 2
    /// carries the player index, `0xFF` means "no indicator" - that is the
    /// case during the very first rally, where the service is not settled yet
    /// (`BV_Phys_H_443e8.c:112-115`). `None` means "this is a local match".
    pub announced_server: Option<u8>,
    /// A sound that has to be sent to the other computer.
    ///
    /// The original plays the sound locally **and** tells the other
    /// computer about it (`BV_MessageByIndex.c:8-14`); the guest only ever
    /// hears the sounds of the host, because it never touches the ball.
    pub sound_request: Option<u8>,
}

impl Match {
    pub fn new(seed: u32, human: Option<usize>) -> Self {
        let mut players = [Player::new(0), Player::new(1)];
        if let Some(h) = human {
            players[h].control = Control::Keyboard;
            players[1 - h].control = Control::Mouse;
            info!(human = h + 1, "match created with a designated human player");
        } else {
            info!("match created");
        }
        let mut rng = Rng::new(seed);
        // The original picks the first server at random - `Random(2)`
        // (`BV_Phys_I_45890.c:32`).
        let server = rng.below(2) as usize;
        info!(server = server + 1, "first server (Random(2))");
        Match {
            players,
            ball: Ball::spawn(server),
            state: MatchState::ServeReady,
            server,
            resting_on: None,
            rest_frames: 0,
            serve_frames: 0,
            frame_number: 0,
            first_rally: true,
            // The first serve happens right away - the afterglow only
            // applies after a finished rally.
            afterglow_frames: AFTERGLOW_FRAMES,
            rallies_started: 0,
            rng,
            human,
            last_point: None,
            announced_server: None,
            sound_request: None,
        }
    }

    /// Start a new rally with `server` at the service position.
    ///
    /// The original (`FUN_004443E8`, lines 88ff) places the ball directly
    /// above the server with `vx = vy = 0` and clears the flag `0x9782E0`, so
    /// the ball **hangs in the air** until a blob touches it. The server has
    /// to jump up to it and play it - the ball is never thrown.
    pub fn serve(&mut self) {
        info!(
            server = self.server + 1,
            score = ?[self.players[0].score, self.players[1].score],
            "--- rally starts ---"
        );
        for i in 0..2 {
            self.players[i].reset_for_rally(i);
        }
        self.ball = Ball::spawn(self.server);
        // The original does **not** throw the ball: it hangs at 0x2580 with
        // vx = vy = 0 and the gravity flag cleared, until a blob touches it
        // (`FUN_004443E8`, lines 88ff). The server has to jump up to it.
        info!(
            server = self.server + 1,
            px = self.ball.px(),
            py = self.ball.py(),
            "serve - the ball hangs until the server plays it"
        );
        self.state = MatchState::Rally;
        self.resting_on = None;
        self.rest_frames = 0;
        self.serve_frames = 0;
        self.rallies_started += 1;
    }

    /// The player whose scoreboard carries the serve indicator.
    ///
    /// The original draws an exclamation mark next to the score of the
    /// player who has the service (`FUN_00443694`, the third cell at
    /// `+0x1a5EC0`), but **not** during the first rally: the flag needs
    /// `local_12 == '\0'` (`BV_Phys_H_443e8.c:616-627`), and `local_12` is
    /// the "first serve" flag of the very first rally. In that rally the
    /// service is not settled yet - if the server loses it, the service
    /// changes without a point, so the original does not show it.
    ///
    /// `None` therefore means "no indicator at all".
    ///
    /// In a network match the **host** decides: the guest takes the server
    /// from the score packet, where `0xFF` means "no indicator" - the case
    /// during the first rally.
    pub fn serve_indicator(&self) -> Option<usize> {
        if let Some(announced) = self.announced_server {
            if announced == 0xFF {
                return None;
            }
            return Some(announced as usize);
        }
        if self.rallies_started >= 2 {
            Some(self.server)
        } else {
            None
        }
    }

    /// The original's match point flag (`BV_Phys_H_443e8.c:39-46`): the
    /// server has more than 13 points and leads.
    ///
    /// While the flag is set, the scoreboard blinks a **second** exclamation
    /// mark next to the serve indicator (`FUN_00443694`, the fourth cell at
    /// `+0x1a5ED0`, drawn while `GetTickCount() & 0x100 == 0`).
    pub fn is_match_point(&self) -> bool {
        let s = self.server;
        self.players[s].score > 13 && self.players[1 - s].score < self.players[s].score
    }

    /// True once somebody has >= WIN_SCORE and leads by >= WIN_LEAD.
    pub fn is_over(&self) -> Option<usize> {
        let a = self.players[0].score;
        let b = self.players[1].score;
        let leader = if a >= b { 0 } else { 1 };
        let lead = (a - b).abs();
        let done = (a >= WIN_SCORE || b >= WIN_SCORE) && lead >= WIN_LEAD;
        trace!(
            score = ?[a, b],
            lead,
            need_score = WIN_SCORE,
            need_lead = WIN_LEAD,
            over = done,
            "match condition check"
        );
        if done {
            Some(leader)
        } else {
            None
        }
    }

    /// Feed one player's intent for this frame.
    pub fn set_intent(&mut self, index: usize, intent: Intent) {
        self.players[index].intent = intent;
    }

    // -----------------------------------------------------------------------
    // The network: the host simulates, the guest mirrors
    // -----------------------------------------------------------------------

    /// The state the host sends every frame.
    ///
    /// The original sends the ball, both blobs and their animation counters
    /// (`BV_Phys_H_443e8.c:735-747`); the guest overwrites its own state
    /// with it (`BV_Menu_0044BF98.c:134-140`).
    pub fn net_state(&self) -> crate::net::State {
        crate::net::State {
            ball_x: self.ball.x,
            ball_y: self.ball.y,
            p0: crate::net::Point {
                x: self.players[0].x,
                y: self.players[0].y,
            },
            p1: crate::net::Point {
                x: self.players[1].x,
                y: self.players[1].y,
            },
            anim0: self.players[0].frame as u32,
            anim1: self.players[1].frame as u32,
        }
    }

    /// Take over the state the host sent.
    ///
    /// The original's guest does exactly this and nothing else: it never
    /// steps the physics, it only draws what the host tells it
    /// (`FUN_0044BF98`).
    pub fn apply_net_state(&mut self, state: &crate::net::State) {
        self.ball.x = state.ball_x;
        self.ball.y = state.ball_y;
        self.players[0].x = state.p0.x;
        self.players[0].y = state.p0.y;
        self.players[1].x = state.p1.x;
        self.players[1].y = state.p1.y;
        self.players[0].frame = state.anim0 as usize;
        self.players[1].frame = state.anim1 as usize;
    }

    /// Take over the score the host sent.
    ///
    /// The original's score packet carries the two scores, the player who
    /// serves (`0xFF` = no serve indicator), the match point flag and the
    /// "match over" flag (`BV_Phys_H_443e8.c:106-117`).
    pub fn apply_net_score(&mut self, p0: u8, p1: u8, server: u8, over: bool) {
        self.players[0].score = p0 as i32;
        self.players[1].score = p1 as i32;
        self.announced_server = Some(server);
        if server != 0xFF {
            self.server = server as usize;
        }
        if over {
            if let Some(w) = self.is_over() {
                self.state = MatchState::Finished(w);
                self.ball.dead = true;
                info!(winner = w + 1, "the host ended the match");
            }
        }
    }

    /// Advance the whole match by one physics frame.
    pub fn step(&mut self) -> RallyResult {
        self.frame_number += 1;
        if self.state == MatchState::ServeReady {
            // The original's frame loop keeps running while the finished
            // rally is still on the screen: the blobs walk on and land, the
            // ball keeps moving (`BV_Phys_H_443e8.c:394, 403-473, 477`).
            // Only the computer block and the two collision calls are
            // switched off (`:246, :391, :474`).
            //
            // This is not a cosmetic detail. A blob that ends the rally in
            // mid-air (the touch limit fault of `:482-526` fires right in
            // the air) has to land again - otherwise the "both blobs are on
            // the floor" condition of `:751-757` never comes true, the next
            // serve never starts and the game freezes with the ball in the
            // air.
            self.physics();
            for i in 0..2 {
                self.players[i].step(i);
            }
            self.physics();

            // The original keeps the finished rally on screen for about two
            // seconds and waits until both blobs stand on the floor again
            // (`FUN_004443E8:665-669` sets `local_24 = GetTickCount()`,
            // `:751-757` returns only when `local_24 + 2000 < now` and both
            // blobs are on the floor). 60 frames are two seconds at 30 fps.
            self.afterglow_frames += 1;
            let grounded = self.players[0].y == crate::player::GROUND_ROW
                && self.players[1].y == crate::player::GROUND_ROW;
            if self.afterglow_frames >= AFTERGLOW_FRAMES && grounded {
                self.serve();
            }
            return RallyResult::Running;
        }
        if self.state != MatchState::Rally {
            return RallyResult::Running;
        }

        // --- the serve phase ------------------------------------------------
        // The ball hangs in the air until a blob touches it: the original
        // clears the flag `0x9782E0` and leaves `vx = vy = 0`
        // (`FUN_004443E8:90-97`).
        //
        // The original's **automatic** jump (the 500 ms lock and the server
        // jumping after one second) lives **only** in the computer opponent
        // block (`FUN_004443E8:373-383`), which runs for the control modes
        // 2, 3 and 4. A **human** player gets the jump flag straight from
        // the input: `0x52 = KeyTable[player+0x34]` for the keyboard
        // (line 202) and `0x52 = (0x97D22C == 1)` for the mouse (line 220).
        // So a human has to jump by himself - the ball simply waits in the
        // air until somebody hits it.
        if !self.ball.in_play {
            self.serve_frames += 1;
        }

        // --- the computer opponents decide -------------------------------
        // The original's CPU block runs **before** the collision
        // (`FUN_004443E8:246-387`), so the computer's intent is already
        // known when the ball is checked against the blobs.
        for i in 0..2 {
            if self.players[i].control == Control::Cpu {
                self.drive_cpu(i);
            }
        }

        // --- collision #1 ----------------------------------------------------
        // The blobs still stand at the positions of the previous frame: the
        // original checks the collision **before** the players move
        // (`FUN_004443E8:392`).
        self.collide();

        // --- ball physics #1 -------------------------------------------------
        // (`FUN_004443E8:394`)
        let mut ground = self.physics() == BallEvent::Ground;

        // The touch counter is reset by the ball's position
        // (`BV_Phys_H_443e8.c:395-402`): the counter of the player whose half
        // the ball is *not* in is cleared, so touches only pile up at the net.
        self.reset_touch_counters();

        // --- players move ----------------------------------------------------
        // The original walks and jumps here (`FUN_004443E8:403-473`).
        for i in 0..2 {
            self.players[i].step(i);
        }

        // --- collision #2 ----------------------------------------------------
        // The original checks a **second** time, now with the blobs at their
        // new positions (`FUN_004443E8:475`) - that is how a blob can still
        // hit the ball in the very frame in which it jumps.
        self.collide();

        // --- ball physics #2 -------------------------------------------------
        // (`FUN_004443E8:477`) - in the original the ball moves **four**
        // sub-steps per frame (two physics calls with two sub-steps each).
        if self.physics() == BallEvent::Ground {
            ground = true;
        }

        // --- resolution ------------------------------------------------------
        // The original resolves the rally at the end of the frame
        // (`FUN_004443E8:478-539`): first the fault (the fourth touch), then
        // the ground contact.
        // --- the touch counter and the fault -------------------------------
        // BV_Phys_H_443e8.c:482-526: the counter only rises on the **edge**
        // of a contact (the flag +0x38 is set and the latch +0x39 is
        // still clear). If the contact lasts longer than 40 frames
        // (+0x40 > 0x28) it is a fault as well - holding the ball on the
        // blob is not allowed.
        let mut fault = None;
        for i in 0..2 {
            if self.players[i].contact {
                if !self.players[i].contact_latch {
                    self.players[i].touch += 1;
                    // The original plays sound B on the contact edge
                    // (`BV_Phys_H_443e8.c:485`: the touch counter is
                    // raised and `FUN_00442964` is called with the second
                    // sound).
                    crate::sound::sound_b();
                    // The original tells the other computer about the sound
                    // as well (`BV_MessageByIndex.c:8-14`).
                    self.sound_request = Some(1);
                    if self.players[i].touch > TOUCH_LIMIT {
                        fault = Some(i);
                    }
                } else {
                    self.players[i].contact_frames += 1;
                    if self.players[i].contact_frames > 0x28 {
                        fault = Some(i);
                    }
                }
            } else {
                self.players[i].contact_frames = 0;
            }
            // The latch is updated at the end of the frame
            // (BV_Phys_H_443e8.c:526) and the flag is cleared at the start
            // of the next one (BV_Phys_H_443e8.c:388-389).
            self.players[i].contact_latch = self.players[i].contact;
            self.players[i].contact = false;
        }
        if let Some(faulty) = fault {
            // The original halves the ball's velocity on a fault
            // (BV_Phys_H_443e8.c:490-501).
            self.ball.vx /= 2;
            self.ball.vy /= 2;
            warn!(
                player = faulty + 1,
                reason = "touch limit",
                "rally fault - the other player wins the rally"
            );
            return self.award(1 - faulty);
        }

        // The original ends the rally on the **first** ground contact
        // (`BV_Phys_G_441d0.c:41-46` sets a flag there) and awards the point
        // to the player whose half the ball did NOT land in.
        if ground {
            let loser = if self.ball.px() < CENTRE_X { 0 } else { 1 };
            info!(
                loser = loser + 1,
                px = self.ball.px(),
                py = self.ball.py(),
                "ball touched down - point"
            );
            return self.award(1 - loser);
        }

        // Match end check after every resolution.
        if let Some(w) = self.is_over() {
            self.state = MatchState::Finished(w);
            self.ball.dead = true;
            return RallyResult::Point(w);
        }

        RallyResult::Running
    }

    /// The blob-versus-ball collision (`FUN_00443BF0`).
    ///
    /// The original runs this **twice** per frame: once before the players
    /// move and once after (`FUN_004443E8:392` and `:475`). The second run
    /// sees the blobs at their new positions, so a blob can still hit the
    /// ball in the very frame in which it jumps.
    ///
    /// The contact flag +0x38 is set here and evaluated in Match::step.
    fn collide(&mut self) {
        let mut hit = false;
        for i in 0..2 {
            // Copy the values out: the push below needs `&mut self`.
            let (player_x, player_y, frame) = {
                let p = &self.players[i];
                (p.x, p.y, p.frame)
            };

            // The original reads the ball's velocity **before** the push
            // (`FUN_00443BF0:33` computes the speed from it and the touch
            // test at lines 52-54 uses it).
            let vx0 = self.ball.vx as f64;
            let vy0 = self.ball.vy as f64;
            let speed = (vx0 * vx0 + vy0 * vy0).sqrt();

            // --- first collision circle: the push circle (radius 57 px) -----
            let box_x = player_x as f64;
            let box_y = (player_y + 0x65 - COLLISION_BOX_Y[frame]) as f64;
            let dx = self.ball.px() as f64 - box_x;
            let dy = self.ball.py() as f64 - box_y;
            let dist_sq = dx * dx + dy * dy;
            let mut touch_event = false;

            if dist_sq < COLLISION_RADIUS_SQ {
                let dist = dist_sq.sqrt().max(0.001);
                // The normal points from the blob's collision box to the ball.
                let (nx, ny) = (dx / dist, dy / dist);

                // The original only counts a touch when the ball
                // **approaches** the blob (`FUN_00443BF0:52-54`):
                //     ball_vy*dy + ball_vx*dx <= _DAT_004440CC * dist * speed
                // `_DAT_004440CC` (0x44440CC) holds 0x33333333, that is
                // practically zero - the ball must not be moving away.
                if vy0 * dy + vx0 * dx <= TOUCH_TOLERANCE * dist * speed {
                    touch_event = true;
                }

                // From now on the ball is in play (`0x9782E0 = 1`).
                self.ball.in_play = true;
                self.ball.dead = false;

                // The new velocity is the **unit vector** times
                // `_DAT_004440D8` (8.0) times `_DAT_004440C4` (32.0) - that
                // is 256 fixed units, i.e. a constant 8 px per sub-step.
                // `FUN_00402944` is the runtime's float-to-int conversion
                // (`FISTP`), **not** a random number: the hit is always the
                // same strength, only the direction varies.
                self.ball.vx = (nx * PUSH_STRENGTH).round() as i32;
                self.ball.vy = (ny * PUSH_STRENGTH).round() as i32;
                self.push_out_of(box_x, box_y, PUSH_EXIT_RADIUS_SQ);
                hit = true;
            }

            // --- second collision circle: the touch circle (radius 64 px) ---
            // The original pushes the ball a **second** time here
            // (`FUN_00443BF0:97-158`), with the **new** velocity of the
            // first circle and the touch box, which sits lower - on the
            // height where the ball actually rests. That second push is
            // what keeps the ball from being knocked into the sand when
            // the blob jumps past it.
            let tbox_x = player_x as f64;
            let tbox_y = (player_y + 0x65 - TOUCH_BOX_Y[frame]) as f64;
            let tdx = self.ball.px() as f64 - tbox_x;
            let tdy = self.ball.py() as f64 - tbox_y;
            let tdist_sq = tdx * tdx + tdy * tdy;

            if tdist_sq < TOUCH_RADIUS_SQ {
                let tdist = tdist_sq.sqrt().max(0.001);
                let (tnx, tny) = (tdx / tdist, tdy / tdist);
                let vx1 = self.ball.vx as f64;
                let vy1 = self.ball.vy as f64;
                let speed1 = (vx1 * vx1 + vy1 * vy1).sqrt();
                if vy1 * tdy + vx1 * tdx <= TOUCH_TOLERANCE * tdist * speed1 {
                    touch_event = true;
                }
                self.ball.in_play = true;
                self.ball.dead = false;
                self.ball.vx = (tnx * PUSH_STRENGTH).round() as i32;
                self.ball.vy = (tny * PUSH_STRENGTH).round() as i32;
                self.push_out_of(tbox_x, tbox_y, TOUCH_EXIT_RADIUS_SQ);
                hit = true;
            }

            if touch_event {
                // The original sets the contact flag `+0x38` in **both**
                // collision loops (`FUN_00443BF0`, lines 55 and 117) and
                // clears the **opponent's** touch counter (`+0x3C`, lines 56
                // and 118) - a player only collects touches while the ball is
                // in his own half.
                self.players[i].contact = true;
                self.players[1 - i].touch = 0;
            }
        }
        if hit {
            self.resting_on = None;
            self.rest_frames = 0;
        }
    }

    /// One physics pass of the ball (`FUN_004441D0`, two sub-steps).
    ///
    /// The original calls this twice per frame, so the ball moves four
    /// sub-steps per frame.
    fn physics(&mut self) -> BallEvent {
        self.ball.step()
    }

    /// Reset the touch counter of the player whose half the ball is not in
    /// (`BV_Phys_H_443e8.c:395-402`).
    fn reset_touch_counters(&mut self) {
        const TOUCH_BAND_LO: i32 = 0x30C1; // 390 px
        const TOUCH_BAND_HI: i32 = 0x3340; // 410 px
        let ball_left = self.ball.x < TOUCH_BAND_LO;
        let ball_right = self.ball.x >= TOUCH_BAND_HI;
        if ball_right {
            self.players[0].touch = 0;
        }
        if ball_left {
            self.players[1].touch = 0;
        }
    }

    /// Resolve the outcome of a rally.
    ///
    /// The original (`BV_Phys_I_45890.c:47-54`, verified in the byte code
    /// at `0x4459a1`):
    ///
    /// ```text
    /// if (r ==  1) score[server]++;         // the server won the rally
    /// else if (r == 0) server = 1 - server;  // the server lost: NO point
    /// else if (r == -1) return;              // abort
    /// ```
    ///
    /// So **only the server scores**: if the server loses the rally the
    /// service simply changes hands and nobody gets a point - exactly what
    /// the original's manual says: "One player has the service and gets one
    /// point every time the opponent makes a mistake. If the player having
    /// the service makes a mistake, he loses his service to his opponent."
    ///
    /// `winner` is the player who did **not** lose the rally - the same
    /// player the original uses as `1 - server` when the service changes.
    fn award(&mut self, winner: usize) -> RallyResult {
        let before = [self.players[0].score, self.players[1].score];
        let server = self.server;
        // The original plays sound A when the rally is over
        // (`BV_Phys_H_443e8.c:668`: the first sound of `sound.pak`).
        crate::sound::sound_a();
        // The other computer has to hear it as well.
        self.sound_request = Some(0);
        // The opening rally of a match is not scored
        // (`BV_Phys_I_45890.c:33` calls the rally with the "first serve"
        // flag, which suppresses the point).
        let first_rally = self.first_rally;
        self.first_rally = false;
        let scored = winner == server && !first_rally;

        if scored {
            self.players[server].score += 1;
        }
        let after = [self.players[0].score, self.players[1].score];
        if scored {
            info!(
                point_for = server + 1,
                before = ?before,
                after = ?after,
                next_server = winner + 1,
                "POINT"
            );
        } else {
            info!(
                before = ?before,
                after = ?after,
                next_server = winner + 1,
                reason = if first_rally {
                    "opening rally is not scored"
                } else {
                    "the server lost the rally - the service changes, no point"
                },
                "NO POINT"
            );
        }
        self.last_point = Some(winner);
        // The rally winner always serves next (`server = 1 - server`).
        self.server = winner;

        if after[server] > WIN_SCORE {
            debug!(
                score = after[server],
                documented_limit = WIN_SCORE,
                "score above the documented win threshold"
            );
        }

        if let Some(w) = self.is_over() {
            self.state = MatchState::Finished(w);
            self.ball.dead = true;
            info!(
                winner = w + 1,
                final_score = ?[after[0], after[1]],
                "=== MATCH OVER ==="
            );
            return RallyResult::Point(w);
        }

        // The original keeps the finished rally on screen for about two
        // seconds before the next serve (`FUN_004443E8:665-669, 751-757`:
        // it returns only when `local_24 + 2000 < now` and both blobs stand
        // on the floor again).
        self.afterglow_frames = 0;
        self.state = MatchState::ServeReady;
        RallyResult::Point(winner)
    }

    /// The original's computer opponents.
    ///
    /// Port of the CPU block in `FUN_004443E8` (lines 246ff). The original
    /// does **not** chase the ball: it predicts the point at which the ball
    /// will reach a given height (`FUN_004438F4`) and walks towards that
    /// spot. The three computer opponents are the original's control modes
    /// 2, 3 and 4 (easy / average / hard); the port keeps them in
    /// [`Player::skill`] (0, 1, 2).
    pub fn drive_cpu(&mut self, cpu_index: usize) {
        if self.state != MatchState::Rally {
            self.players[cpu_index].intent = Intent::default();
            return;
        }

        // The original's control mode of this player (2 = easy, 3 = average,
        // 4 = hard) - `FUN_004443E8:272, 316`.
        let mode = self.players[cpu_index].skill.clamp(0, 2) as i32 + 2;
        // `-1` on the left half, `+1` on the right half
        // (`FUN_004443E8:252-256`).
        let dir: i32 = if cpu_index == 0 { -1 } else { 1 };

        let ball_x = to_px(self.ball.x);
        let ball_y = to_px(self.ball.y);

        // --- the target ----------------------------------------------------
        let mut target = self.predict_landing_x(CPU_PREDICT_H);
        let have_target = target > CPU_PREDICT_INVALID;
        // `local_2d`: the hard opponent's "wait in the middle" flag.
        let mut hard = false;

        if have_target {
            if mode == 4 {
                // `FUN_004443E8:273-288`: a slow ball on the far side makes
                // the hard opponent wait in the middle instead of chasing.
                hard = self.ball.vx.unsigned_abs() as i32 * 4 < (ball_x - 400) * -dir;
                if hard {
                    target = 400;
                }
                if (target - 400).abs() > 100 {
                    // Second prediction with a higher target height.
                    let t2 = self.predict_landing_x(CPU_PREDICT_H2);
                    if t2 > CPU_PREDICT_INVALID && ball_y > 299 {
                        hard = true;
                        target = t2;
                    }
                }
            }

            // --- mirror the target into the player's own half ---------------
            // `FUN_004443E8:289-314`.
            if cpu_index == 1 {
                if target > 0x300 {
                    target = 0x600 - target;
                }
                if target < 0x1B5 {
                    target = 0x36A - target;
                }
                if target < 500 {
                    target -= 0x20;
                }
            } else {
                if target < 0x20 {
                    target = 0x40 - target;
                }
                if target > 0x16B {
                    target = 0x2D6 - target;
                }
                if target > 300 {
                    target += 0x20;
                }
            }

            // 21 px in front of the ball, in the direction of the net.
            target += dir * 0x15;

            // --- the easy opponent's blunder counter -----------------------
            // `FUN_004443E8:316-325`.
            if mode == 2 {
                if self.players[cpu_index].blunder > 0 {
                    target = dir * 200 + 400;
                    self.players[cpu_index].blunder -= 1;
                }
                if self.ball.vy > 0 {
                    self.players[cpu_index].blunder = self.rng.below(7) as i32 + 7;
                }
            }

            // --- walk towards the target (7 px dead zone) ------------------
            let px = self.players[cpu_index].x;
            self.players[cpu_index].intent.right = px + 7 <= target;
            self.players[cpu_index].intent.left = target <= px - 7;
        }

        // The original stops the computer while it is in contact with the
        // ball (`BV_Phys_H_443e8.c:369-372`): the contact flag `+0x38`
        // clears the movement flags, the jump flag is not affected.
        if self.players[cpu_index].contact {
            self.players[cpu_index].intent.left = false;
            self.players[cpu_index].intent.right = false;
        }
        // --- the jump -------------------------------------------------------
        // `FUN_004443E8:329-367`.
        let px = self.players[cpu_index].x;
        let py = self.players[cpu_index].y;
        let mut jump = false;

        // (1) the ball is inside the blob's reach window.
        if self.ball.vy < 0x0B
            && dir * (px - ball_x) < 0x15
            && (ball_x - px) * dir < 7
            && (ball_x - px).abs() < 0x78
            && self.ball.vx.abs() < 0x41
            && ball_y > 200
            && ball_y - py > 0x46
            && (ball_y < 400 || self.ball.vy == 0)
            && self.ball.vx.abs() < 0x14
        {
            jump = true;
        }

        // (2) the original jumps with a probability of one in three hundred
        // (`FUN_004443E8:343`).
        if self.rng.below(300) == 0 {
            jump = true;
        }

        if have_target {
            // (3) the ball will land far away but is close to the player.
            if (target - px).abs() > 0x4B
                && (ball_x - px).abs() < 0x41
                && self.ball.vy < 0
                && mode != 4
            {
                jump = true;
            }
            // (4) the hard opponent jumps next to the ball.
            if hard
                && (target + dir * 5 - px).abs() < 0x2D
                && ball_y > 0xFA
                && (px - ball_x).abs() < self.ball.vx.abs()
                && ball_y < 600
            {
                jump = true;
            }
        }

        // (5) the serve phase: the jump is locked for 500 ms and the server
        // jumps after one second (`FUN_004443E8:373-383`). The port counts
        // frames at 30 fps, so 15 frames are 500 ms and 30 frames one second.
        if !self.ball.in_play {
            if self.serve_frames < 15 {
                jump = false;
            }
            if self.serve_frames >= 30 && cpu_index == self.server {
                jump = true;
            }
        }

        self.players[cpu_index].intent.jump = jump;
    }

    /// Push the ball out of the blob the way the original does
    /// (`FUN_00443BF0`): the velocity is set **once**, then the ball is
    /// moved in tenth steps (`x += vx/10`, `y += vy/10`) until it is more
    /// than `radius_sq` away from the collision box again. The geometry is
    /// recomputed after every step (`FUN_004439EC` for the push circle,
    /// `FUN_00443AB4` for the touch circle).
    ///
    /// This is what keeps the hit from being repeated in the next frame:
    /// the ball leaves the circle in the same frame, so it is pushed **once**
    /// and then flies straight. Without it the port pushed the ball four
    /// times in a row (it stayed inside the circle), and every push tilted
    /// the ball a bit further to the side.
    fn push_out_of(&mut self, box_x: f64, box_y: f64, radius_sq: f64) {
        // The original has no limit - the loop ends because the ball moves
        // away from the box. The cap is a port safeguard against a zero
        // velocity (the original would hang in that case).
        for _ in 0..256 {
            self.ball.x = self.ball.x.wrapping_add(self.ball.vx / 10);
            self.ball.y = self.ball.y.wrapping_add(self.ball.vy / 10);
            let dx = self.ball.px() as f64 - box_x;
            let dy = self.ball.py() as f64 - box_y;
            if dx * dx + dy * dy > radius_sq {
                break;
            }
        }
    }

    /// Predict the x position at which the ball reaches `height_px`.
    ///
    /// Port of `FUN_004438F4`: the ballistic solution of
    /// `y(t) = y + vy*t - g/2*t²` for `y(t) = height_px`. The original stores
    /// the gravity negated (`_DAT_004439E4` = -3.5); 3.5 is the mean of the
    /// two sub-step gravities 3 and 4.
    ///
    /// The result is the ball's x in whole pixels (the original converts the
    /// fixed point value with `FUN_00405C32`, i.e. a division by 32).
    /// [`CPU_PREDICT_INVALID`] means "the ball never reaches that height".
    ///
    /// The original rounds the double to an integer before dividing it by 32
    /// (`FUN_00402944`); the port rounds to the nearest integer as well, the
    /// difference to the original is at most 1/32 px.
    fn predict_landing_x(&self, height_px: i32) -> i32 {
        let vy = self.ball.vy as f64;
        let y = self.ball.y as f64;
        let f1 = -vy / CPU_PREDICT_G;
        let f3 = f1 * f1 - (2.0 * y) / CPU_PREDICT_G + (height_px as f64 * 64.0) / CPU_PREDICT_G;
        if f3 < 0.0 {
            return CPU_PREDICT_INVALID;
        }
        let root = f3.sqrt();
        let mut t = f1 + root;
        if t < 0.0 {
            t = f1 - root;
        }
        if t <= 0.0 {
            return CPU_PREDICT_INVALID;
        }
        let x = self.ball.x as f64 + self.ball.vx as f64 * t;
        (x.round() as i64 / 32) as i32
    }

    /// Write the state of this frame into the log.
    ///
    /// The original shipped without any diagnostics, which makes a physics
    /// problem hard to chase down. The port therefore writes **one event per
    /// physics frame** - 200 per second - with the complete state of the ball
    /// and of both blobs.
    ///
    /// The event goes into the normal log (`logs/volleyforge.log` and the
    /// console) as soon as the level is raised:
    ///
    /// ```text
    /// VOLLEYFORGE_LOG=trace volleyforge
    /// ```
    ///
    /// Without that, the frame dump stays off - the default level is `info`.
    pub fn trace_frame(&self, tick_ms: u64) {
        trace!(
            f = self.frame_number,
            t = tick_ms,
            st = %state_name(self.state),
            srv = self.server + 1,
            sc1 = self.players[0].score,
            sc2 = self.players[1].score,
            serve_f = self.serve_frames,
            rally = self.rallies_started,
            rest = self.resting_on.map(|i| i + 1),
            rest_f = self.rest_frames,
            glow = self.afterglow_frames,
            first = self.first_rally,
            last_p = self.last_point.map(|i| i + 1),
            ball = ?BallTrace::of(&self.ball),
            p1 = ?PlayerTrace::of(&self.players[0]),
            p2 = ?PlayerTrace::of(&self.players[1]),
            "frame"
        );
    }
}

/// The name of a state, for the log.
fn state_name(state: MatchState) -> &'static str {
    match state {
        MatchState::ServeReady => "ServeReady",
        MatchState::Rally => "Rally",
        MatchState::Finished(_) => "Finished",
    }
}

/// The ball's state in the frame dump.
///
/// The fields are read by the derived `Debug` impl - and only by it. The
/// dead-code lint ignores derived `Debug`, so it would report every field as
/// unused.
#[allow(dead_code)]
#[derive(Debug)]
struct BallTrace {
    /// Position in whole pixels.
    x: i32,
    y: i32,
    /// The position of the previous sub-step - the net crossing is detected
    /// with it.
    px: i32,
    py: i32,
    /// Velocity per sub-step (1/32 px).
    vx: i32,
    vy: i32,
    /// Frames since the ball was created.
    age: u64,
    /// The original's `0x9782E0`: the ball is in play.
    play: bool,
    /// The rally is over.
    dead: bool,
}

impl BallTrace {
    fn of(ball: &Ball) -> Self {
        BallTrace {
            x: ball.px(),
            y: ball.py(),
            px: ball.prev_x >> 5,
            py: ball.prev_y >> 5,
            vx: ball.vx,
            vy: ball.vy,
            age: ball.age,
            play: ball.in_play,
            dead: ball.dead,
        }
    }
}

/// One blob's state in the frame dump.
///
/// Like [`BallTrace`], the fields are only read by the derived `Debug` impl.
#[allow(dead_code)]
#[derive(Debug)]
struct PlayerTrace {
    x: i32,
    /// The blob's lower edge, counted from the bottom.
    y: i32,
    /// Jump velocity and acceleration (1/256 px per frame).
    vel: i32,
    accel: i32,
    /// The blob is in the air.
    air: bool,
    /// Ball touches taken during the current rally.
    touch: i32,
    /// The contact flag and its latch of the previous frame - the touch
    /// counter only rises on the **edge** of a contact.
    con: bool,
    conl: bool,
    /// Frames since the contact began.
    conf: u32,
    /// Frames during which this blob cannot hit the ball again.
    cool: u32,
    /// Animation counter and its direction.
    fr: usize,
    anim: i32,
    /// The latest input.
    l: bool,
    r: bool,
    j: bool,
    /// Who steers this blob.
    ctrl: Control,
    /// The difficulty of a computer opponent (0 = easy).
    skill: u32,
    /// The blunder counter of the easy computer opponent.
    blunder: i32,
}

impl PlayerTrace {
    fn of(p: &Player) -> Self {
        PlayerTrace {
            x: p.x,
            y: p.y,
            vel: p.vel,
            accel: p.accel,
            air: p.airborne,
            touch: p.touch,
            con: p.contact,
            conl: p.contact_latch,
            conf: p.contact_frames,
            cool: p.cooldown,
            fr: p.frame,
            anim: p.anim_dir,
            l: p.intent.left,
            r: p.intent.right,
            j: p.intent.jump,
            ctrl: p.control,
            skill: p.skill,
            blunder: p.blunder,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The serve must work: the server jumps into the hanging ball, the
    /// ball is pushed **up** (the hit strength is a constant 8 px per
    /// sub-step) and the ball rises instead of dropping into the sand.
    ///
    /// Regression test for the bug where the hit was scaled by a random
    /// factor, so the ball barely moved and was knocked down again by the
    /// next collision.
    #[test]
    fn serve_hits_the_ball_upwards() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.serve();
        let mut peak = 0;
        for f in 0..30 {
            m.set_intent(
                0,
                Intent {
                    left: false,
                    right: false,
                    jump: f >= 16,
                },
            );
            m.step();
            peak = peak.max(m.ball.py());
        }
        assert!(
            m.ball.in_play,
            "the ball must be in play after the server hit it"
        );
        assert!(
            m.ball.vy > 0,
            "the ball must rise after the hit, vy = {}",
            m.ball.vy
        );
        assert!(
            peak > 320,
            "the ball must be pushed upwards, peak = {peak} px"
        );
        assert_eq!(
            m.players[0].touch, 1,
            "one hit, not a flood of touches"
        );
    }

    /// The original's automatic serve jump (the 500 ms lock, the server
    /// jumping after one second) exists **only** in the computer opponent
    /// block (`FUN_004443E8:373-383`, control modes 2/3/4). A **human**
    /// player has to jump by himself - the ball simply hangs in the air
    /// until somebody hits it.
    #[test]
    fn human_does_not_jump_automatically() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.serve();
        for _ in 0..60 {
            m.set_intent(0, Intent::default());
            m.step();
        }
        assert_eq!(
            m.players[0].y,
            crate::player::GROUND_ROW,
            "the human must not jump by himself"
        );
        assert!(
            !m.ball.in_play,
            "the ball must wait until somebody hits it"
        );
        assert_eq!(m.ball.py(), 300, "the ball must not fall");
    }

    /// The human's own jump key has to work right away - the original does
    /// not lock it during the serve phase.
    #[test]
    fn human_jump_works_during_the_serve() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.serve();
        m.set_intent(
            0,
            Intent {
                left: false,
                right: false,
                jump: true,
            },
        );
        m.step();
        assert!(
            m.players[0].y > crate::player::GROUND_ROW,
            "the human must jump at once, y = {}",
            m.players[0].y
        );
    }

    /// Reproduces the reported behaviour: at the serve, pressing jump + left
    /// sends the ball to the **right** (and jump + right on the other side
    /// sends it to the **left**) - the ball is pushed away from the side
    /// the player walks to.
    ///
    /// The measurement: the hit always happens in frame 18 (the third frame
    /// of the jump). The longer the direction key is held before the hit,
    /// the more the ball is deflected to the **opposite** side.
    #[test]
    fn serve_direction_follows_the_player_movement() {
        for (server, left_key) in [(0usize, true), (1usize, false)] {
            for hold in 0..=3 {
                let mut m = Match::new(0x5EED_1234, Some(server));
                // The port always starts with server 0; the original picks
                // one at random (`Random(2)`, `FUN_004443E8:91-93`).
                m.server = server;
                m.serve();
                let mut first_hit = None;
                for f in 0..40 {
                    let sideways = f >= 16 && f < 16 + hold;
                    m.set_intent(
                        server,
                        Intent {
                            left: sideways && left_key,
                            right: sideways && !left_key,
                            jump: f >= 16,
                        },
                    );
                    m.step();
                    if m.ball.in_play && first_hit.is_none() {
                        first_hit = Some((
                            f,
                            m.ball.vx,
                            m.ball.vy,
                            m.players[server].x,
                            m.ball.px(),
                            m.ball.py(),
                        ));
                    }
                }
                match first_hit {
                    Some((f, vx, vy, px, bx, by)) => println!(
                        "server={server} seite={} Taste={hold} Frames -> Treffer in Frame {f}: vx={vx} vy={vy} (Spieler_x={px}, Ball={bx}/{by})",
                        if left_key { "links" } else { "rechts" },
                    ),
                    None => println!(
                        "server={server} seite={} Taste={hold} Frames -> kein Treffer",
                        if left_key { "links" } else { "rechts" },
                    ),
                }
            }
        }
    }

    /// Shows the ball's path after the serve hit: the original pushes the
    /// ball **out of the blob** in one tenth steps (FUN_00443BF0), so the
    /// hit direction is decided once and the ball flies straight. The port
    /// moves the ball by a single `v`, so it stays inside the collision
    /// circle and gets pushed again in the next frame - with a new angle.
    #[test]
    fn serve_ball_path_after_the_hit() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.server = 0;
        m.serve();
        for f in 0..30 {
            let sideways = f == 16;
            m.set_intent(
                0,
                Intent {
                    left: sideways,
                    right: false,
                    jump: f >= 16,
                },
            );
            m.step();
            if f >= 15 {
                println!(
                    "f={f:2} spieler_x={:3} ball=({:3},{:3}) v=({:5},{:5})",
                    m.players[0].x,
                    m.ball.px(),
                    m.ball.py(),
                    m.ball.vx,
                    m.ball.vy
                );
            }
        }
    }

    /// A rally that ends while a blob is still in the air must not freeze
    /// the game.
    ///
    /// The original's frame loop keeps running after the rally is over: the
    /// blobs walk on and land, the ball keeps moving
    /// (`BV_Phys_H_443e8.c:394, 403-473, 477`). Only the computer block and
    /// the two collision calls are switched off (`:246, :391, :474`).
    ///
    /// The port returned from the `ServeReady` state right away, so a blob
    /// that ended the rally in mid-air never landed. The "both blobs are on
    /// the floor" condition of `:751-757` never came true, the next serve
    /// never started and the game froze with the ball in the air - the bug
    /// that was reported with two computer opponents (the right blob stuck
    /// at y = 262).
    #[test]
    fn the_game_does_not_freeze_after_a_rally_ended_in_the_air() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.serve();
        // The state of the reported crash: the right blob is in the air
        // (y = 262) when the rally ends.
        m.players[1].airborne = true;
        m.players[1].y = 262;
        m.players[1].vel = 128;
        m.players[1].accel = -1200;
        m.state = MatchState::ServeReady;
        m.afterglow_frames = 0;

        let mut frames = 0;
        while m.state == MatchState::ServeReady && frames < 300 {
            m.set_intent(0, Intent::default());
            m.set_intent(1, Intent::default());
            m.step();
            frames += 1;
        }
        assert_eq!(
            m.state,
            MatchState::Rally,
            "the next serve must start even if a blob was still in the air ({frames} frames)"
        );
        assert_eq!(
            m.players[1].y,
            crate::player::GROUND_ROW,
            "the blob must land again during the afterglow"
        );
    }

    /// The scoreboard's serve indicator and the match point flag.
    ///
    /// The original draws an exclamation mark next to the score of the
    /// player who has the service (`FUN_00443694`, the third cell at
    /// `+0x1A5EC0`), but **not** during the first rally
    /// (`BV_Phys_H_443e8.c:616-627`) - in that rally the service is not
    /// settled yet. The match point flag (`BV_Phys_H_443e8.c:39-46`)
    /// needs more than 13 points and a lead.
    #[test]
    fn serve_indicator_and_match_point() {
        let mut m = Match::new(0x5EED_1234, Some(0));
        m.serve();
        assert_eq!(
            m.serve_indicator(),
            None,
            "no serve indicator during the first rally"
        );
        m.rallies_started = 2;
        m.server = 0;
        assert_eq!(m.serve_indicator(), Some(0), "the server is marked");
        m.server = 1;
        assert_eq!(m.serve_indicator(), Some(1), "the mark follows the serve");

        m.server = 0;
        m.players[0].score = 13;
        m.players[1].score = 0;
        assert!(!m.is_match_point(), "13 points are not a match point");
        m.players[0].score = 14;
        assert!(m.is_match_point(), "14 points and a lead is a match point");
        m.players[1].score = 15;
        assert!(!m.is_match_point(), "without the lead it is no match point");
    }

    /// The host's state has to survive the way to the guest unchanged.
    #[test]
    fn the_state_travels_from_the_host_to_the_guest() {
        let mut host = Match::new(0x5EED_1234, Some(0));
        host.serve();
        host.players[0].x = 123;
        host.players[0].y = 456;
        host.players[1].x = 789;
        host.players[1].y = 1011;
        host.players[0].frame = 3;
        host.players[1].frame = 7;
        host.ball.x = 42;
        host.ball.y = 4242;

        let state = host.net_state();
        let mut guest = Match::new(0x5EED_1234, Some(1));
        guest.apply_net_state(&state);

        assert_eq!(guest.ball.x, 42);
        assert_eq!(guest.ball.y, 4242);
        assert_eq!(guest.players[0].x, 123);
        assert_eq!(guest.players[0].y, 456);
        assert_eq!(guest.players[1].x, 789);
        assert_eq!(guest.players[1].y, 1011);
        assert_eq!(guest.players[0].frame, 3);
        assert_eq!(guest.players[1].frame, 7);
    }

    /// The guest takes the score, the server and the end of the match from
    /// the host's score packet.
    #[test]
    fn the_score_travels_from_the_host_to_the_guest() {
        let mut guest = Match::new(0x5EED_1234, Some(1));
        guest.apply_net_score(7, 3, 1, false);
        assert_eq!(guest.players[0].score, 7);
        assert_eq!(guest.players[1].score, 3);
        assert_eq!(guest.server, 1);
        assert_eq!(
            guest.serve_indicator(),
            Some(1),
            "the announced server carries the indicator"
        );

        // 0xFF means "no indicator" - that is the case during the first rally.
        guest.apply_net_score(0, 0, 0xFF, false);
        assert_eq!(guest.serve_indicator(), None);

        // The end of the match.
        guest.apply_net_score(15, 13, 0, true);
        assert!(
            matches!(guest.state, MatchState::Finished(0)),
            "the guest sees the match end: {:?}",
            guest.state
        );
    }
}
