# The endpoint on your own server

One binary, configured entirely by environment variables, storing to one
SQLite file. It either terminates mTLS itself (paths A, B) or sits on plain
HTTP behind something that does (C, D, E). Prefer A or B: fewer moving
parts, and identity comes from the verified peer certificate rather than a
header another process must be trusted to set.

## Configuration (all paths)

From `supervictor/endpoint/src/config.rs`; a malformed value fails the boot
rather than silently disabling a check.

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8000` | listen port; boards dial 443, so set `443` for anything boards talk to |
| `TLS_CERT`, `TLS_KEY`, `TLS_CLIENT_CA` | unset | all three → in-binary mTLS with mandatory client certs chaining to `TLS_CLIENT_CA`; none → plain HTTP, identity from proxy headers |
| `STORE_BACKEND` | `sqlite` | `sqlite` or `dynamo` (needs the `dynamo` build feature) |
| `SQLITE_DB_PATH` | `:memory:` | set a real path or every restart forgets the fleet |
| `REQUIRE_REGISTRATION` | `false` | `true` turns away certificates that are not registered, active and bound; flip it once every device is registered with its `subject_dn` |
| `POWER_THRESHOLD_MA` | `1000` | fleet default for "powered on": the clamp's ~1 A ADC floor (per-device override via `PUT /devices/{id}/threshold`) |
| `WATCHDOG_INTERVAL_SECS` | `300` | logs dark-device transitions; `0` disables |
| `UPLINK_TTL_DAYS` | `30` | DynamoDB row expiry; SQLite keeps everything |
| `LOG_LEVEL` | `info` | tracing filter; logs are JSON |

Certificates the endpoint needs: a **server** cert + key whose SAN is the
HOST devices dial, and the **client CA** (`certs/ca/ca.pem`) that issued the
device and admin certs. The CA's private key never goes to the server.

## A. systemd Linux box (aarch64 or x86_64)

Release tags ship static musl binaries with checksums; the installer
verifies the sha256, installs the binary and a hardened unit, and writes
`/etc/supervictor/env` once (never overwritten on upgrade).

On the box, as root:

```sh
curl -fsSL https://raw.githubusercontent.com/JimothyJohn/supervictor/master/scripts/install-endpoint.sh \
  | sudo bash -s -- v0.1.0        # pick the latest v* tag: gh release list -R JimothyJohn/supervictor
```

From the dev machine, copy the three PEMs (the unit loads them as systemd
credentials, so only root reads the key):

```sh
scp certs/servers/<name>/server.pem certs/servers/<name>/server.key certs/ca/ca.pem <box>:/tmp/
ssh <box> 'sudo install -m 0600 /tmp/server.pem /tmp/server.key /tmp/ca.pem /etc/supervictor/ && sudo rm /tmp/server.pem /tmp/server.key /tmp/ca.pem && sudo systemctl restart supervictor-endpoint && sudo systemctl --no-pager status supervictor-endpoint'
```

What the unit fixes for you: `PORT=443` via `CAP_NET_BIND_SERVICE` (no root
at runtime), `DynamicUser`, `ProtectSystem=strict`, a syscall filter, the
database in `/var/lib/supervictor/supervictor.db`. Edit
`/etc/supervictor/env` for everything else, then `systemctl restart`.
Logs: `journalctl -u supervictor-endpoint -f`. Upgrade: rerun the installer
with a newer tag. Air-gapped: `install-endpoint.sh --local <tarball>`.

No release for their architecture (armv7, riscv64)? Build on the box or
cross-compile (path B) and use `--local` with a tarball containing the
binary named `supervictor-endpoint`, or skip the installer and copy the unit
from `supervictor/endpoint/deploy/supervictor-endpoint.service` by hand.

Open the port: whatever firewall or cloud security group sits in front must
allow TCP 443 inbound from where the boards are (LAN, or the world for a
VPS — mTLS is the gate, there is no unauthenticated route besides `/health`
and nothing behind it answers without a certificate).

## B. Bare binary (macOS, WSL, a dev box, no systemd)

```sh
cargo build -p supervictor-endpoint --release          # the host target; needs Rust only
# cross for a Linux box from macOS: cargo build -p supervictor-endpoint --release --target aarch64-unknown-linux-musl
#   (needs `rustup target add` and a musl linker; or build on the box itself)
PORT=443 STORE_BACKEND=sqlite SQLITE_DB_PATH=/var/lib/supervictor/supervictor.db \
TLS_CERT=/etc/supervictor/server.pem TLS_KEY=/etc/supervictor/server.key TLS_CLIENT_CA=/etc/supervictor/ca.pem \
  ./target/release/supervictor-endpoint
