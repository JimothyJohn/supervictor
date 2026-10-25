---
name: bring-your-own
description: Get a user running supervictor on THEIR OWN hardware and THEIR OWN server, on any platform — stand the endpoint up on a Linux box, VPS, Raspberry Pi, container, macOS or WSL machine (not just the local Quickstart), point it at a hostname they control, and bring telemetry in from a board that is not the bench Olimex (another ESP32 Ethernet or WiFi board, a Linux SBC, or any device that can do mTLS and POST JSON). Use this whenever someone says "my own board", "my own server", "deploy this on my VPS / Pi / NAS / home server", "port to <board>", "I don't have an Olimex", "self-host", "connect my device", "custom hardware", "how do I run the backend somewhere", or asks what the minimum contract is for a device or a server to join the fleet — even if they don't say the word supervictor. For the stock local setup (this laptop, a supported board over USB) use /quickstart instead; come here as soon as the server leaves localhost or the device is not in Quickstart's board menu.
---

# Bring your own board and server

The system is three identities and one wire contract. Everything else — which
OS the server runs, which chip the device is, whether the fleet is one bench
board or a rack — is a choice this skill helps the user make and then verify.
Nothing here needs AWS, Docker, or a specific OS; those are optional paths in
the references.

```
 fleet CA (certs/ca/ca.pem, ca.key)   ← issues every identity below; never leaves the dev machine
   ├─ server cert   SAN = the HOST devices dial            → the endpoint's TLS_CERT/TLS_KEY
   ├─ device cert   CN = device id, OU=devices            → embedded in firmware / passed to a client
   └─ admin cert    OU=admin (+ .p12 for a browser)       → dashboard, /fleet, the MCP server
 wire contract: POST https://HOST:443/  {"id": "<device id>", "current": <mA>, ...}  over mTLS
```

Work through the five steps in order; each one has a check that proves it
before moving on. Read `references/server.md` when you reach step 2 and
`references/board.md` when you reach step 4 — they hold the per-platform
commands and the honest limits.

## 0. Learn what they have (before touching anything)

Decide these from the conversation and the machine, asking only what you
cannot infer:

| Question | Why it matters | Typical answers |
|---|---|---|
| Where will the endpoint run? | picks the server path | this laptop · a LAN box (Pi, NAS, mini-PC) · a VPS/cloud VM · a container host · behind an existing reverse proxy |
| What name will devices dial? | baked into firmware as `HOST`, must be in the server cert's SAN | a DNS name they control · a LAN IP (bench only) · `localhost` (simulator only — boards cannot reach it) |
| What is the device? | picks the board path | Olimex ESP32-POE-ISO / POE2 / an ESP32-C3 (Quickstart knows these) · another ESP32 board · a Linux SBC · something else with TLS |
| Does a CA already exist? | `certs/ca/` is gitignored and per-machine; a second CA with the same name breaks every existing cert | `ls certs/ca` |

Boards dial port **443** — that is a compile-time constant in the firmware,
not configuration. A server on another port (the local Quickstart uses 8443)
serves simulators and the desktop client, not boards.

## 1. The CA and the certificates

From the repo root (`openssl` is the only requirement):

```sh
./Quickstart certs ca                      # once per fleet; keep certs/ca/ca.key private and backed up
./Quickstart certs server <name> <HOST>    # SAN = DNS:<HOST>, or IP:<HOST> for a bare IP
./Quickstart certs device <device-id>      # CN = device id; one per device
./Quickstart certs admin <you>             # OU=admin + admin.p12 (password printed once)
./Quickstart certs list                    # what exists, with expiry
```

`<name>` is only the directory under `certs/servers/`; `<HOST>` is what goes
in the certificate and must equal what devices dial. A board that dials
`sv.example.net` will refuse a certificate for `10.0.0.44` and vice versa.

