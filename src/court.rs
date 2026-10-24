//! Court geometry and fixed-point scale, derived from the original's
//! `volley.exe` (Borland Delphi) game object layout.
//!
//! The original stores world coordinates as **signed 32-bit fixed point with
//! 32 sub-steps per pixel** (screen coordinate == `value >> 5`). Everything in
//! `ball` and `player` uses that same unit so the recovered constants apply
//! verbatim, without rounding drift.

/// Number of sub-pixel units per screen pixel (1/32 px).
pub const SUBPX: i32 = 32;

/// Convert a fixed-point value to whole screen pixels (floor division).
///
/// The original converts with an arithmetic right shift (`>> 5`), which
/// rounds towards minus infinity for negative values. `v >> 5` reproduces
/// that exactly (Rust's `>>` on `i32` is arithmetic).
#[inline]
pub const fn to_px(v: i32) -> i32 {
    v >> 5
}

/// Convert whole screen pixels into the fixed-point representation.
#[inline]
pub const fn from_px(px: i32) -> i32 {
    px * SUBPX
}

/// Logical resolution of the play field.
pub const VIEW_W: i32 = 800;
pub const VIEW_H: i32 = 600;

/// Horizontal centre line. The original keeps the net here, and the two
/// halves are exactly `VIEW_W / 2` wide.
pub const CENTRE_X: i32 = VIEW_W / 2; // 400

// --- Ball limits (all values in 1/32 px, taken verbatim from the binary) ---

/// Ground / bounce plane. The ball is clamped up to here when it falls
/// through; anything below is a rally point against the owning half.
pub const GROUND_Y: i32 = 0x1080; // 4224 -> 132 px

/// Left wall.
pub const WALL_L: i32 = 0x0400; // 1024 -> 32 px

/// Right wall.
pub const WALL_R: i32 = 0x6000; // 24576 -> 768 px

/// Net collision window, left-facing half.
/// Triggered when `0x17B < (x_px - 0x20) < 0x19A`, ball travelling left.
pub const NET_L_LO: i32 = 0x17B;
pub const NET_L_HI: i32 = 0x19A;

/// Net collision window, right-facing half.
/// Triggered when `0x186 < (x_px + 0x20) < 0x1A5`, ball travelling right.
pub const NET_R_LO: i32 = 0x186;
pub const NET_R_HI: i32 = 0x1A5;

/// X the ball is snapped to after bouncing off the net, in 1/32 px.
pub const NET_SNAP_RIGHT: i32 = 0x3760; // 14176 -> 443 px
pub const NET_SNAP_LEFT: i32 = 0x2CA0; // 11424 -> 357 px

/// Top of the net: the ball passes over when `(y_px - 0x20) >= 0x143`.
pub const NET_TOP_PX: i32 = 0x143; // 323

/// X offset used when testing the net window, in whole pixels.
pub const NET_PROBE_PX: i32 = 0x20; // 32

/// `FUN_00444100`: the net circle. The ball passes the net plane at 384 px
/// (0x2DFF); below 320 px of height it is snapped back to the side it came
/// from.
pub const NET_PLANE: i32 = 0x2DFF; // 12287 -> 384 px
pub const NET_CIRCLE_LO: i32 = 0x2D1F; // 11551 -> 361 px
pub const NET_CIRCLE_HI: i32 = 0x36E1; // 14017 -> 438 px
pub const NET_CIRCLE_SNAP_L: i32 = 0x2D00; // 11520 -> 360 px
pub const NET_CIRCLE_SNAP_R: i32 = 0x3700; // 14080 -> 440 px
pub const NET_CIRCLE_OLD_X: i32 = 0x3201; // 12801 -> 400 px
pub const NET_CIRCLE_Y: i32 = 0x2801; // 10241 -> 320 px
pub const NET_CIRCLE_OLD_Y: i32 = 0x28C0; // 10432 -> 326 px

// --- Rally rules ----------------------------------------------------------

/// Maximum number of ball touches per side before the rally is lost.
pub const TOUCH_LIMIT: i32 = 3; // the 4th touch is the fault

/// Points needed to win.
///
/// The original uses 15 (`FUN_00445890.c:58`).
pub const WIN_SCORE: i32 = 15;

/// Required lead to close out the match.
pub const WIN_LEAD: i32 = 2;

// --- Player spawn positions ----------------------------------------------

/// Player 0 starts at x = 200 px, player 1 at x = 600 px, both at y = 100
/// (1/32 units, i.e. ground level for the jumper).
pub const PLAYER_SPAWN_X: [i32; 2] = [200, 600];
pub const PLAYER_SPAWN_Y: i32 = 100;

// --- Timing ---------------------------------------------------------------

/// The original's frame rate.
///
/// The rally handler busy-waits at its end until
/// `GetTickCount() >= last_frame + 0x978234` (`BV_Phys_H_443e8.c:722`), and
/// `0x978234` is initialised to `0x21` = 33 ms. **The match therefore runs at
/// about 30 fps** - with a busy-wait, no `Sleep`, so it burns the CPU just
/// like the original did.
///
/// The menu loop limits itself differently: `0x978234 >> 2` = 8 ms plus a
/// `Sleep(5)`, that is roughly 125 fps.
///
/// The physics is bound to the frame rate (the ball integrates two sub-steps
/// per call), so the port has to run at the original's rate for the
/// original's constants to produce the original's speeds:
/// 12 px/frame walking, a 26 frame jump, a 23 frame ball fall.
pub const PHYSICS_HZ: u64 = 30;

