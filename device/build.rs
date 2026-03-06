fn main() {
    // Re-run when compile-time env vars used by env!() change
    for var in ["HOST", "PORT", "SSID", "PASSWORD", "DEVICE_NAME", "CA_PATH"] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    // Only use embedded linker scripts for ESP32 target
    let target = std::env::var("TARGET").unwrap_or_default();
    // Enable builds on both desktop and embedded
    if target == "riscv32imc-unknown-none-elf" {
        linker_be_nice();
        println!("cargo:rustc-link-arg=-Tdefmt.x");
        println!("cargo:rustc-link-arg=-Tlinkall.x");
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
