"""qs certs — Thin wrapper around gen_certs.sh with certificate verification."""

from __future__ import annotations

import argparse
import subprocess

from quickstart import runner
from quickstart.config import ProjectConfig
from quickstart.preflight import require


def _gen_script(config: ProjectConfig) -> str:
    return str(config.cloud_dir / config.gen_certs_script)


def _certs_dir(config: ProjectConfig):
    return config.repo_root / config.certs_dir_name


def _run_gen(config: ProjectConfig, gen_args: list[str], *, verbose: bool, dry_run: bool) -> int:
    """Run gen_certs.sh with the given arguments. Returns 0 on success."""
    try:
        runner.run(
            [_gen_script(config)] + gen_args,
            cwd=config.cloud_dir, verbose=verbose, dry_run=dry_run,
        )
        return 0
    except subprocess.CalledProcessError as e:
        runner.error(f"gen_certs.sh {' '.join(gen_args)} failed: {e}")
        return 1


def cmd_ca(args: argparse.Namespace, config: ProjectConfig) -> int:
    return _run_gen(config, ["ca"], verbose=args.verbose, dry_run=args.dry_run)


def cmd_device(args: argparse.Namespace, config: ProjectConfig) -> int:
    gen_args = ["device", args.name]
    if args.days:
        gen_args.append(str(args.days))
    return _run_gen(config, gen_args, verbose=args.verbose, dry_run=args.dry_run)


def cmd_server(args: argparse.Namespace, config: ProjectConfig) -> int:
    gen_args = ["server", args.name, args.host_ip]
    if args.days:
        gen_args.append(str(args.days))
    return _run_gen(config, gen_args, verbose=args.verbose, dry_run=args.dry_run)


def cmd_list(args: argparse.Namespace, config: ProjectConfig) -> int:
    return _run_gen(config, ["list"], verbose=args.verbose, dry_run=args.dry_run)