/// Sub-steps the original integrates per rendered frame.
pub const SUBSTEPS: i32 = 2;

/// Downward acceleration per sub-step, in 1/32 px.
///
/// The original folds gravity into the term `vy -= 3 - drag` inside
/// `FUN_004441D0` and gates **the whole term** behind the flag `0x9782E0`
/// ("the ball is in play"). The port does the same: `Ball::step_once` only
/// applies the gravity while `in_play` is set - during the serve the flag
/// is clear and the ball hangs in the air, exactly like in the original
/// (`BV_Phys_G_441d0.c:26-28`).
pub const GRAVITY: i32 = 3;

/// Sanity ceiling for the ball's downward position, well past the visible
/// court. A ball below this is treated as a physics fault.
pub const BALL_Y_CEILING: i32 = from_px(VIEW_H) * 8;

/// Vertical pixel band in which a standing player can reach the ball.
pub const REACH_LO_PX: i32 = 300;
pub const REACH_HI_PX: i32 = 500;

/// Ball is considered "on the ground" below this upward speed.
pub const GROUNDED_SPEED_LIMIT: i32 = 0x0B; // 11

/// Dump every recovered constant together with the address it came from.
///
/// This is the single most useful trace when the port drifts from the
/// original: it records *why* each number is what it is.
pub fn log_recovered_constants() {
    use tracing::info;

    info!("--- recovered constants (source: blobb/volley.exe) ---");

    macro_rules! row {
        ($name:expr, $val:expr, $px:expr, $src:expr) => {
            info!(
                name = $name,
                value = $val as i32,
                pixels = $px,
                source = $src,
                "constant"
            )
        };
    }

    row!("SUBPX", SUBPX, 1, "scale (>> 5 everywhere)");
    row!("VIEW_W", VIEW_W, VIEW_W, "0x441f53c / DFM");
    row!("VIEW_H", VIEW_H, VIEW_H, "0x441f55c / DFM");
    row!("CENTRE_X", CENTRE_X, CENTRE_X, "net plane");
    row!("GROUND_Y", GROUND_Y, GROUND_Y >> 5, "FUN_004441D0 bounce clamp");
    row!("WALL_L", WALL_L, WALL_L >> 5, "FUN_004441D0 wall");
    row!("WALL_R", WALL_R, WALL_R >> 5, "FUN_004441D0 wall");
    row!("NET_L_LO", NET_L_LO, NET_L_LO, "FUN_004441D0 net probe");
    row!("NET_L_HI", NET_L_HI, NET_L_HI, "FUN_004441D0 net probe");
    row!("NET_R_LO", NET_R_LO, NET_R_LO, "FUN_004441D0 net probe");
    row!("NET_R_HI", NET_R_HI, NET_R_HI, "FUN_004441D0 net probe");
    row!("NET_SNAP_RIGHT", NET_SNAP_RIGHT, NET_SNAP_RIGHT >> 5, "FUN_004441D0 net");
    row!("NET_SNAP_LEFT", NET_SNAP_LEFT, NET_SNAP_LEFT >> 5, "FUN_004441D0 net");
    row!("NET_TOP_PX", NET_TOP_PX, NET_TOP_PX, "FUN_004441D0 net height");
    row!("NET_PROBE_PX", NET_PROBE_PX, NET_PROBE_PX, "FUN_004441D0 net offset");
    row!("TOUCH_LIMIT", TOUCH_LIMIT, TOUCH_LIMIT, "4th touch = fault");
    row!("WIN_SCORE", WIN_SCORE, WIN_SCORE, "FUN_00445890 loop guard");
    row!("WIN_LEAD", WIN_LEAD, WIN_LEAD, "FUN_00445890 loop guard");
    row!("PLAYER_SPAWN_X0", PLAYER_SPAWN_X[0], PLAYER_SPAWN_X[0], "FUN_00445890");
    row!("PLAYER_SPAWN_X1", PLAYER_SPAWN_X[1], PLAYER_SPAWN_X[1], "FUN_00445890");
    row!("PLAYER_SPAWN_Y", PLAYER_SPAWN_Y, PLAYER_SPAWN_Y >> 5, "FUN_00445890");
    row!("REACH_LO_PX", REACH_LO_PX, REACH_LO_PX, "FUN_00443E8 hit band");
    row!("REACH_HI_PX", REACH_HI_PX, REACH_HI_PX, "FUN_00443E8 hit band");
    row!("SUBSTEPS", SUBSTEPS, SUBSTEPS, "FUN_004441D0 do/while");
    row!("PHYSICS_HZ", PHYSICS_HZ as i32, PHYSICS_HZ as i32, "frame limiter");

    info!(
        ball_spawn_x_left = PLAYER_SPAWN_X[0] + 200,
        ball_spawn_x_right = PLAYER_SPAWN_X[1] - 200,
        ball_spawn_y = 0x2580 >> 5,
        "ball serve position (FUN_00443E8)"
    );

    info!("--- end of recovered constants ---");
}