```

Port 443 needs root or `CAP_NET_BIND_SERVICE` (`setcap
cap_net_bind_service=+ep <binary>` on Linux; on macOS run as root or put
a pf redirect from 443 to a high port). Keep it alive with what the
platform has: a launchd plist on macOS, `nohup`/`screen` for a bench,
a systemd user unit on WSL. Write the start command and the port into the
project's notes so the next person can restart it.

The local Quickstart endpoint is exactly this path on port 8443 with the
`localhost` server cert — fine for simulators and the desktop client, never
for boards (port and name are both wrong for them).

## C. Container + Caddy (Docker Compose)

`supervictor/endpoint/docker-compose.yml` runs the endpoint on plain HTTP
(`:8000`, SQLite in the container) behind Caddy on `:443` with
`client_auth mode require_and_verify` against `certs/ca/ca.pem`. Caddy
forwards the verified subject as `X-SSL-Client-Subject-DN`. It expects the
server cert at `certs/servers/caddy/`:

```sh
./Quickstart certs server caddy <HOST>
cd supervictor/endpoint && docker compose up -d --build
```

Mount a volume for the database and set `SQLITE_DB_PATH` to it, or the
fleet vanishes with the container. The Dockerfile builds from source
(multi-minute first build).

## D. Behind their own reverse proxy

The endpoint on plain HTTP (no `TLS_*`), the proxy doing mTLS. The contract
the proxy must honour (`supervictor/endpoint/src/middleware.rs`):

- verify the client certificate against `certs/ca/ca.pem` (require, not
  optional);
- set `X-SSL-Client-Subject-DN` to the verified subject (RFC 2253 form
  works; the endpoint compares component-wise);
- **strip** any `X-SSL-Client-Subject-DN` and `x-amzn-request-context` the
  client sent — the endpoint cannot tell a forged header from a forwarded
  one in this mode;
- speak HTTP/1.1 to the endpoint; expose `/health` without a certificate if
  a load balancer needs it, everything else only with one.

nginx: `ssl_verify_client on; ssl_client_certificate ca.pem;
proxy_set_header X-SSL-Client-Subject-DN $ssl_client_s_dn;` (nginx renders
the DN in RFC 2253 order). Caddy: the repo's `Caddyfile` is the worked
example (`header_up X-SSL-Client-Subject-DN {http.request.tls.client.subject}`).
Traefik: `requireAndVerifyClientCert` in the TLS options verifies the
certificate, but its `passTLSClientCert` middleware emits an encoded info
header, not a DN — it needs a step that builds `CN=…,OU=…` from those
fields, so prefer nginx or Caddy unless they already have that. Verify with the bundled
`check-server.sh`: the admin step passing proves the header reached the
endpoint; a device cert being treated as admin proves the proxy is not
stripping inbound headers.

## E. An AWS EC2 box

Path A on an EC2 instance. `supervictor/endpoint/template-staging.yaml` is
the reference: one t4g.micro whose bootstrap runs
`scripts/install-endpoint.sh` from a release tag, an Elastic IP, a Route53
record and a 443-only security group; `scripts/deploy-staging.sh` deploys
it and `scripts/staging-letsencrypt.sh` swaps in a public server
certificate. The serverless path (API Gateway + Lambda) was retired
2026-10-27: it could not carry the client certificate's identity into the
binary, and nothing here needs anything AWS-specific.

## Public server certificate (Let's Encrypt)

Only the server certificate changes; the client CA stays the fleet CA.
Boards verify the server against `CA_PATH`, so drop the public root into
`certs/ca/` (ISRG Root X1 for Let's Encrypt — the RSA chain, which is why
`scripts/staging-letsencrypt.sh` requests an RSA key) and set
`CA_PATH=ca/<that-root>.pem` in `.env.dev` before building firmware. On a
systemd box the certbot deploy hook copies `fullchain.pem`/`privkey.pem`
over `/etc/supervictor/server.{pem,key}` and restarts the unit; the script
is AWS/Route53-specific but the shape (certonly → copy → restart → timer)
is portable.

## Backups and secrets

- `certs/ca/ca.key` is the fleet. Back it up offline; a lost key means
  reissuing every certificate. Never copy it to the server.
- The SQLite file is the only state (`SQLITE_DB_PATH`); `sqlite3 <db>
  ".backup <file>"` while running is safe.
- Rotate a server certificate by reissuing with `FORCE=1` and restarting;
  devices keep working as long as the signer (`CA_PATH` on the board) is
  the same.
