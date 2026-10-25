//! Industrial machine profiles for the telemetry simulator.
//!
//! A profile turns the bench simulator into a stand-in for one piece of
//! plant equipment: the current trace follows that machine's duty cycle (an
//! extruder soaks its barrel heaters before the screw turns, a press brake
//! idles its pump between strokes, a compressor cycles on tank pressure) on
//! top of a working day of production runs, breaks and shutdowns. Everything
//! is integer arithmetic on the simulator's seeded RNG, stepped one simulated
//! second at a time, so the same seed yields the same trace on the ESP32 and
//! on the host.
//!
//! `current` is the supply current on one leg, in mA, averaged over the
//! uplink interval the way an energy meter integrates: a two-second press
//! stroke inside a five-second window shows up as a bump, not as a missed
//! spike. Temperature is what a probe on that machine would read — barrel or
//! oven temperature where the machine heats, motor-housing temperature
//! otherwise.
//!
//! Durations are "demo realistic": real cycle times (a bend takes seconds, a
//! CNC roughing pass minutes) but compressed shifts (a production run is
//! 15-45 min, a break 3-10 min, a shutdown 10-30 min) so a dashboard shows a
//! whole working day within an hour or two.
//! [`crate::models::sim::Simulator::set_time_scale`] compresses further.

use super::sim::Xorshift64;

/// Which piece of equipment a device pretends to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The original bench trace: a per-device baseline with drift, noise,
    /// spikes and dropouts. No machine model.
    Generic,
    /// Single-screw plastics extruder: barrel heaters soak, then the screw
    /// motor runs steadily while heater zones cycle; feed starvation dips,
    /// die changes.
    Extruder,
    /// Hydraulic press brake: pump idles between strokes; each bend is a
    /// short high-current stroke, operator-paced.
    PressBrake,
    /// CNC mill: spindle/servo base load, roughing and finishing passes,
    /// rapids, tool changes, part load pauses.
    Cnc,
    /// Band / cold saw: blade idles, cuts are plateaus that creep up as the
    /// blade dulls until a blade change resets them.
    Saw,
    /// Resistive oven: thermostat bang-bang between two levels, full-on soak
    /// from cold, door openings.
    Oven,
    /// Reciprocating air compressor: off until cut-in pressure, runs to
    /// cut-out; start inrush; duty depends on air demand.
    Compressor,
    /// Belt conveyor: light steady load, heavier when loaded, occasional jam
    /// (stall then overload trip).
    Conveyor,
    /// Injection molding machine: barrel heaters plus a very regular
    /// clamp/inject/hold/cool/eject hydraulic cycle; mold changes.
    Molder,
    /// MIG welder: idle fan between arcs, noisy arc-on bursts, fit-up pauses.
    Welder,
    /// Centrifugal process pump: steady 24/7 load with slow setpoint changes
    /// and cavitation noise bursts.
    Pump,
}

/// Where a machine is in its working day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    /// Producing parts.
    Running,
    /// Operator break: machine powered, not producing.
    Break,
    /// Main disconnect off.
    Off,
}

/// Ambient (milli-°C) the thermal models cool toward.
pub const AMBIENT_MILLI: i32 = 22_000;
/// Hottest temperature any profile reports (milli-°C): an extruder barrel.
pub const TEMP_MILLI_MAX: i32 = 400_000;
/// Motor-housing rise above ambient at full load (milli-°C).
const MOTOR_RISE_MILLI: i32 = 40_000;
/// Heater element overshoot above its setpoint while energised (milli-°C).
const HEATER_DRIVE_MILLI: i32 = 60_000;
/// Thermostat hysteresis, each side of the setpoint (milli-°C).
const HYSTERESIS_MILLI: i32 = 2_000;
/// Motor start inrush as a one-second mean, in multiples of rated current.
const INRUSH_X: i32 = 3;
/// Bound on the emitted current: inrush is the biggest thing we ever report.
const MAX_X: i32 = 6;
/// A tripped motor stays off this long (seconds).
const TRIP_MIN_S: u32 = 60;
const TRIP_MAX_S: u32 = 600;
/// One second in N while running trips the overload (about once per 10 h).
const TRIP_ONE_IN: u32 = 36_000;
/// Oven door openings and pump cavitation / conveyor jams: one in N seconds
/// (the latter two are rolled once per planned stage, hence the `/ 60`).
const DOOR_ONE_IN: u32 = 300;
const CAVITATION_ONE_IN: u32 = 400;
const JAM_ONE_IN: u32 = 600;

