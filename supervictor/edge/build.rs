fn main() {
    // Re-run when any compile-time env!() var changes.
    // Scans src/ for env!("VAR") patterns so new vars are picked up automatically.
    track_compile_env_vars();

    // Only use embedded linker scripts for ESP32 target
    let target = std::env::var("TARGET").unwrap_or_default();
    // Enable builds on both desktop and embedded. Xtensa links through
    // xtensa-esp32-elf-gcc, which lacks lld's --error-handling-script, so
    // the old linker_be_nice() hint hook is gone.
    if target == "xtensa-esp32-none-elf" {
        println!("cargo:rustc-link-arg=-Tlinkall.x");
    }
}

/// Scan src/**/*.rs for `env!("VAR")` and emit `cargo:rerun-if-env-changed` for each.
fn track_compile_env_vars() {
    use std::collections::HashSet;
    use std::path::Path;

    let mut seen = HashSet::new();
    walk_rs(Path::new("src"), &mut |contents| {
        for chunk in contents.split("env!(\"").skip(1) {
            if let Some(name) = chunk.split('"').next() {
                if seen.insert(name.to_string()) {
                    println!("cargo:rerun-if-env-changed={name}");
                }
            }
        }
    });
}

/// Recursively visit .rs files under `dir`, calling `f` with each file's contents.
fn walk_rs(dir: &std::path::Path, f: &mut dyn FnMut(&str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                println!("cargo:rerun-if-changed={}", path.display());
                f(&contents);
            }
        }
    }
}