def cmd_verify(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Verify the mTLS certificate chain using OpenSSL."""
    verbose = args.verbose
    dry_run = args.dry_run
    certs = _certs_dir(config)

    ca_pem = str(certs / "ca" / "ca.pem")
    server_pem = str(certs / "servers" / args.server_name / "server.pem")
    client_pem = str(certs / "devices" / args.device_name / "client.pem")

    all_ok = True

    def _check(label: str, cmd: list[str], *, expect_ok: bool = True) -> bool:
        """Run an openssl command. Returns True if check passed."""
        runner.step(label)
        try:
            result = runner.run(cmd, capture=True, verbose=verbose, dry_run=dry_run)
            if dry_run:
                return True
            if expect_ok and "OK" not in result.stdout:
                runner.error(f"  FAIL: {result.stdout.strip()}")
                return False
            runner.success(f"  {result.stdout.strip()}")
            return True
        except subprocess.CalledProcessError as e:
            runner.error(f"  FAIL: {e}")
            return False

    # 1. Verify CA is valid (self-signed)
    if not _check("Verify root CA", ["openssl", "verify", "-CAfile", ca_pem, ca_pem]):
        all_ok = False

    # 2. Verify server cert against CA
    if not _check("Verify server cert against CA", ["openssl", "verify", "-CAfile", ca_pem, server_pem]):
        all_ok = False

    # 3. Verify client cert against CA
    if not _check("Verify client cert against CA", ["openssl", "verify", "-CAfile", ca_pem, client_pem]):
        all_ok = False

    # 4. Inspect server cert SAN
    _check(
        "Server cert SAN",
        ["openssl", "x509", "-in", server_pem, "-noout", "-ext", "subjectAltName"],
        expect_ok=False,
    )

    # 5. Inspect client cert subject DN
    _check(
        "Client cert subject",
        ["openssl", "x509", "-in", client_pem, "-noout", "-subject"],
        expect_ok=False,
    )

    # 6. Verify client cert has clientAuth Extended Key Usage
    runner.step("Client cert Extended Key Usage")
    try:
        result = runner.run(
            ["openssl", "x509", "-in", client_pem, "-noout", "-ext", "extendedKeyUsage"],
            capture=True, verbose=verbose, dry_run=dry_run,
        )
        if not dry_run:
            eku_output = result.stdout.strip()
            if "clientAuth" in eku_output:
                runner.success(f"  {eku_output}")
            else:
                runner.error(f"  FAIL: clientAuth not found in Extended Key Usage: {eku_output}")
                all_ok = False
    except subprocess.CalledProcessError as e:
        runner.error(f"  FAIL: could not read Extended Key Usage: {e}")
        all_ok = False

    # 7. Check expiry on all three
    runner.step("Certificate expiry dates")
    for label, cert in [("CA", ca_pem), ("Server", server_pem), ("Client", client_pem)]:
        try:
            result = runner.run(
                ["openssl", "x509", "-in", cert, "-noout", "-enddate"],
                capture=True, verbose=verbose, dry_run=dry_run,
            )
            if not dry_run:
                expiry = result.stdout.strip().removeprefix("notAfter=")
                runner.success(f"  {label}: expires {expiry}")
        except subprocess.CalledProcessError as e:
            runner.error(f"  {label} expiry check failed: {e}")
            all_ok = False

    if all_ok:
        runner.success("\nAll checks passed.")
        return 0
    else:
        runner.error("\nSome checks failed.")
        return 1


def cmd_handshake(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Simulate an mTLS handshake against a running server."""
    verbose = args.verbose
    dry_run = args.dry_run
    certs = _certs_dir(config)

    ca_pem = str(certs / "ca" / "ca.pem")
    client_pem = str(certs / "devices" / args.device_name / "client.pem")
    client_key = str(certs / "devices" / args.device_name / "client.key")
    target = f"{args.host}:{args.port}"

    all_ok = True

    # 1. Full mTLS handshake (client cert presented)
    runner.step(f"mTLS handshake to {target}")
    mtls_cmd = [
        "openssl", "s_client", "-connect", target,
        "-cert", client_pem,
        "-key", client_key,
        "-CAfile", ca_pem,
    ]
    if args.tls_version:
        mtls_cmd.append(f"-{args.tls_version}")
    try:
        result = runner.run(
            mtls_cmd,
            capture=True, verbose=verbose, dry_run=dry_run,
        )
        if not dry_run:
            if "Verify return code: 0 (ok)" in result.stdout:
                runner.success("  Handshake OK — Verify return code: 0 (ok)")
            else:
                # Extract the verify return code line for diagnostics
                for line in result.stdout.splitlines():
                    if "Verify return code:" in line:
                        runner.error(f"  FAIL: {line.strip()}")
                        break
                else:
                    runner.error("  FAIL: could not find verify return code in output")
                all_ok = False
    except subprocess.CalledProcessError as e:
        runner.error(f"  FAIL: mTLS handshake failed: {e}")
        all_ok = False

    # 2. Without client cert — should be rejected if mTLS is enforced
    if args.test_no_client:
        runner.step(f"Connecting without client cert to {target} (should fail if mTLS enforced)")
        no_client_cmd = [
            "openssl", "s_client", "-connect", target,
            "-CAfile", ca_pem,
        ]
        if args.tls_version:
            no_client_cmd.append(f"-{args.tls_version}")
        try:
            result = runner.run(
                no_client_cmd,
                capture=True, check=False, verbose=verbose, dry_run=dry_run,
            )
            if not dry_run:
                if "Verify return code: 0 (ok)" in result.stdout:
                    runner.error("  WARN: server accepted connection without client cert — mTLS may not be enforced")
                else:
                    runner.success("  Server rejected connection without client cert (mTLS enforced)")
        except subprocess.CalledProcessError:
            runner.success("  Server rejected connection without client cert (mTLS enforced)")

    if all_ok:
        runner.success("\nHandshake checks passed.")
        return 0
    else:
        runner.error("\nHandshake checks failed.")
        return 1


def run_certs(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Dispatch to the appropriate certs subcommand."""
    require(["openssl"])
    dispatch = {
        "ca": cmd_ca,
        "device": cmd_device,
        "server": cmd_server,
        "list": cmd_list,
        "verify": cmd_verify,
        "handshake": cmd_handshake,
    }
    return dispatch[args.certs_command](args, config)


def register_subparser(sub: argparse._SubParsersAction) -> None:
    """Register the certs command and its subcommands on the main CLI parser."""
    certs_p = sub.add_parser("certs", help="Generate and verify mTLS certificates")
    certs_sub = certs_p.add_subparsers(dest="certs_command", required=True)

    certs_sub.add_parser("ca", help="Initialize the root CA")

    device_p = certs_sub.add_parser("device", help="Issue a device client certificate")
    device_p.add_argument("name", help="Device name (e.g. esp32)")
    device_p.add_argument("--days", type=int, default=None, help="Validity in days")

    server_p = certs_sub.add_parser("server", help="Issue a server/TLS certificate")
    server_p.add_argument("name", help="Server name (e.g. caddy)")
    server_p.add_argument("--host-ip", default="127.0.0.1", help="SAN IP (default: 127.0.0.1)")
    server_p.add_argument("--days", type=int, default=None, help="Validity in days")

    certs_sub.add_parser("list", help="List all issued certificates")

    verify_p = certs_sub.add_parser("verify", help="Verify the mTLS certificate chain")
    verify_p.add_argument("--device-name", default="esp32", help="Device cert to verify (default: esp32)")
    verify_p.add_argument("--server-name", default="caddy", help="Server cert to verify (default: caddy)")

    hs_p = certs_sub.add_parser("handshake", help="Simulate mTLS handshake against a running server")
    hs_p.add_argument("--host", default="localhost", help="Server host (default: localhost)")
    hs_p.add_argument("--port", default="443", help="Server port (default: 443)")
    hs_p.add_argument("--device-name", default="esp32", help="Device cert to use (default: esp32)")
    hs_p.add_argument("--server-name", default="caddy", help="Server cert name (default: caddy)")
    hs_p.add_argument("--tls-version", default="tls1_3", help="TLS version flag (default: tls1_3)")
    hs_p.add_argument("--test-no-client", action="store_true", help="Also test without client cert to confirm mTLS is enforced")