const MIN_S: u32 = 60;
/// Shift lengths (seconds).
const RUN_MIN_S: u32 = 15 * MIN_S;
const RUN_MAX_S: u32 = 45 * MIN_S;
const BREAK_MIN_S: u32 = 3 * MIN_S;
const BREAK_MAX_S: u32 = 10 * MIN_S;
const OFF_MIN_S: u32 = 10 * MIN_S;
const OFF_MAX_S: u32 = 30 * MIN_S;
/// After a run, this many in ten go to a break; the rest shut down.
const BREAK_AFTER_RUN_IN_TEN: u32 = 7;

impl Profile {
    /// Every profile, in the order the docs list them.
    pub const ALL: [Profile; 11] = [
        Profile::Generic,
        Profile::Extruder,
        Profile::PressBrake,
        Profile::Cnc,
        Profile::Saw,
        Profile::Oven,
        Profile::Compressor,
        Profile::Conveyor,
        Profile::Molder,
        Profile::Welder,
        Profile::Pump,
    ];

    /// Canonical name, as accepted by [`Profile::parse`].
    pub const fn name(self) -> &'static str {
        match self {
            Profile::Generic => "generic",
            Profile::Extruder => "extruder",
            Profile::PressBrake => "press-brake",
            Profile::Cnc => "cnc",
            Profile::Saw => "saw",
            Profile::Oven => "oven",
            Profile::Compressor => "compressor",
            Profile::Conveyor => "conveyor",
            Profile::Molder => "molder",
            Profile::Welder => "welder",
            Profile::Pump => "pump",
        }
    }

    /// Parse a profile name (lower-case; a few aliases). `const` so a
    /// misspelt `SIM_PROFILE` fails the firmware build instead of silently
    /// falling back.
    pub const fn parse(name: &str) -> Option<Profile> {
        if str_eq(name, "generic") || str_eq(name, "bench") {
            Some(Profile::Generic)
        } else if str_eq(name, "extruder") {
            Some(Profile::Extruder)
        } else if str_eq(name, "press-brake") || str_eq(name, "press") {
            Some(Profile::PressBrake)
        } else if str_eq(name, "cnc") || str_eq(name, "mill") || str_eq(name, "machine-tool") {
            Some(Profile::Cnc)
        } else if str_eq(name, "saw") {
            Some(Profile::Saw)
        } else if str_eq(name, "oven") || str_eq(name, "heater") {
            Some(Profile::Oven)
        } else if str_eq(name, "compressor") {
            Some(Profile::Compressor)
        } else if str_eq(name, "conveyor") {
            Some(Profile::Conveyor)
        } else if str_eq(name, "molder") || str_eq(name, "injection") {
            Some(Profile::Molder)
        } else if str_eq(name, "welder") {
            Some(Profile::Welder)
        } else if str_eq(name, "pump") {
            Some(Profile::Pump)
        } else {
            None
        }
    }

    /// Full-load current of the main motor (or element), mA on one leg.
    pub const fn rated_ma(self) -> i32 {
        match self {
            Profile::Generic => 2_000,
            Profile::Extruder => 30_000,
            Profile::PressBrake => 30_000,
            Profile::Cnc => 40_000,
            Profile::Saw => 15_000,
            Profile::Oven => 25_000,
            Profile::Compressor => 20_000,
            Profile::Conveyor => 8_000,
            Profile::Molder => 60_000,
            Profile::Welder => 40_000,
            Profile::Pump => 25_000,
        }
    }

    /// Hard upper bound on the emitted current (inrush included), mA.
    pub const fn max_ma(self) -> i32 {
        match self {
            Profile::Generic => super::sim::CURRENT_MAX,
            _ => self.rated_ma() * MAX_X,
        }
    }

    /// Barrel/oven heaters: (setpoint milli-°C, element current mA).
    const fn heater(self) -> Option<(i32, i32)> {
        match self {
            Profile::Extruder => Some((200_000, 24_000)),
            Profile::Molder => Some((230_000, 30_000)),
            Profile::Oven => Some((150_000, 25_000)),
            _ => None,
        }
    }

    /// Draw while powered but with the main motor stopped (controls, fans).
    const fn controls_ma(self) -> i32 {
        match self {
            Profile::Oven => 800,
            Profile::Welder => 1_500,
            Profile::Conveyor => 200,
            Profile::Compressor => 150,
            _ => 0,
        }
    }

    /// Default sample-to-sample noise, percent of the current level.
    const fn noise_pct(self) -> i32 {
        match self {
            Profile::Generic => 3,
            Profile::Extruder => 3,
            Profile::PressBrake => 4,
            Profile::Cnc => 6,
            Profile::Saw => 8,
            Profile::Oven => 1,
            Profile::Compressor => 3,
            Profile::Conveyor => 4,
            Profile::Molder => 4,
            Profile::Welder => 12,
            Profile::Pump => 3,
        }
    }

    /// Motors have start inrush; resistive elements and inverter welders don't.
    const fn has_inrush(self) -> bool {
        !matches!(self, Profile::Generic | Profile::Oven | Profile::Welder)
    }

    /// (heating tau, cooling tau) in seconds for the first-order thermal model.
    const fn thermal_taus(self) -> (i32, i32) {
        match self {
            Profile::Extruder | Profile::Molder => (400, 1_500),
            Profile::Oven => (300, 1_200),
            _ => (600, 900),
        }
    }
}

