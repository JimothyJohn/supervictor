---
name: quickstart
description: Set up a supervictor checkout end to end with judgment instead of prompts — toolchain, .env, fleet CA and certificates, build, tests, a local mTLS endpoint, then a simulated machine or a flashed ESP32 board. Use when the user says "set this up", "get me running", "quickstart", "flash a board", "why won't Quickstart work", or lands in a fresh clone.
---

# Quickstart with an agent

`./Quickstart` already does the whole path and asks only where it must. Your
job is to answer those questions from evidence before it asks, run it
non-interactively, and diagnose the first failure instead of retrying.

## 1. Look before you run

Gather, in parallel, from the repo root:

- `rustup toolchain list`, `cargo --version`, `openssl version`, `which espflash`
- `ls .env.* certs/ca certs/devices certs/admins .logs 2>/dev/null`
- `./Quickstart status` (an endpoint or simulator may already be up)
- Serial ports: `ls /dev/cu.usbserial* /dev/cu.usbmodem* /dev/ttyUSB* /dev/ttyACM* 2>/dev/null`,
  and for each candidate `espflash board-info --port <p>` to learn the chip
  and MAC. `TESTRIG.md` at the repo root (gitignored, machine-specific) maps
  MACs to boards when it exists.
- If `.env.dev` exists, read it: `HOST`, `DEVICE_NAME`, `CA_PATH`, `ESPFLASH_PORT`,
  whether `SSID`/`PASSWORD` are filled. Never print `PASSWORD`.

## 2. Decide the answers

| Question Quickstart would ask | Decide it from |
|---|---|
| device name | `DEVICE_NAME` in `.env.dev`; else the user's words; else `test-device` |
| what posts telemetry | a board on a serial port the user mentioned → `flash`; no board → `sim`; user only wants the API → `none` |
| which board | `espflash board-info`: esp32 + MAC in TESTRIG → poe-iso or poe2 as listed; esp32c3 → c3 |
| which serial port | the one whose chip matches; ask the user only if two boards match |
| HOST for a board | must be reachable from the board on :443. `localhost` means "simulate instead" unless the user names an endpoint |

Export the answers: `QS_HARDWARE`, `QS_BOARD` (poe-iso | poe2 | c3),
`ESPFLASH_PORT`, `QS_PORT` (if 8443 is taken), `QS_TESTS=0` only when the
user explicitly wants to skip tests. For a WiFi board with blank
`SSID`/`PASSWORD`, ask the user for them and write them into `.env.dev`
yourself — the script refuses rather than guessing.

## 3. Run it

```sh
QS_HARDWARE=sim ./Quickstart            # or flash / none, plus the exports above
```

It is idempotent: rerunning after a fix skips every step that already
succeeded. Prefer rerunning the whole thing to hand-running stages.

Flashing needs the Xtensa toolchain (`./Quickstart install` → espup) and the
`~/export-esp.sh` environment; the script sources it when present.

## 4. When it fails

Read the step it printed (`[n/6]`) and the log it names. Fix the cause, rerun.
Stop and tell the user after the second failure of the same step.

| Symptom | Cause → fix |
|---|---|
| `cargo`/`openssl`/`curl` missing | install it; rustup.rs for Rust |
| `CA_PATH ... does not exist` | `.env.dev` points at a root (e.g. `ca/AmazonRootCA1.pem`) that isn't in `certs/`. Local endpoint: `CA_PATH=ca/ca.pem`. Public endpoint: fetch its root into `certs/ca/` |
| build error mentioning `env!("HOST")` or a cert `include_bytes!` | a compile-time var is unset or its file is missing; `.env.dev` is loaded by the script, so check the names there |
| duplicate lang item / libcore errors on a host build | someone ran a host-target cargo command from `supervictor/edge/` (its `.cargo/config.toml` build-std poisons `target/<host>/`). `cargo clean -p supervictor-edge --target <host>` from the root |
| `port 8443 is taken by pid N` | an older endpoint or another service; `./Quickstart down`, or `QS_PORT=9443` |
| endpoint exited | `.logs/endpoint.log`: usually a stale `.logs/local.db` from an older schema (delete it) or a TLS path |
| simulator exited | `.logs/sim.log`: cert paths, or the device is `revoked` on the endpoint |
| `espflash` cannot open the port | another monitor holds it (`lsof <port>`), or the board needs the BOOT button held on reset |
| `xtensa-esp32-elf-gcc not on PATH` | `. ~/export-esp.sh`, or `./Quickstart install` |

## 5. Verify and report

```sh
./Quickstart status                                   # endpoint + simulator + fleet summary
./Quickstart fleet localhost 8443                     # fleet table over the admin cert
curl -sS --cacert certs/ca/ca.pem --cert certs/admins/$USER/admin.pem --key certs/admins/$USER/admin.key \
  https://localhost:8443/fleet/history?window=5m | head -c 400
```

Tell the user: the dashboard URL, which `.p12` to import (its password was
printed once by the cert step — point them at the line, do not re-print
secrets you did not see), what is posting telemetry, and `./Quickstart down`.
