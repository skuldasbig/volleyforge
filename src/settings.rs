//! Player settings, stored in `volleyforge.vf` next to the executable.
//!
//! The original keeps these values in a binary `settings.dat` next to the
//! game. That format is undocumented and only partly understood, so VolleyForge
//! uses a small, readable text file instead - the game does not care, and a
//! human can still fix a broken configuration by hand.
//!
//! The values themselves are the original's:
//!
//! | setting                | original location        |
//! |------------------------|--------------------------|
//! | six key codes          | `+0x97820C` (24 byte block) |
//! | two control modes       | `+0x978224` (8 byte block)  |
//! | two player names        | `+0x978238` / `+0x97828C`   |
//! | sound on/off           | `+0x97822C`                 |
//! | two team colours       | `+0x97828B` / `+0x9782DF`   |

use std::collections::BTreeMap;
use std::path::Path;

use egui::Key;

use crate::log::{info, warn};

/// How a player is controlled.
///
/// The values are the original's: `0x978224` and `0x978228` cycle through
/// `0..5`, and the original refuses to give the mouse to both players at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    Keyboard,
    Mouse,
    CpuEasy,
    CpuAvg,
    CpuHard,
}

impl Control {
    /// The original's index of this mode.
    pub fn index(self) -> usize {
        match self {
            Control::Keyboard => 0,
            Control::Mouse => 1,
            Control::CpuEasy => 2,
            Control::CpuAvg => 3,
            Control::CpuHard => 4,
        }
    }

    /// Build a mode from the original's index.
    pub fn from_index(i: usize) -> Control {
        match i {
            1 => Control::Mouse,
            2 => Control::CpuEasy,
            3 => Control::CpuAvg,
            4 => Control::CpuHard,
            _ => Control::Keyboard,
        }
    }

    /// The label the original shows in the settings screen (`@settings` 7..11).
    pub fn label(self) -> &'static str {
        match self {
            Control::Keyboard => "Keyboard",
            Control::Mouse => "Mouse",
            Control::CpuEasy => "Comp.(easy)",
            Control::CpuAvg => "Comp.(avg.)",
            Control::CpuHard => "Comp.(hard)",
        }
    }

    /// True for the three computer opponents.
    pub fn is_cpu(self) -> bool {
        matches!(self, Control::CpuEasy | Control::CpuAvg | Control::CpuHard)
    }

    /// The name the original gives a computer opponent (`@names`).
    pub fn cpu_name(self) -> &'static str {
        match self {
            Control::CpuEasy => "Adam",
            Control::CpuAvg => "Bert",
            Control::CpuHard => "Carl",
            _ => "",
        }
    }
}

/// The six configurable keys: two players, three actions each.
#[derive(Clone, Copy, Debug)]
pub struct KeyBindings {
    /// player 0: left, right, jump
    pub p0: [Key; 3],
    /// player 1: left, right, jump
    pub p1: [Key; 3],
}

impl Default for KeyBindings {
    fn default() -> Self {
        // The slots are left, right, jump (`BV_Ref_KeyDef.c:42,52,65` maps them
        // to `@keydef` " left" / " right" / " jump"). The original's own
        // `settings.dat` starts with `58 43 41` - so for player one
        // **X walks left, C walks right and A jumps**. The help text only
        // lists the three keys ("A, X, C"), it does not give the order.
        KeyBindings {
            p0: [Key::X, Key::C, Key::A],
            p1: [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp],
        }
    }
}

impl KeyBindings {
    pub fn get(&self, player: usize, slot: usize) -> Key {
        match player {
            0 => self.p0[slot],
            _ => self.p1[slot],
        }
    }

    pub fn set(&mut self, player: usize, slot: usize, key: Key) {
        match player {
            0 => self.p0[slot] = key,
            _ => self.p1[slot] = key,
        }
    }

    /// True when the key is already used by one of the slots before `step`.
    ///
    /// The original compares a new key only with the slots it has already
    /// asked for (`0x447a48..0x447a6b`): the slot that is filled right now
    /// is **not** part of the comparison, so a key may stay in its own slot.
    pub fn used_before(&self, key: Key, step: usize) -> bool {
        (0..step).any(|idx| self.get(idx / 3, idx % 3) == key)
    }
}

