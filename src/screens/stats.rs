//! The statistics screen.
//!
//! `FUN_0044A570` shows the statistics board (692x590) with a table on top of
//! it. The table can be switched between five variants and the columns can be
//! sorted by clicking their header.
//!
//! The five tables come from `@stats`:
//!
//! ```text
//!   matches against humans
//!   matches against Adam (easy)
//!   matches against Bert (avg.)
//!   matches against Carl (hard)
//!   all matches
//! ```

use crate::canvas::Canvas;
use crate::fonts::TextStyle;
use crate::layout::{self, LINE_HEIGHT};
use crate::log::info;
use super::{draw_cursor, draw_row, hit, Action, Ctx, Input, Screen};

/// State of the statistics screen.
#[derive(Clone, Debug, Default)]
pub struct StatsState {
    /// Which of the five tables is shown.
    pub table: usize,
    /// Which column the table is sorted by.
    pub sort: usize,
    /// Page of the table.
    pub page: i32,
}

/// The X of the four columns (`DAT_0044DC0C` .. `DAT_0044DC18`).
const COLUMN_X: [i32; 4] = [110, 330, 430, 530];

/// The number of statistics tables (`@stats` 7..11).
const TABLE_COUNT: usize = 5;

/// The canvas row the table starts in (`0x197`).
const ROW_Y: i32 = 0x197;

/// The top-down Y of the column header row.
const HEADER_Y: i32 = 145;

/// Draw the screen and act on the input.
pub fn render(
    state: &mut StatsState,
    canvas: &mut Canvas,
    ctx: &mut Ctx,
    input: &Input,
) -> Action {
    canvas.copy_from(&ctx.assets.background_night);

    // --- the statistics board ----------------------------------------------
    if let Some(set) = ctx.assets.set(692, 590) {
        let x = (800 - 692) / 2;
        let y = (600 - 590) / 2;
        canvas.blit(set.frame(0), 692, 590, x, y);
    }

    // --- the table ---------------------------------------------------------
    let mut rows: Vec<&crate::settings::StatsRecord> = ctx.settings.stats.iter().collect();
    rows.sort_by_key(|r| match state.sort {
        1 => r.games,
        2 => r.won,
        3 => r.score,
        _ => 0,
    });

    // the name of the current table (`@stats` 7..11)
    ctx.font.draw(
        canvas,
        &ctx.text("stats", 7 + state.table),
        110,
        40,
        TextStyle::Normal,
    );

    // column headers, clickable for sorting (`@stats` 0..3)
    let mut header_rects = [(0, 0, 0, 0); 4];
    for (i, col) in (0..4).map(|i| ctx.text("stats", i)).enumerate() {
        let x = COLUMN_X[i];
        let style = if state.sort == i {
            TextStyle::Highlight
        } else {
            TextStyle::Normal
        };
        ctx.font.draw(canvas, &col, x, HEADER_Y, style);
        header_rects[i] = (x, HEADER_Y, ctx.font.width(&col), LINE_HEIGHT);
    }

    // The two remaining columns of the statistics table: the port and the IP
    // address of a network game. The original draws their headers - the "²"
    // and the "³" - in a row **below** the table (`DAT_0044DC0C`: x = 110 and
    // 660, `DAT_0044DC2C`: y = 100, both in its bottom-up coordinates).
    for (i, x) in [110, 660].iter().enumerate() {
        let header = ctx.text("stats", 4 + i);
        ctx.font.draw(
            canvas,
            &header,
            *x,
            layout::to_top_down(100, LINE_HEIGHT),
            TextStyle::Normal,
        );
    }

    // the rows: the original starts at canvas row 407 and steps by 24
    let per_page = 9;
    let start = (state.page.max(0) as usize) * per_page;
    for (i, rec) in rows.iter().skip(start).take(per_page).enumerate() {
        let y = layout::to_top_down(ROW_Y - i as i32 * LINE_HEIGHT, LINE_HEIGHT);
        let cells = [
            rec.name.clone(),
            rec.games.to_string(),
            rec.won.to_string(),
            rec.score.to_string(),
        ];
        for (c, value) in cells.iter().enumerate() {
            ctx.font.draw(canvas, value, COLUMN_X[c], y, TextStyle::Normal);
        }
    }

    // the table switch and the back button
    let switch = draw_row(canvas, ctx.font, "NEXT TABLE", 110, 545, false, false);
    let back = draw_row(canvas, ctx.font, &ctx.text("stats", 6), 560, 545, false, false);

    draw_cursor(canvas, ctx.assets, input.mouse);

    // --- input -------------------------------------------------------------
    if input.clicked {
        for (i, rect) in header_rects.iter().enumerate() {
            if hit(*rect, input.mouse) {
                state.sort = i;
                info!(column = i, "statistics sorted by column");
                return Action::None;
            }
        }
        if hit(switch, input.mouse) {
            state.table = (state.table + 1) % TABLE_COUNT;
            info!(table = state.table, "statistics table switched");
            return Action::None;
        }
        if hit(back, input.mouse) {
            return Action::Switch(Screen::Menu(
                crate::screens::menu::MenuState::default(),
            ));
        }
    }
    if input.pressed(egui::Key::ArrowLeft) {
        state.page = (state.page - 1).max(0);
    }
    if input.pressed(egui::Key::ArrowRight) {
        state.page += 1;
    }
    if input.pressed(egui::Key::Tab) {
        state.table = (state.table + 1) % TABLE_COUNT;
    }
    if input.pressed(egui::Key::Enter) || input.pressed(egui::Key::Space) {
        state.table = (state.table + 1) % TABLE_COUNT;
    }
    if input.pressed(egui::Key::Escape) {
        return Action::Switch(Screen::Menu(
            crate::screens::menu::MenuState::default(),
        ));
    }
    Action::None
}
