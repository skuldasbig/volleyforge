//! The original's two sound effects.
//!
//! The original plays them through the Win32 API (`FUN_00442964` in
//! `BV_MessageByIndex.c`):
//!
//! ```c
//! if (*(char *)(param_1 + 0x97822C) == '\x01') {
//!     PlaySoundA(param_2, (HMODULE)0x0, 7);
//! }
//! ```
//!
//! `7` is `SND_ASYNC | SND_NODEFAULT | SND_MEMORY`: the sound is played from
//! a memory buffer, asynchronously, without a fallback sound. The two buffers
//! come out of `sound.pak` (`BV_GraphicsUnpacker.c:303-315`): chunk 0 is
//! stored at `+0x9781F0` (sound **A**), chunk 1 at `+0x9781F4` (sound **B**).
//!
//! * **Sound A** - the rally is over (`BV_Phys_H_443e8.c:668`) and the menu
//!   entries "match start" and "quit" (`BV_Menu_00447E0C.c:112/134`).
//! * **Sound B** - the ball touches a blob (`BV_Phys_H_443e8.c:485`) and
//!   every click in the menus (`BV_Menu_00447E0C.c:104-106, 117-130`).
//!
//! **Deliberate deviation:** the original calls `PlaySoundA` directly, the
//! port uses [`kira`] so that the game has cross-platform sound on desktop
//! (Windows, macOS, Linux) and the web (WebAudio via WebAssembly). The behaviour
//! is the same: one sound at a time - a new sound cuts off the running one,
//! just like `PlaySoundA` does.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
};

use crate::log::warn;

/// Are the sound effects switched on? The original keeps the switch at
/// `+0x97822C` and only plays when it is 1.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// The two raw buffers of `sound.pak`: index 0 = sound A, index 1 = sound B.
static BUFFERS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// The pre-decoded sound effects ready for playback.
static SOUNDS: OnceLock<Vec<Option<StaticSoundData>>> = OnceLock::new();

struct AudioPlayer {
    manager: AudioManager<DefaultBackend>,
    current_handle: Option<StaticSoundHandle>,
}

static PLAYER: Mutex<Option<AudioPlayer>> = Mutex::new(None);

/// Hand the decoded sound effects over to the sound module.
///
/// The loader of the startup screen calls this, so the effects come from the
/// `sound.pak` next to the executable.
pub fn set_buffers(buffers: Vec<Vec<u8>>) {
    let sounds: Vec<Option<StaticSoundData>> = buffers
        .iter()
        .map(|bytes| {
            let cursor = Cursor::new(bytes.clone());
            match StaticSoundData::from_cursor(cursor) {
                Ok(data) => Some(data),
                Err(err) => {
                    warn!("could not decode sound effect: {err}");
                    None
                }
            }
        })
        .collect();
    let _ = SOUNDS.set(sounds);
    let _ = BUFFERS.set(buffers);
}

/// Switch the sound effects on or off (the settings row).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Are the sound effects switched on?
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// **Sound A** (chunk 0 of `sound.pak`, stored at `+0x9781F0`).
///
/// The original plays it when the rally is over
/// (`BV_Phys_H_443e8.c:668`), when a match starts and when the game is
/// left (`BV_Menu_00447E0C.c:112/134`).
pub fn sound_a() {
    play(0);
}

/// **Sound B** (chunk 1 of `sound.pak`, stored at `+0x9781F4`).
///
/// The original plays it when the ball touches a blob
/// (`BV_Phys_H_443e8.c:485`) and for every click in the menus
/// (`BV_Menu_00447E0C.c:104-106, 117-130`).
pub fn sound_b() {
    play(1);
}

/// Play a sound by its index in `sound.pak`.
///
/// The original ships the index over the network in its sound packet
/// (`BV_MessageByIndex.c:8-14`) and the guest plays it
/// (`BV_Menu_0044BF98.c:144-146`).
pub fn play_index(index: usize) {
    play(index);
}

/// Play one of the two sounds, if the sound effects are switched on.
fn play(index: usize) {
    if !enabled() {
        return;
    }

    let Some(sounds) = SOUNDS.get() else {
        return;
    };
    let Some(Some(sound_data)) = sounds.get(index) else {
        return;
    };

    let Ok(mut guard) = PLAYER.lock() else {
        return;
    };

    if guard.is_none() {
        match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => {
                *guard = Some(AudioPlayer {
                    manager,
                    current_handle: None,
                });
            }
            Err(err) => {
                warn!("no audio device - the game runs without sound: {err}");
                return;
            }
        }
    }

    if let Some(player) = guard.as_mut() {
        // `PlaySoundA` stops the running sound when a new one
        // starts, so the port does the same.
        if let Some(mut handle) = player.current_handle.take() {
            handle.stop(Tween::default());
        }
        match player.manager.play(sound_data.clone()) {
            Ok(handle) => {
                player.current_handle = Some(handle);
            }
            Err(err) => {
                warn!("sound could not be played: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The original's `sound.pak` - next to the test binary, next to the
    /// program or in the package root.
    fn container() -> Option<Vec<u8>> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("sound.pak"));
                if let Some(up) = dir.parent() {
                    candidates.push(up.join("sound.pak"));
                }
            }
        }
        candidates.push(PathBuf::from("sound.pak"));
        for candidate in candidates {
            if let Ok(data) = std::fs::read(&candidate) {
                return Some(data);
            }
        }
        None
    }

    /// The two effects of `sound.pak` have to be real RIFF/WAVE files: the
    /// original hands them to `PlaySoundA` with `SND_MEMORY`.
    #[test]
    fn the_two_sounds_are_wave_files() {
        let raw = container().expect(
            "sound.pak not found - copy the original's containers next to the program",
        );
        let sounds: Vec<Vec<u8>> = crate::pak::decode_chunks(&raw)
            .into_iter()
            .filter(|c| c.len() > 4 && &c[0..4] == b"RIFF")
            .collect();
        assert_eq!(sounds.len(), 2, "sound.pak holds two effects");
        for s in sounds {
            assert_eq!(&s[0..4], b"RIFF", "RIFF header");
            assert_eq!(&s[8..12], b"WAVE", "WAVE header");
        }
    }

    /// The decoder has to be able to read the effects - the original plays
    /// the same bytes.
    #[test]
    fn the_sounds_can_be_decoded() {
        let raw = container().expect(
            "sound.pak not found - copy the original's containers next to the program",
        );
        let sounds: Vec<Vec<u8>> = crate::pak::decode_chunks(&raw)
            .into_iter()
            .filter(|c| c.len() > 4 && &c[0..4] == b"RIFF")
            .collect();
        for s in sounds {
            let cursor = Cursor::new(s);
            let sound = StaticSoundData::from_cursor(cursor).expect("the effect decodes");
            assert_eq!(sound.sample_rate, 44_100, "the sample rate of the effects");
        }
    }
}