/// One row of the statistics table.
#[derive(Clone, Debug, Default)]
pub struct StatsRecord {
    pub name: String,
    pub games: u32,
    pub won: u32,
    pub score: u32,
}

/// Where the settings file lives: next to the executable.
pub fn settings_path() -> std::path::PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("volleyforge.vf")))
            .unwrap_or_else(|| std::path::PathBuf::from("volleyforge.vf"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::path::PathBuf::from("volleyforge.vf")
    }
}

/// The three localisation tables of `text.pak`, in the original's order.
///
/// The original stores the index in `0x978208` and cycles it with
/// `(*(int *)(param_1 + 0x978208) + 1) % 3` (`BV_Menu_00447E0C.c:131`).
/// The value is also persisted in `settings.dat` (4 bytes at `+0x978208`).
pub const LANGUAGE_COUNT: usize = 3;

/// The original's default language: German (index 1).
///
/// The original's own menu screenshot shows the German table ("Spiel starten",
/// "English Please"), so German is what the game starts with.
pub const DEFAULT_LANGUAGE: usize = 1;

/// Everything the settings screen can change.
#[derive(Clone, Debug)]
pub struct Settings {
    pub keys: KeyBindings,
    pub control: [Control; 2],
    pub names: [String; 2],
    pub sound_on: bool,
    pub colors: [u8; 2],
    pub stats: Vec<StatsRecord>,
    /// Index of the active localisation table (0 = French, 1 = German, 2 = English).
    pub language: usize,
    /// The game runs in fullscreen. F11 switches it in every screen.
    ///
    /// The original is always fullscreen; VolleyForge starts in a window and
    /// lets the player switch.
    pub fullscreen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            keys: KeyBindings::default(),
            // The original's shipped settings.dat has player 1 on the keyboard and
   // player 2 on the mouse (BV_Menu_00447E0C.c:35-40, the mode byte at
   // 0x1C is 0 for player 1 and 1 for player 2).
   control: [Control::Keyboard, Control::Mouse],
            names: ["Player 1".to_string(), "Player 2".to_string()],
            sound_on: true,
            colors: [0, 3], // red vs green, exactly like the original
            stats: Vec::new(),
            language: DEFAULT_LANGUAGE,
            fullscreen: false,
        }
    }
}

impl Settings {
    /// The name shown for a player: their own name, or the computer's name.
    pub fn display_name(&self, player: usize) -> String {
        if self.control[player].is_cpu() {
            self.control[player].cpu_name().to_string()
        } else {
            self.names[player].clone()
        }
    }

    /// Switch to the next localisation table, exactly like the original's
    /// `(*(int *)(param_1 + 0x978208) + 1) % 3`.
    pub fn cycle_language(&mut self) {
        self.language = (self.language + 1) % LANGUAGE_COUNT;
        info!(
            language = self.language,
            "language switched (the original cycles FR -> DE -> EN)"
        );
    }

    /// Cycle a player's control mode, applying the original's rule that the
    /// mouse cannot be used by both players at the same time.
    pub fn cycle_control(&mut self, player: usize) {
        let mut next = (self.control[player].index() + 1) % 5;
        if next == Control::Mouse.index() && self.control[1 - player] == Control::Mouse {
            next = Control::CpuEasy.index();
        }
        self.control[player] = Control::from_index(next);
        info!(player, mode = %self.control[player].label(), "control mode changed");
    }

    /// Cycle a player's team colour through the eight original colours.
    pub fn cycle_color(&mut self, player: usize) {
        self.colors[player] = (self.colors[player] + 1) & 7;
        info!(player, color = self.colors[player], "team colour changed");
    }

    // --- persistence -----------------------------------------------------

