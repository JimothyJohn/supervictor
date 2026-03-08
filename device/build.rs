fn main() {
    // Re-run when any compile-time env!() var changes.
    // Scans src/ for env!("VAR") patterns so new vars are picked up automatically.
    track_compile_env_vars();

    // Only use embedded linker scripts for ESP32 target
    let target = std::env::var("TARGET").unwrap_or_default();
    // Enable builds on both desktop and embedded
    if target == "riscv32imc-unknown-none-elf" {
        linker_be_nice();
        println!("cargo:rustc-link-arg=-Tdefmt.x");
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

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!("💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`");
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