/// `const`-evaluable `&str` equality (no trait calls in const fn).
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `rated * pct / 100` without i32 overflow.
const fn pct(rated: i32, pct: i32) -> i32 {
    ((rated as i64) * (pct as i64) / 100) as i32
}

/// Working-day clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Schedule {
    shift: Shift,
    left_s: u32,
}

impl Schedule {
    fn new(rng: &mut Xorshift64) -> Self {
        Self {
            shift: Shift::Running,
            left_s: rng.range(RUN_MIN_S as i32, RUN_MAX_S as i32) as u32,
        }
    }

    /// One second on; returns the shift now in force and whether it changed.
    fn tick(&mut self, rng: &mut Xorshift64) -> (Shift, bool) {
        if self.left_s > 1 {
            self.left_s -= 1;
            return (self.shift, false);
        }
        let (shift, lo, hi) = match self.shift {
            Shift::Running if rng.below(10) < BREAK_AFTER_RUN_IN_TEN => {
                (Shift::Break, BREAK_MIN_S, BREAK_MAX_S)
            }
            Shift::Running => (Shift::Off, OFF_MIN_S, OFF_MAX_S),
            Shift::Break | Shift::Off => (Shift::Running, RUN_MIN_S, RUN_MAX_S),
        };
        self.shift = shift;
        self.left_s = rng.range(lo as i32, hi as i32) as u32;
        (shift, true)
    }
}

/// First-order thermal mass: moves toward a target with a heating or a
/// cooling time constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Thermal {
    milli: i32,
    heat_tau: i32,
    cool_tau: i32,
}

impl Thermal {
    fn tick(&mut self, target_milli: i32) {
        let delta = target_milli - self.milli;
        let tau = if delta > 0 {
            self.heat_tau
        } else {
            self.cool_tau
        };
        // Round away from zero so small deltas still converge.
        let step = if delta > 0 {
            (delta + tau - 1) / tau
        } else {
            (delta - tau + 1) / tau
        };
        self.milli = (self.milli + step).clamp(0, TEMP_MILLI_MAX);
    }
}

/// One machine, ticked a simulated second at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    profile: Profile,
    schedule: Schedule,
    shift: Shift,
    /// Current mechanical stage (meaning is per profile) and seconds left in it.
    stage: u8,
    stage_left_s: u32,
    /// Main-motor current for the stage, mA.
    level_ma: i32,
    /// Noise in force for the stage, percent.
    noise_pct: i32,
    /// Per-machine personality: steady-state run level, percent of rated.
    duty_pct: i32,
    /// Cycle counter (ops per part, arcs per fit-up, cuts per blade).
    counter: u32,
    /// Blade wear, percent added to cut current.
    wear_pct: i32,
    heater_on: bool,
    /// Seconds of overload trip remaining (motor forced off).
    trip_left_s: u32,
    thermal: Thermal,
    /// Motor was stopped on the previous second (for inrush).
    stopped: bool,
}

impl Machine {
    /// A machine of `profile` with a personality drawn from `rng`. Starts
    /// cold, at the beginning of a production run.
    pub fn new(profile: Profile, rng: &mut Xorshift64) -> Self {
        let (heat_tau, cool_tau) = profile.thermal_taus();
        Self {
            profile,
            schedule: Schedule::new(rng),
            shift: Shift::Running,
            stage: 0,
            stage_left_s: 0,
            level_ma: 0,
            noise_pct: profile.noise_pct(),
            duty_pct: rng.range(55, 80),
            counter: 0,
            wear_pct: 0,
            heater_on: false,
            trip_left_s: 0,
            thermal: Thermal {
                milli: AMBIENT_MILLI,
                heat_tau,
                cool_tau,
            },
            stopped: true,
        }
    }

    /// The profile this machine runs.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Shift in force after the last tick.
    pub fn shift(&self) -> Shift {
        self.shift
    }

    /// Probe temperature after the last tick, milli-°C.
    pub fn temp_milli(&self) -> i32 {
        self.thermal.milli
    }