    /// Read the settings file, falling back to the defaults.
    pub fn load(path: &Path) -> Settings {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
                if let Ok(Some(content)) = storage.get_item("volleyforge_settings") {
                    return Self::parse(&content);
                }
            }
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            info!(path = %path.display(), "no settings file yet, using the defaults");
            return Settings::default();
        };
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        // The statistics are a list, not a single value - they get their own
        // lines ("stat = name|games|won|points").
        let mut stat_lines: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                if k == "stat" {
                    stat_lines.push(v.trim().to_string());
                    continue;
                }
                map.insert(k.to_string(), v.trim().to_string());
            }
        }
        for i in 0..6 {
            if let Some(v) = map.get(&format!("key{i}")) {
                if let Some(k) = key_from_name(v) {
                    settings.keys.set(i / 3, i % 3, k);
                }
            }
        }
        for i in 0..2 {
            if let Some(v) = map.get(&format!("control{i}")) {
                if let Ok(n) = v.parse::<usize>() {
                    settings.control[i] = Control::from_index(n);
                }
            }
        }
        for i in 0..2 {
            if let Some(v) = map.get(&format!("name{i}")) {
                settings.names[i] = v.to_string();
            }
        }
        for i in 0..2 {
            if let Some(v) = map.get(&format!("color{i}")) {
                if let Ok(n) = v.parse::<u8>() {
                    settings.colors[i] = n & 7;
                }
            }
        }
        if let Some(v) = map.get("sound") {
            settings.sound_on = v == "1";
        }
        if let Some(v) = map.get("language") {
            if let Ok(n) = v.parse::<usize>() {
                settings.language = n % LANGUAGE_COUNT;
            }
        }
        if let Some(v) = map.get("fullscreen") {
            settings.fullscreen = v == "1";
        }
        for line in stat_lines {
            let mut parts = line.split('|');
            let (Some(name), Some(games), Some(won), Some(score)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let (Ok(games), Ok(won), Ok(score)) = (
                games.parse::<u32>(),
                won.parse::<u32>(),
                score.parse::<u32>(),
            ) else {
                continue;
            };
            settings.stats.push(StatsRecord {
                name: name.to_string(),
                games,
                won,
                score,
            });
        }
        info!(
            p1 = %settings.names[0],
            p2 = %settings.names[1],
            "settings loaded"
        );
        settings
    }

    /// Write the settings file.
    pub fn save(&self, path: &Path) {
        let mut out = String::from("# VolleyForge settings\n");
        for i in 0..6 {
            out.push_str(&format!(
                "key{} = {}\n",
                i,
                key_name(self.keys.get(i / 3, i % 3))
            ));
        }
        for i in 0..2 {
            out.push_str(&format!("control{} = {}\n", i, self.control[i].index()));
        }
        for i in 0..2 {
            out.push_str(&format!("name{} = {}\n", i, self.names[i]));
        }
        for i in 0..2 {
            out.push_str(&format!("color{} = {}\n", i, self.colors[i]));
        }
        out.push_str(&format!("sound = {}\n", self.sound_on as u8));
        out.push_str(&format!("language = {}\n", self.language));
        out.push_str(&format!("fullscreen = {}\n", self.fullscreen as u8));
        for rec in &self.stats {
            out.push_str(&format!(
                "stat = {}|{}|{}|{}\n",
                rec.name.replace('|', " "),
                rec.games,
                rec.won,
                rec.score
            ));
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
                let _ = storage.set_item("volleyforge_settings", &out);
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
                let _ = storage.set_item("volleyforge_settings", &out);
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        match std::fs::write(path, out) {
            Ok(()) => info!(path = %path.display(), "settings saved"),
            Err(e) => warn!(path = %path.display(), error = %e, "could not save the settings"),
        }
    }
}

/// A stable, readable name for a key.
fn key_name(k: Key) -> String {
    k.name().to_string()
}

/// Parse a key back from its name.
fn key_from_name(s: &str) -> Option<Key> {
    // `Key::name` is the enum variant in lower case with underscores, e.g.
    // "arrow_left". Walk the list once and compare.
    const ALL: &[Key] = &[
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J,
        Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T,
        Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6,
        Key::Num7, Key::Num8, Key::Num9,
        Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown,
        Key::Space, Key::Enter, Key::Escape, Key::Tab, Key::Backspace,
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6,
    ];
    ALL.iter().find(|k| k.name() == s).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fullscreen flag survives a save and a load.
    #[test]
    fn the_fullscreen_flag_is_persisted() {
        let path = std::env::temp_dir().join("volleyforge_settings_test.vf");
        let mut settings = Settings::default();
        settings.fullscreen = true;
        settings.save(&path);
        let loaded = Settings::load(&path);
        assert!(loaded.fullscreen, "the fullscreen flag was lost");
        // and off again
        let mut settings = Settings::default();
        settings.fullscreen = false;
        settings.save(&path);
        assert!(!Settings::load(&path).fullscreen);
        let _ = std::fs::remove_file(&path);
    }
}