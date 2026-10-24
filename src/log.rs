//! Logging facade, deliberately shaped like SLF4J so the intent reads the
//! same way it would in a Java codebase:
//!
//! ```ignore
//! use crate::log::{info, warn, debug, trace, error};
//!
//! info!(player = 1, "serve started");
//! warn!(score = 17, "score above the documented limit of 15");
//! ```
//!
//! Under the hood this is `tracing`; [`crate::logging`] installs the
//! subscriber that writes to `logs/volleyforge.log` plus the console.
//!
//! Level policy used throughout this crate:
//!
//! | level    | what lands there                                            |
//! |----------|-------------------------------------------------------------|
//! | `error!` | the run cannot continue (unrecoverable state, missing assets) |
//! | `warn!`  | recoverable but noteworthy (fell back, probe failed, no CPU) |
//! | `info!`  | state transitions: rally start/end, points, matches, modes   |
//! | `debug!` | per-event detail: bounces, touches, input edges, render stats|
//! | `trace!` | per-frame state dumps — 200/s, so opt in explicitly          |

pub use tracing::{debug, error, info, trace, warn};

/// Log a panic together with its location before the process dies, so the
/// backtrace lands in the log file instead of only on stderr.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        error!(location, payload = %info, "PANIC");
        eprintln!("volleyforge: panic logged to the log file ({location})");
        previous(info);
    }));
}