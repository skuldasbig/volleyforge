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
//! port uses [`rodio`] so that the game also has sound on macOS and Linux.
//! The behaviour is the same: one sound at a time - a new sound cuts off the
//! running one, just like `PlaySoundA` does.

use std::io::{BufReader, Cursor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use rodio::{Decoder, OutputStream, Sink, Source};

use crate::log::warn;

/// Are the sound effects switched on? The original keeps the switch at
/// `+0x97822C` and only plays when it is 1.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// The audio output. It is opened when the first sound is played, so a
/// machine without a sound device does not stop the game.
///
/// `rodio::OutputStream` is not `Send`, so the stream lives in its own
/// thread and the port only sends it the raw bytes of an effect.
#[cfg(not(target_arch = "wasm32"))]
fn player() -> Option<&'static std::sync::mpsc::Sender<Vec<u8>>> {
    static SENDER: OnceLock<Option<std::sync::mpsc::Sender<Vec<u8>>>> = OnceLock::new();
    SENDER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                let Ok((stream, handle)) = OutputStream::try_default() else {
                    warn!("no audio device - the game runs without sound");
                    return;
                };
                let Ok(sink) = Sink::try_new(&handle) else {
                    warn!("no audio output - the game runs without sound");
                    return;
                };
                for bytes in rx {
                    let cursor = Cursor::new(bytes);
                    let Ok(decoder) = Decoder::new(BufReader::new(cursor)) else {
                        warn!("the sound effect could not be decoded");
                        continue;
                    };
                    // `PlaySoundA` stops the running sound when a new one
                    // starts, so the port does the same.
                    sink.stop();
                    sink.append(decoder);
                    sink.play();
                }
                drop(stream);
            });
            Some(tx)
        })
        .as_ref()
}

/// The two buffers of `sound.pak`: index 0 = sound A, index 1 = sound B.
static BUFFERS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// The decoded sound effects.
fn buffers() -> &'static Vec<Vec<u8>> {
    BUFFERS.get_or_init(Vec::new)
}

/// Hand the decoded sound effects over to the sound module.
///
/// The loader of the startup screen calls this, so the effects come from the
/// `sound.pak` next to the executable.
pub fn set_buffers(buffers: Vec<Vec<u8>>) {
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

/// Play one of the two sounds, if the sound effects are switched on.

#[cfg(target_arch = "wasm32")]
fn to_base64(data: &[u8]) -> String {
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(B64_CHARS[(b0 >> 2) as usize] as char);
        out.push(B64_CHARS[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn play_web(index: usize) {
    use web_sys::HtmlAudioElement;
    static AUDIO_CACHE: OnceLock<[Option<HtmlAudioElement>; 2]> = OnceLock::new();
    let audios = AUDIO_CACHE.get_or_init(|| {
        let bufs = buffers();
        [
            bufs.get(0).and_then(|b| {
                let src = format!("data:audio/wav;base64,{}", to_base64(b));
                HtmlAudioElement::new_with_src(&src).ok()
            }),
            bufs.get(1).and_then(|b| {
                let src = format!("data:audio/wav;base64,{}", to_base64(b));
                HtmlAudioElement::new_with_src(&src).ok()
            }),
        ]
    });
    if let Some(Some(audio)) = audios.get(index) {
        audio.set_current_time(0.0);
        let _ = audio.play();
    }
}

fn play(index: usize) {
    if !enabled() {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        play_web(index);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(bytes) = buffers().get(index) else {
            return;
        };
        let Some(player) = player() else {
            return;
        };
        let _ = player.send(bytes.clone());
    }
}

/// Unused, but keeps the `Source` trait in scope for the decoder.
#[allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
fn _assert_source<S: Source<Item = f32>>(_s: S) {}

#[cfg(all(test, not(target_arch = "wasm32")))]
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
            let cursor = Cursor::new(s.clone());
            let decoder = Decoder::new(BufReader::new(cursor)).expect("the effect decodes");
            assert_eq!(decoder.sample_rate(), 44_100, "the sample rate of the effects");
        }
    }
}