If the user wants a publicly trusted server certificate (Let's Encrypt),
that only replaces the **server** cert; devices then need that public root in
`certs/ca/` as their `CA_PATH`. The client CA (`ca.pem`) still gates who may
connect. See `references/server.md` → "Public server certificate".

## 2. The endpoint on their server

Pick one path from `references/server.md` and follow its section:

| Path | When | What they end up with |
|---|---|---|
| **A. systemd Linux box** | any Linux with systemd, aarch64 or x86_64 | one static binary, a hardened unit, three PEMs in `/etc/supervictor`, SQLite in `/var/lib/supervictor` |
| **B. bare binary** | macOS, WSL, a dev box, a Linux without systemd, "just run it" | `supervictor-endpoint` built from source, configured by env vars, run under whatever supervisor they have |
| **C. container + Caddy** | they already run Docker Compose | the endpoint on plain HTTP behind Caddy terminating mTLS |
| **D. their own reverse proxy** | nginx/Traefik/Caddy already in front | the endpoint on plain HTTP; the proxy verifies client certs and forwards the subject header |
| **E. AWS Lambda** | they are an AWS shop | pointer only — the SAM template exists; identity propagation has an open issue (TODO.md) |

Whichever path: **prove it from a device's point of view** before touching
hardware. The bundled script does the whole checklist (DNS, port, chain,
SAN, device handshake, admin access, registration):

```sh
.claude/skills/bring-your-own/scripts/check-server.sh <HOST> [443] [device-id] [admin-name]
```

Every line must say `PASS`. A `FAIL` names the cert or variable to fix; do
not move on with a failing line because the board will fail the same way,
with far less diagnostic output.

## 3. Register the device

The endpoint accepts uplinks from any certificate the CA issued, but a
device only gets *bound* to its certificate (and `REQUIRE_REGISTRATION=true`
only admits it) once registered:

```sh
SUBJECT="$(openssl x509 -in certs/devices/<id>/client.pem -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
curl --cacert certs/ca/ca.pem --cert certs/admins/<you>/admin.pem --key certs/admins/<you>/admin.key \
  -H 'Content-Type: application/json' -X POST "https://<HOST>/devices" \
  -d "{\"device_id\":\"<id>\",\"owner_id\":\"<owner>\",\"subject_dn\":\"$SUBJECT\"}"
```

201 means registered, 409 means it already was. (`./Quickstart onboard
<id> <owner> <HOST>` does cert + register + flash + ping in one go, but it
flashes the POE-ISO build only — use it for that board, the curl above for
everything else.) Optional in the same body: `power_threshold_ma` (the mA
above which the machine counts as "on"), `color`, `line_style`, `marker`.

## 4. Telemetry from their device

Pick one path from `references/board.md`:

| Device | Path |
|---|---|
| Olimex ESP32-POE-ISO, ESP32-POE2, ESP32-C3 devkit | supported: `HOST=<HOST>` in `.env.dev`, then `QS_HARDWARE=flash ./Quickstart` or the `cargo run` line for the board |
| another classic-ESP32 board with a LAN8720/LAN8710 PHY over RMII | a new `edge/src/board/<name>.rs` + a `board-<name>` feature; a few constants, no driver work |
| another ESP32 family chip over WiFi (S3, C6, …) | not wired today; it means plumbing a `chip-<x>` feature through every esp-* dependency — scope it honestly |
| Raspberry Pi or any Linux/macOS machine | the desktop client binary: same certs, same wire, runtime env vars, no toolchain on the device |
| anything else that can do TLS 1.2+ with a client certificate | speak the wire contract directly — ten lines in any language; prove it with curl first |

The curl proof that any device path reduces to (run from the dev machine):

```sh
curl --cacert certs/ca/ca.pem --cert certs/devices/<id>/client.pem --key certs/devices/<id>/client.key \
  -H 'Content-Type: application/json' -d '{"id":"<id>","current":1200}' https://<HOST>/
```

A 200 with `"device_id":"<id>"` in the body is the entire contract; a 403
means the certificate's subject does not match the registration from step 3.

## 5. Verify, then hand over

- `./Quickstart fleet <HOST> 443` — the device row should read `fresh` within one interval (5 s default).
- If the MCP server is configured (`.mcp.json` at the repo root), ask it:
  `supervictor_backend_status` with `endpoint: https://<HOST>` proves reachability and admin access;
  `supervictor_device_status` with the id shows the last uplinks and whether the cert is bound.
- Dashboard: import `certs/admins/<you>/admin.p12` into the browser, trust `certs/ca/ca.pem`, open `https://<HOST>/ui`.

Tell the user, in this order: the hostname and port, where the endpoint's
config and database live, which certificate files they must keep safe
(`ca.key` above all), how to add the next device (steps 1, 3, 4 only), and
how to upgrade (path A: rerun the installer with a newer tag; B: rebuild).

## When it fails

| Symptom | Cause → fix |
|---|---|
| check-server: `SAN does not include HOST` | server cert issued for a different name; reissue `certs server <name> <HOST>` (FORCE=1 to overwrite) and reinstall it |
| check-server: handshake `unknown ca` / `BadSignature` | the endpoint's `TLS_CLIENT_CA` is not the CA that issued this device cert — two CAs exist; use one |
| curl/firmware: 403 `forbidden` on POST / | registered `subject_dn` ≠ certificate subject; re-register with the RFC2253 subject exactly as above |
| firmware: `DNS resolution failed` | the board's DHCP DNS cannot resolve HOST; use a name the LAN resolves, or an IP literal (firmware accepts both) |
| firmware: `TCP connect error` | port 443 closed or filtered on the server; `ss -ltnp` on the box, firewall, security group |
| firmware: TLS handshake fails, curl works | the board bakes `CA_PATH` at compile time — it must be the CA that signed the **server** cert (fleet CA, or the public root for Let's Encrypt); rebuild + reflash after changing `.env.dev` |
| endpoint exits at boot: `incomplete TLS configuration` | all three of `TLS_CERT`, `TLS_KEY`, `TLS_CLIENT_CA` or none; a malformed value fails the boot on purpose |
| endpoint runs, dashboard 403 | the browser presented no admin cert, or its `OU` is not `admin`; reimport the `.p12` |
| device shows `fresh` then `dark` | it posts once and dies: watch its log (`espflash monitor`, `journalctl`, `edge.log`) for the second attempt's error |

Stop and tell the user after the second failure of the same step with what
you saw — a missing firewall rule, a wrong hostname, or a second CA on
another machine is almost always the answer, and they know which.
