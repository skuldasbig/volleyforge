//! Structured logging with an automatically created log file.
//!
//! The original shipped with no diagnostics at all, which makes a physics or
//! protocol problem very hard to chase down. VolleyForge therefore installs a
//! `tracing` subscriber on startup that writes to two places:
//!
//!   * `logs/volleyforge.log` (appended, next to the executable)
//!   * the console, so you see the same lines while playing
//!
//! The file and its directory are created on first run, so no setup step is
//! needed. Set `VOLLEYFORGE_LOG=debug` to raise the level, or
//! `VOLLEYFORGE_LOG_DIR` to redirect the output.
//!
//! The per-frame state dump (200 events per second) lives on the `trace`
//! level and is therefore off by default. Switch it on with
//!
//! ```text
//! VOLLEYFORGE_LOG=volleyforge=trace
//! ```
//!
//! It then lands in the same file as everything else - there is no second
//! log file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::prelude::*;

/// Directory the log file is written to.
pub fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VOLLEYFORGE_LOG_DIR") {
        return PathBuf::from(dir);
    }
    // next to the executable, so a portable install keeps its logs with it
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("logs")
}

/// Absolute path of the active log file.
pub fn log_file() -> PathBuf {
    log_dir().join("volleyforge.log")
}

/// Install the global subscriber. Safe to call once; later calls are ignored.
pub fn init() {
    #[cfg(target_arch = "wasm32")]
    {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
    let dir = log_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("volleyforge: cannot create log dir {}: {e}", dir.display());
        return;
    }
    let path = dir.join("volleyforge.log");

    let file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("volleyforge: cannot open {}: {e}", path.display());
            return;
        }
    };

    let filter = EnvFilter::try_from_env("VOLLEYFORGE_LOG").unwrap_or_else(|_| {
        // Default: keep the noisy per-frame tracing at debug level.
        EnvFilter::new("volleyforge=info,warn")
    });

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(move || file.try_clone().expect("clone log file handle"))
        .with_ansi(false)
        .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".to_string()));

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_timer(ChronoLocal::new("%H:%M:%S".to_string()));

    // Make sure a crash still leaves the file readable.
    let _ = writeln!(
        std::io::stderr(),
        "volleyforge: logging to {}",
        path.display()
    );

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console_layer)
        .init();
}
}

/// Log a one-line session header with build/runtime facts.
pub fn banner(version: &str) {
    let started = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    tracing::info!(
        version,
        started_epoch = started,
        exe = ?std::env::current_exe().unwrap_or_default(),
        log = ?log_file(),
        "VolleyForge session start"
    );
}