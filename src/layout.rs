//! Screen layout constants, recovered from `volley.exe`.
//!
//! Every value in here was read out of the binary's DATA segment
//! (`RVA 0x4D000`, file offset `0x4C000`) or derived from the decompiled
//! drawing routines. None of them is guessed.
//!
//! # The vertical axis
//!
//! The original draws into a **bottom-up** DirectDraw surface, so all Y
//! values in the binary are counted from the *bottom* of the screen and
//! describe the **lower edge** of a 24 px tall text row. [`to_top_down`]
//! converts them to the top-down coordinates a normal renderer uses.

/// Horizontal advance between two glyphs (`DAT_0044DB4C`).
pub const GLYPH_ADVANCE: i32 = 20;

/// Size of one font cell (`24x24`).
pub const GLYPH_SIZE: i32 = 24;

/// Height of one text row (`DAT_0044DB50`).
pub const LINE_HEIGHT: i32 = 24;

/// The main menu text is centred on this X (`FUN_00447E0C`).
pub const MENU_TEXT_CENTER_X: i32 = 500;

/// Lower edge of the six main menu rows (`DAT_0044DB64`), bottom-up.
pub const MENU_ITEM_Y: [i32; 6] = [310, 265, 220, 175, 130, 85];

/// Left edge of the settings screen (`0xDC` in `FUN_004486FC`).
pub const SETTINGS_X: i32 = 220;

/// Lower edge of the seven settings rows (`DAT_0044DB88`), bottom-up.
pub const SETTINGS_ITEM_Y: [i32; 7] = [480, 440, 320, 280, 180, 140, 40];

/// Label indices inside `@settings` per row (`DAT_0044DBC0`).
pub const SETTINGS_LABEL_IDX: [usize; 7] = [2, 3, 2, 3, 4, 5, 6];

/// Left edge of each settings row (`DAT_0044DBA4`, read from the binary):
/// the first six rows sit at `0xDC` (220), the seventh one - the "OK" row -
/// at `0x172` (370).
pub const SETTINGS_ITEM_X: [i32; 7] = [220, 220, 220, 220, 220, 220, 370];

/// Lower edge of the two colour plates (`DAT_0044DB80` / `DAT_0044DB84`).
pub const SWATCH_Y: [i32; 2] = [520, 360];

/// Left edge of the colour plates (`0xD9` in `FUN_0044812C`).
pub const SWATCH_X: i32 = 217;

/// Edge length of the colour plates.
pub const SWATCH_SIZE: i32 = 30;

/// Left edge of the two LED scoreboards (`DAT_0044DB44` / `DAT_0044DB48`).
pub const SCOREBOARD_X: [i32; 2] = [100, 636];

/// Lower edge of the multiplayer option rows (`DAT_0044DC6C`).
pub const MP_ITEM_Y: [i32; 3] = [230, 190, 150];

/// Left edge of the multiplayer option rows (`DAT_0044DC78`).
pub const MP_ITEM_X: [i32; 3] = [250, 250, 250];

/// Lower edge of the host settings rows (`DAT_0044DC84`).
pub const HOST_ITEM_Y: [i32; 4] = [250, 200, 150, 100];

/// Left edge of the host settings rows (`DAT_0044DC9C`).
pub const HOST_ITEM_X: [i32; 4] = [200, 200, 200, 200];

/// Lower edge of the client settings rows (`DAT_0044DCB4`).
pub const CLIENT_ITEM_Y: [i32; 4] = [250, 200, 150, 100];

/// Left edge of the client settings rows (`DAT_0044DCC4`).
pub const CLIENT_ITEM_X: [i32; 4] = [110, 110, 110, 110];

/// Screen titles are centred here, on this row.
pub const TITLE_CENTER_X: i32 = 400;
pub const TITLE_Y: i32 = 300;

/// Where the original puts the logo in the menu.
pub const LOGO_POS: (i32, i32) = (24, 8);

/// Convert a bottom-up canvas Y (the *lower* edge of a `height` tall object)
/// into a top-down screen Y (the *upper* edge).
pub fn to_top_down(canvas_y: i32, height: i32) -> i32 {
    600 - canvas_y - height
}

/// The eight team colours, in the original's order (`FUN_0044A784`).
///
/// The values are `0x00RRGGBB`, exactly as the original stores them.
pub const TEAM_COLORS: [u32; 8] = [
    0xFF0000, // 0 red
    0xFF7F00, // 1 orange
    0xFFFF00, // 2 yellow
    0x00FF00, // 3 green
    0x00ECFF, // 4 cyan
    0x0000FF, // 5 blue
    0xC000FF, // 6 magenta
    0xFF00D8, // 7 pink
];

/// The additive lift the original's tinting adds to every channel
/// (`FUN_0044A6F0`).
///
/// The original computes it from the **source pixel's blue channel**:
/// `iVar1 = (param_2 & 0xff) * 5 + -0x4b2` clamped at zero. `param_2` is the
/// sprite pixel, `param_1` the team colour - the call site
/// (`FUN_0044A784`) passes the colour first and the pixel second.
///
/// The lift is therefore a property of the pixel being tinted, not of the
/// team colour: a white pixel always gets the full lift, a dark one none.
fn tint_lift(src_blue: u8) -> u32 {
    (5 * src_blue as i32 - 0x4B2).max(0) as u32
}

/// The original's text tinting function (`FUN_0044A6F0`).
///
/// Multiplies every channel with the team colour and adds a small lift that
/// only becomes visible for very bright blue components.
pub fn tint_channel(color: u32, src: u8) -> u8 {
    let cb = (color & 0xFF) as u32;
    let add = tint_lift(src);
    let product = (cb * src as u32) >> 8;
    (product + add).min(255) as u8
}

/// Apply [`tint_channel`] to a whole RGB triple.
pub fn tint_rgb(color: u32, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let cb = (color & 0xFF) as u32;
    let cg = ((color >> 8) & 0xFF) as u32;
    let cr = ((color >> 16) & 0xFF) as u32;
    let add = tint_lift(b);
    let f = |cc: u32, sc: u8| (((cc * sc as u32) >> 8) + add).min(255) as u8;
    (f(cr, r), f(cg, g), f(cb, b))
}

/// The original's greyed-out text variant (`FUN_0044724C`), used for menu
/// entries that cannot be used.
pub fn grey_rgb(r: u8, g: u8, b: u8, highlight: bool) -> (u8, u8, u8) {
    let lum = (b as u32 * 3 + g as u32 * 6 + r as u32) / 10;
    let v = if highlight {
        (0xBFi32 - (lum as i32 >> 1)).min(255)
    } else {
        (0x7Fi32 - (lum as i32 >> 1)).min(255)
    }
    .max(0) as u8;
    (v, v, v)
}

/// The original's "selected entry" brightening (`FUN_00447398`).
pub fn highlight_channel(c: u8) -> u8 {
    (c as u32 * 2 + 0x1E).min(255) as u8
}
