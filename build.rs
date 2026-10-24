use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    for name in ["graph.pak", "sound.pak", "text.pak"] {
        println!("cargo:rerun-if-changed={name}");
    }

    #[cfg(windows)]
    {
        if Path::new("icon.ico").exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon("icon.ico");
            let _ = res.compile();
        }
    }

    copy_containers();
}

/// Copy the original's containers next to the executable.
///
/// The game reads `graph.pak`, `sound.pak` and `text.pak` from the folder of
/// the executable. The copies in the repository root are the developer's
/// convenience - this step puts them where the game looks for them, so
/// `cargo build` alone is enough.
fn copy_containers() {
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR = <target>/<profile>/build/<package>-<hash>/out
    let Some(profile_dir) = Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };
    for name in ["graph.pak", "sound.pak", "text.pak"] {
        let source = Path::new(name);
        if !source.is_file() {
            continue;
        }
        let target = profile_dir.join(name);
        let same = std::fs::read(&target)
            .ok()
            .zip(std::fs::read(source).ok())
            .map(|(a, b)| a == b)
            .unwrap_or(false);
        if same {
            continue;
        }
        if let Err(e) = std::fs::copy(source, &target) {
            println!("cargo:warning=could not copy {name} to {}: {e}", target.display());
        }
    }
}
