fn main() {
    // Re-run when any compile-time env!() var changes.
    // Scans src/ for env!("VAR") patterns so new vars are picked up automatically.
    track_compile_env_vars();

    // Only use embedded linker scripts for ESP32-family targets
    let target = std::env::var("TARGET").unwrap_or_default();
    // Enable builds on both desktop and embedded. Xtensa links through
    // xtensa-esp32-elf-gcc, which lacks lld's --error-handling-script, so
    // the old linker_be_nice() hint hook is gone; riscv (ESP32-C3) links
    // through rust-lld as usual.
    if target == "xtensa-esp32-none-elf" || target == "riscv32imc-unknown-none-elf" {
        println!("cargo:rustc-link-arg=-Tlinkall.x");

        // mbedtls-rs-sys's prebuilt libs need printf/snprintf/vsnprintf. WiFi
        // builds get them from esp-wifi-sys's libprintf.a blob; ethernet-only
        // builds compile the same library (vendored mpaland/printf v4.0.0,
        // MIT) from source. `_putchar`, its output backend, is defined in
        // src/network/ethernet.rs. Ethernet is ESP32-classic-only, so the
        // shim (built with the Xtensa GCC) never applies to riscv targets.
        let ethernet_only = std::env::var_os("CARGO_FEATURE_ETHERNET").is_some()
            && std::env::var_os("CARGO_FEATURE_WIFI").is_none();
        if ethernet_only && target == "xtensa-esp32-none-elf" {
            compile_printf_shim();
        }
    }
}

/// Compile csrc/printf.c with the Xtensa GCC that already serves as the
/// linker (installed by espup). Linked as a bare object file — archives are
/// scanned lazily and ours would sit before the mbedtls libs that need it;
/// objects are always included, so link order stops mattering.
fn compile_printf_shim() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set for build scripts");
    let obj = format!("{out_dir}/printf.o");

    println!("cargo:rerun-if-changed=csrc/printf.c");
    println!("cargo:rerun-if-changed=csrc/printf.h");

    // Floats never appear in mbedtls debug output; disabling them drops the
    // soft-float double-formatting paths.
    let status = std::process::Command::new("xtensa-esp32-elf-gcc")
        .args([
            "-c",
            "-Os",
            "-ffunction-sections",
            "-mlongcalls",
            "-DPRINTF_DISABLE_SUPPORT_FLOAT",
            "-DPRINTF_DISABLE_SUPPORT_EXPONENTIAL",
            "csrc/printf.c",
            "-o",
            &obj,
        ])
        .status()
        .expect("xtensa-esp32-elf-gcc not on PATH — run `. ~/export-esp.sh` (Quickstart install sets this up)");
    assert!(status.success(), "compiling csrc/printf.c failed");

    // mpaland v4.0.0 defines printf_() etc. and aliases the standard names
    // only via header macros; mbedtls's prebuilt objects reference the
    // standard names, so rename at the symbol level.
    let status = std::process::Command::new("xtensa-esp32-elf-objcopy")
        .args([
            "--redefine-sym=printf_=printf",
            "--redefine-sym=sprintf_=sprintf",
            "--redefine-sym=snprintf_=snprintf",
            "--redefine-sym=vsnprintf_=vsnprintf",
            "--redefine-sym=vprintf_=vprintf",
            &obj,
        ])
        .status()
        .expect("xtensa-esp32-elf-objcopy not on PATH — run `. ~/export-esp.sh`");
    assert!(status.success(), "renaming printf symbols failed");

    println!("cargo:rustc-link-arg={obj}");
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