    /// Advance one simulated second; returns that second's mean supply
    /// current in mA.
    pub fn tick(&mut self, rng: &mut Xorshift64) -> i32 {
        let (shift, changed) = self.schedule.tick(rng);
        self.shift = shift;
        if changed {
            // Re-plan immediately: a shutdown mid-stroke stops the stroke.
            self.stage_left_s = 0;
        }

        let heater_ma = self.heater_tick();

        if self.stage_left_s == 0 {
            self.plan_stage(rng);
        }
        self.stage_left_s -= 1;

        // Faults.
        if self.trip_left_s > 0 {
            self.trip_left_s -= 1;
        } else if self.level_ma > 0 && self.profile.has_inrush() && rng.below(TRIP_ONE_IN) == 0 {
            self.trip_left_s = rng.range(TRIP_MIN_S as i32, TRIP_MAX_S as i32) as u32;
        }
        let motor_ma = if self.trip_left_s > 0 {
            0
        } else {
            self.level_ma
        };

        // Thermal: heaters drive their setpoint; motors warm with load.
        let target = match self.profile.heater() {
            Some((setpoint, _)) if self.heater_on => setpoint + HEATER_DRIVE_MILLI,
            Some(_) => AMBIENT_MILLI,
            None => {
                let load_pct =
                    (i64::from(motor_ma) * 100 / i64::from(self.profile.rated_ma())) as i32;
                AMBIENT_MILLI + pct(MOTOR_RISE_MILLI, load_pct)
            }
        };
        self.thermal.tick(target);
        if self.profile == Profile::Oven && shift == Shift::Running && rng.below(DOOR_ONE_IN) == 0 {
            // Door opened: the cavity dumps heat, the thermostat will catch up.
            self.thermal.milli =
                (self.thermal.milli - rng.range(10_000, 20_000)).max(AMBIENT_MILLI);
        }

        // Compose the second's current.
        let mut ma = if motor_ma > 0 {
            let span = pct(motor_ma, self.noise_pct);
            motor_ma + rng.range(-span, span)
        } else {
            0
        };
        if self.stopped && motor_ma > 0 && self.profile.has_inrush() {
            ma = ma.max(self.profile.rated_ma() * INRUSH_X);
        }
        self.stopped = motor_ma == 0;
        if shift != Shift::Off {
            ma += heater_ma + self.profile.controls_ma();
        }
        ma.clamp(0, self.profile.max_ma())
    }

    /// Thermostat for the heated profiles; returns element current.
    fn heater_tick(&mut self) -> i32 {
        let Some((setpoint, heater_ma)) = self.profile.heater() else {
            return 0;
        };
        if self.shift == Shift::Off {
            self.heater_on = false;
            return 0;
        }
        if self.heater_on {
            if self.thermal.milli > setpoint + HYSTERESIS_MILLI {
                self.heater_on = false;
            }
        } else if self.thermal.milli < setpoint - HYSTERESIS_MILLI {
            self.heater_on = true;
        }
        if self.heater_on {
            heater_ma
        } else {
            0
        }
    }

    /// Enter stage `stage` for `dur_s` seconds at `level_ma` (motor current).
    fn set(&mut self, stage: u8, dur_s: u32, level_ma: i32) {
        self.stage = stage;
        self.stage_left_s = dur_s.max(1);
        self.level_ma = level_ma;
        self.noise_pct = self.profile.noise_pct();
    }

    fn set_rand(&mut self, rng: &mut Xorshift64, stage: u8, lo_s: u32, hi_s: u32, level_ma: i32) {
        let dur = rng.range(lo_s as i32, hi_s as i32) as u32;
        self.set(stage, dur, level_ma);
    }

    /// Barrel still cold: heaters only, screw/pump waits.
    fn needs_soak(&self) -> bool {
        match self.profile.heater() {
            Some((setpoint, _)) => self.thermal.milli < setpoint - 10_000,
            None => false,
        }
    }

