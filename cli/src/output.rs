use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ANSI escape codes
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const RESET: &str = "\x1b[0m";

const ACCENT_COLORS: &[&str] = &[CYAN, YELLOW, MAGENTA, GREEN];

static VERBOSE: AtomicBool = AtomicBool::new(false);
static ACCENT_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Enable or disable verbose output globally.
pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

/// Returns `true` if verbose output is enabled.
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn next_accent() -> &'static str {
    let idx = ACCENT_INDEX.fetch_add(1, Ordering::Relaxed);
    ACCENT_COLORS[idx % ACCENT_COLORS.len()]
}

/// Major milestone — always visible.
pub fn milestone(msg: &str) {
    let accent = next_accent();
    println!("\n{BOLD}{accent}== {msg} =={RESET}");
}

/// Step header — verbose only.
pub fn step(msg: &str) {
    if !is_verbose() {
        return;
    }
    println!("\n{BOLD}{CYAN}=> {msg}{RESET}");
}

/// Success message.
pub fn success(msg: &str) {
    println!("{GREEN}{msg}{RESET}");
}

/// Error message to stderr.
pub fn error(msg: &str) {
    eprintln!("{RED}{msg}{RESET}");
}

/// Info — always visible, no color.
pub fn info(msg: &str) {
    println!("{msg}");
}

/// Interactive yes/no confirmation. Returns true for "y" or "yes".
pub fn confirm(prompt: &str) -> bool {
    print!("{BOLD}{prompt}{RESET}");
    if io::stdout().flush().is_err() {
        return false;
    }
    let stdin = io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => false, // EOF
        Ok(_) => {
            let answer = line.trim().to_lowercase();
            answer == "y" || answer == "yes"
        }
        Err(_) => false,
    }
}

/// Reset verbose flag and accent color index (used in tests).
pub fn reset_state() {
    VERBOSE.store(false, Ordering::Relaxed);
    ACCENT_INDEX.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbose_default_false() {
        reset_state();
        assert!(!is_verbose());
    }

    #[test]
    fn test_set_verbose() {
        reset_state();
        set_verbose(true);
        assert!(is_verbose());
        set_verbose(false);
        assert!(!is_verbose());
    }

    #[test]
    fn test_accent_cycle() {
        reset_state();
        assert_eq!(next_accent(), CYAN);
        assert_eq!(next_accent(), YELLOW);
        assert_eq!(next_accent(), MAGENTA);
        assert_eq!(next_accent(), GREEN);
        // Wraps around
        assert_eq!(next_accent(), CYAN);
    }
}