    /// Pick the next mechanical stage from the current one and the shift.
    /// Stage numbers are per profile; 0 is always "off", 1 "idle".
    fn plan_stage(&mut self, rng: &mut Xorshift64) {
        let rated = self.profile.rated_ma();
        if self.shift == Shift::Off {
            self.set(0, MIN_S, 0);
            return;
        }
        match self.profile {
            Profile::Generic => self.set(1, MIN_S, pct(rated, self.duty_pct)),

            Profile::Extruder => {
                if self.shift == Shift::Break || self.needs_soak() {
                    // Heaters hold the barrel; screw stopped.
                    self.set(if self.needs_soak() { 2 } else { 1 }, 30, 0);
                } else {
                    let run = pct(rated, self.duty_pct);
                    match rng.below(100) {
                        0..=2 => self.set_rand(rng, 3, 20, 90, pct(run, 40)), // feed starvation
                        3..=5 => self.set_rand(rng, 4, 120, 480, 0),          // die change
                        _ => {
                            let wobble = rng.range(-3, 3);
                            self.set_rand(rng, 5, 300, 900, pct(run, 100 + wobble));
                        }
                    }
                }
            }

            Profile::PressBrake => {
                let idle = pct(rated, 30);
                if self.shift == Shift::Break {
                    self.set(1, MIN_S, idle);
                    return;
                }
                match self.stage {
                    10 => self.set_rand(rng, 11, 1, 3, pct(rated, 45)), // approach
                    11 => {
                        let force = rng.range(150, 220);
                        self.set_rand(rng, 12, 1, 4, pct(rated, force)); // bend
                    }
                    12 => self.set_rand(rng, 13, 1, 3, pct(rated, 60)), // return
                    _ if rng.below(40) == 0 => self.set_rand(rng, 14, 120, 360, idle), // tooling setup
                    _ => self.set_rand(rng, 10, 8, 40, idle), // waiting on the operator
                }
            }

            Profile::Cnc => {
                let base = pct(rated, 15);
                if self.shift == Shift::Break {
                    self.set(1, MIN_S, base);
                    return;
                }
                match self.stage {
                    20 => {
                        self.counter = rng.range(2, 5) as u32;
                        self.set_rand(rng, 21, 2, 8, pct(rated, 20)); // rapid
                    }
                    21 => {
                        let load = rng.range(70, 95);
                        self.set_rand(rng, 22, 20, 120, pct(rated, load)); // roughing
                    }
                    22 => {
                        let load = rng.range(35, 50);
                        self.set_rand(rng, 23, 20, 90, pct(rated, load)); // finishing
                    }
                    23 => self.set_rand(rng, 24, 5, 15, base), // tool change
                    24 if self.counter > 1 => {
                        self.counter -= 1;
                        self.set_rand(rng, 21, 2, 8, pct(rated, 20));
                    }
                    _ => self.set_rand(rng, 20, 30, 120, base), // part load/unload
                }
            }

            Profile::Saw => {
                if self.shift == Shift::Break {
                    self.set(1, MIN_S, 0);
                    return;
                }
                match self.stage {
                    30 => {
                        let load = rng.range(60, 100);
                        let cut = pct(pct(rated, load), 100 + self.wear_pct);
                        self.counter += 1;
                        if self.counter.is_multiple_of(5) {
                            self.wear_pct += 1;
                        }
                        self.set_rand(rng, 31, 8, 60, cut); // cutting
                    }
                    31 => self.set_rand(rng, 32, 3, 8, pct(rated, 30)), // retract
                    _ if self.wear_pct >= 30 => {
                        self.wear_pct = 0;
                        self.counter = 0;
                        self.set_rand(rng, 33, 180, 600, 0); // blade change
                    }
                    _ => self.set_rand(rng, 30, 10, 40, pct(rated, 25)), // positioning stock
                }
            }

            // The element is the aux heater; the "motor" is just the fan.
            Profile::Oven => self.set(1, MIN_S, 0),

            Profile::Compressor => {
                let (idle_lo, idle_hi, run_lo, run_hi) = if self.shift == Shift::Break {
                    (480, 1_200, 60, 90)
                } else {
                    (60, 300, 60, 180)
                };
                match self.stage {
                    40 => self.set_rand(rng, 41, idle_lo, idle_hi, 0), // tank full
                    _ => self.set_rand(rng, 40, run_lo, run_hi, pct(rated, 85)), // pumping
                }
            }

            Profile::Conveyor => {
                if self.shift == Shift::Break {
                    self.set(1, MIN_S, 0);
                    return;
                }
                match self.stage {
                    52 => {
                        // Stall ends in an overload trip.
                        self.trip_left_s = rng.range(TRIP_MIN_S as i32, TRIP_MAX_S as i32) as u32;
                        self.set(50, self.trip_left_s, pct(rated, 40));
                    }
                    _ if rng.below(JAM_ONE_IN / 60) == 0 => {
                        self.set_rand(rng, 52, 2, 5, pct(rated, 250)); // jam
                    }
                    50 => {
                        let load = rng.range(60, 90);
                        self.set_rand(rng, 51, 30, 300, pct(rated, load)); // loaded
                    }
                    _ => self.set_rand(rng, 50, 20, 120, pct(rated, 40)), // empty belt
                }
            }

            Profile::Molder => {
                let idle = pct(rated, 20);
                if self.shift == Shift::Break || self.needs_soak() {
                    self.set(if self.needs_soak() { 2 } else { 1 }, 30, idle);
                    return;
                }
                match self.stage {
                    60 => self.set_rand(rng, 61, 2, 4, pct(rated, 180)), // inject
                    61 => self.set_rand(rng, 62, 3, 8, rated),           // hold
                    62 => {
                        let cool = (10 + self.duty_pct / 4) as u32; // per-machine cooling time
                        self.set_rand(rng, 63, cool, cool + 2, pct(rated, 25));
                    }
                    63 => self.set(64, 2, pct(rated, 60)), // eject
                    _ if rng.below(60) == 0 => self.set_rand(rng, 65, 300, 900, idle), // mold change
                    _ => self.set(60, 2, pct(rated, 80)),                              // clamp
                }
            }

            Profile::Welder => {
                if self.shift == Shift::Break {
                    self.set(1, MIN_S, 0);
                    return;
                }
                match self.stage {
                    70 => {
                        if self.counter > 1 {
                            self.counter -= 1;
                            self.set_rand(rng, 71, 3, 30, 0); // reposition
                        } else {
                            self.set_rand(rng, 72, 60, 300, 0); // fit-up
                        }
                    }
                    71 => {
                        let amps = rng.range(60, 110);
                        self.set_rand(rng, 70, 2, 25, pct(rated, amps)); // arc on
                    }
                    _ => {
                        self.counter = rng.range(10, 30) as u32;
                        let amps = rng.range(60, 110);
                        self.set_rand(rng, 70, 2, 25, pct(rated, amps));
                    }
                }
            }

            Profile::Pump => {
                // Runs through breaks; only a shutdown stops it.
                let load = rng.range(70, 85);
                let level = pct(rated, load);
                if self.shift == Shift::Running && rng.below(CAVITATION_ONE_IN / 60) == 0 {
                    self.set_rand(rng, 81, 10, 60, level);
                    self.noise_pct = 15;
                } else {
                    self.set_rand(rng, 80, 300, 1_800, level);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;

    const DAY_S: u32 = 86_400;

    fn machine(profile: Profile, seed: u64) -> (Machine, Xorshift64) {
        let mut rng = Xorshift64::new(seed);
        let m = Machine::new(profile, &mut rng);
        (m, rng)
    }

    /// Run `secs` seconds, returning every per-second current.
    fn trace(profile: Profile, seed: u64, secs: u32) -> (Machine, Vec<i32>) {
        let (mut m, mut rng) = machine(profile, seed);
        let out = (0..secs).map(|_| m.tick(&mut rng)).collect();
        (m, out)
    }

    #[test]
    fn names_round_trip_and_unknown_rejected() {
        for p in Profile::ALL {
            assert_eq!(Profile::parse(p.name()), Some(p), "{p:?}");
        }
        assert_eq!(Profile::parse("press"), Some(Profile::PressBrake));
        assert_eq!(Profile::parse("heater"), Some(Profile::Oven));
        assert_eq!(Profile::parse("mill"), Some(Profile::Cnc));
        assert_eq!(Profile::parse("Extruder"), None, "names are lower-case");
        assert_eq!(Profile::parse(""), None);
        assert_eq!(Profile::parse("extruders"), None);
        // Usable in a const: this is what the firmware does with SIM_PROFILE.
        const P: Option<Profile> = Profile::parse("compressor");
        assert_eq!(P, Some(Profile::Compressor));
    }

    #[test]
    fn same_seed_same_trace_every_profile() {
        for p in Profile::ALL {
            let (_, a) = trace(p, 11, 3_000);
            let (_, b) = trace(p, 11, 3_000);
            assert_eq!(a, b, "{p:?}");
        }
    }

    #[test]
    fn profiles_differ_from_each_other() {
        let (_, generic) = trace(Profile::Generic, 5, 600);
        for p in Profile::ALL.into_iter().skip(1) {
            let (_, t) = trace(p, 5, 600);
            assert_ne!(t, generic, "{p:?} looks like Generic");
        }
    }

    #[test]
    fn current_and_temperature_stay_in_bounds_for_a_day() {
        for p in Profile::ALL {
            for seed in 1..4u64 {
                let (mut m, mut rng) = machine(p, seed * 0x1234_5678);
                for _ in 0..DAY_S {
                    let ma = m.tick(&mut rng);
                    assert!((0..=p.max_ma()).contains(&ma), "{p:?} {ma}");
                    let t = m.temp_milli();
                    assert!((0..=TEMP_MILLI_MAX).contains(&t), "{p:?} {t}");
                }
            }
        }
    }

    /// Every machine works a day with runs, breaks and shutdowns, and goes
    /// dark when shut down.
    #[test]
    fn working_day_has_every_shift_and_off_means_zero() {
        for p in Profile::ALL {
            let (mut m, mut rng) = machine(p, 99);
            let mut seen = [false; 3];
            let mut off_nonzero = 0;
            for _ in 0..DAY_S {
                let ma = m.tick(&mut rng);
                match m.shift() {
                    Shift::Running => seen[0] = true,
                    Shift::Break => seen[1] = true,
                    Shift::Off => {
                        seen[2] = true;
                        if ma != 0 {
                            off_nonzero += 1;
                        }
                    }
                }
            }
            assert_eq!(seen, [true; 3], "{p:?} missed a shift state");
            assert_eq!(off_nonzero, 0, "{p:?} drew current while off");
        }
    }

    #[test]
    fn shift_lengths_are_within_their_bands() {
        let mut rng = Xorshift64::new(3);
        let mut s = Schedule::new(&mut rng);
        let mut lengths: Vec<(Shift, u32)> = Vec::new();
        let mut cur = (s.shift, 0u32);
        for _ in 0..DAY_S * 3 {
            let (shift, changed) = s.tick(&mut rng);
            if changed {
                lengths.push(cur);
                cur = (shift, 0);
            }
            cur.1 += 1;
        }
        assert!(lengths.len() > 40, "{}", lengths.len());
        for (shift, len) in lengths.into_iter().skip(1) {
            let (lo, hi) = match shift {
                Shift::Running => (RUN_MIN_S, RUN_MAX_S),
                Shift::Break => (BREAK_MIN_S, BREAK_MAX_S),
                Shift::Off => (OFF_MIN_S, OFF_MAX_S),
            };
            assert!((lo..=hi).contains(&len), "{shift:?} lasted {len}s");
        }
    }

    /// Oven: a thermostat holds the setpoint once warm, and the element is
    /// either fully on or fully off — two levels, nothing in between.
    #[test]
    fn oven_is_bang_bang_around_its_setpoint() {
        let (setpoint, element) = Profile::Oven.heater().unwrap();
        let fan = Profile::Oven.controls_ma();
        let (mut m, mut rng) = machine(Profile::Oven, 21);
        let mut warm_secs = 0;
        let mut out_of_band = 0;
        let mut levels = [0u32; 2]; // fan only, fan + element
        for i in 0..6 * 3_600 {
            let ma = m.tick(&mut rng);
            if m.shift() == Shift::Off {
                continue;
            }
            if ma == fan {
                levels[0] += 1;
            } else if ma == fan + element {
                levels[1] += 1;
            } else {
                panic!("oven current {ma} is neither fan nor fan+element");
            }
            if i > 1_800 && m.shift() == Shift::Running {
                warm_secs += 1;
                if (m.temp_milli() - setpoint).abs() > 25_000 {
                    out_of_band += 1;
                }
            }
        }
        assert!(levels[0] > 0 && levels[1] > 0, "{levels:?}");
        assert!(
            out_of_band * 10 < warm_secs,
            "oven off its setpoint {out_of_band}/{warm_secs} s"
        );
    }

    /// Extruder: cold start soaks (heaters only, screw stopped) before the
    /// screw motor is allowed to run.
    #[test]
    fn extruder_soaks_before_running() {
        let (mut m, mut rng) = machine(Profile::Extruder, 8);
        let (_, heater) = Profile::Extruder.heater().unwrap();
        let mut first_motor_second = None;
        for i in 0..3_600 {
            let ma = m.tick(&mut rng);
            if m.level_ma > 0 && first_motor_second.is_none() {
                first_motor_second = Some(i);
            }
            if first_motor_second.is_none() {
                assert!(ma == 0 || ma == heater, "soak draws {ma} mA at {i}s");
            }
        }
        let start = first_motor_second.expect("screw never started");
        assert!(start > 60, "screw started after only {start}s");
        assert!(
            m.temp_milli() > 150_000,
            "barrel still cold: {}",
            m.temp_milli()
        );
    }

    /// Press brake: strokes push above rated current, the idle pump sits well
    /// below, and the time-average lands between.
    #[test]
    fn press_brake_strokes_spike_over_an_idle_pump() {
        let rated = Profile::PressBrake.rated_ma();
        let (mut m, mut rng) = machine(Profile::PressBrake, 4);
        let mut running: Vec<i32> = Vec::new();
        for _ in 0..3 * 3_600 {
            let ma = m.tick(&mut rng);
            if m.shift() == Shift::Running && m.trip_left_s == 0 {
                running.push(ma);
            }
        }
        let max = *running.iter().max().unwrap();
        let min = *running.iter().filter(|&&v| v > 0).min().unwrap();
        let mean = running.iter().map(|&v| i64::from(v)).sum::<i64>() / running.len() as i64;
        assert!(max > rated * 14 / 10, "no stroke above rated: {max}");
        assert!(min < rated * 4 / 10, "idle pump too hot: {min}");
        assert!(
            mean > i64::from(min) && mean < i64::from(rated),
            "mean {mean}"
        );
    }

    #[test]
    fn compressor_cycles_between_off_and_pumping() {
        let rated = Profile::Compressor.rated_ma();
        let controls = Profile::Compressor.controls_ma();
        let (_, t) = trace(Profile::Compressor, 12, 2 * 3_600);
        let idle = t.iter().filter(|&&v| v == controls).count();
        let pumping = t.iter().filter(|&&v| v > rated * 7 / 10).count();
        assert!(
            idle > 600 && pumping > 600,
            "idle {idle}s, pumping {pumping}s"
        );
        let starts = t
            .windows(2)
            .filter(|w| w[0] < rated / 2 && w[1] >= rated * 2)
            .count();
        assert!(starts >= 5, "only {starts} inrush starts seen");
    }

    #[test]
    fn saw_blade_wears_then_gets_changed() {
        let (mut m, mut rng) = machine(Profile::Saw, 17);
        let mut peak_wear = 0;
        let mut changed = false;
        for _ in 0..DAY_S {
            m.tick(&mut rng);
            peak_wear = peak_wear.max(m.wear_pct);
            if m.stage == 33 {
                changed = true;
            }
        }
        assert!(peak_wear >= 25, "blade never dulled: {peak_wear}");
        assert!(changed, "blade never changed");
    }

    #[test]
    fn conveyor_jams_stall_then_trip() {
        let rated = Profile::Conveyor.rated_ma();
        let controls = Profile::Conveyor.controls_ma();
        let (_, t) = trace(Profile::Conveyor, 2, DAY_S);
        let stalls = t.iter().filter(|&&v| v >= rated * 2).count();
        assert!(stalls > 0, "no jam in a day");
        // Right after a stall the belt is tripped: controls only.
        let tripped = t.windows(8).any(|w| w[0] >= rated * 2 && w[7] == controls);
        assert!(tripped, "jam did not trip the belt");
    }

    #[test]
    fn molder_cycle_is_regular_and_hot() {
        let rated = Profile::Molder.rated_ma();
        let (mut m, mut rng) = machine(Profile::Molder, 33);
        let mut injects = 0;
        for _ in 0..3 * 3_600 {
            let ma = m.tick(&mut rng);
            if m.stage == 61 && ma > rated * 15 / 10 {
                injects += 1;
            }
        }
        assert!(injects > 100, "only {injects} injection seconds in 3 h");
        assert!(m.temp_milli() > 150_000 || m.shift() == Shift::Off);
    }

    #[test]
    fn welder_alternates_arcs_and_idle() {
        let (_, t) = trace(Profile::Welder, 6, 3_600);
        let idle = Profile::Welder.controls_ma();
        let arcs = t.iter().filter(|&&v| v > idle * 10).count();
        let idles = t.iter().filter(|&&v| v == idle).count();
        assert!(arcs > 300 && idles > 300, "arcs {arcs}s idle {idles}s");
    }

    #[test]
    fn pump_runs_through_breaks() {
        let (mut m, mut rng) = machine(Profile::Pump, 44);
        let mut break_secs = 0;
        let mut break_zero = 0;
        for _ in 0..DAY_S {
            let ma = m.tick(&mut rng);
            if m.shift() == Shift::Break && m.trip_left_s == 0 {
                break_secs += 1;
                if ma == 0 {
                    break_zero += 1;
                }
            }
        }
        assert!(break_secs > 0);
        assert_eq!(break_zero, 0, "pump stopped during a break");
    }

    #[test]
    fn motor_start_shows_inrush_but_elements_do_not() {
        for p in Profile::ALL {
            let (_, t) = trace(p, 77, DAY_S);
            let peak = *t.iter().max().unwrap();
            if p.has_inrush() {
                assert!(peak >= p.rated_ma() * INRUSH_X, "{p:?} peak {peak}");
            } else {
                assert!(peak < p.rated_ma() * 2, "{p:?} peak {peak}");
            }
        }
    }

    #[test]
    fn thermal_converges_both_ways() {
        let mut t = Thermal {
            milli: AMBIENT_MILLI,
            heat_tau: 300,
            cool_tau: 1_200,
        };
        for _ in 0..5_000 {
            t.tick(210_000);
        }
        assert!((209_000..=210_000).contains(&t.milli), "{}", t.milli);
        for _ in 0..20_000 {
            t.tick(AMBIENT_MILLI);
        }
        assert!(
            (AMBIENT_MILLI..=AMBIENT_MILLI + 1_000).contains(&t.milli),
            "{}",
            t.milli
        );
    }
}
