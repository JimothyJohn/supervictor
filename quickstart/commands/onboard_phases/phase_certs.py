"""Phase: generate CA and device certificates if missing."""

from __future__ import annotations

import subprocess

from quickstart import runner
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus


def _extract_subject_dn(cert_path) -> str:
    """Read the subject DN from a PEM certificate using openssl."""
    result = runner.run(
        ["openssl", "x509", "-in", str(cert_path), "-noout", "-subject"],
        capture=True,
    )
    # Output format: "subject= /CN=..."  or  "subject=CN = ..."
    return result.stdout.strip().removeprefix("subject=").strip()


def _upload_truststore(ctx: OnboardContext, ca_pem) -> None:
    """Upload CA cert to S3 as the API Gateway mTLS truststore."""
    runner.run(
        ["aws", "s3", "cp", str(ca_pem), "s3://supervictor/truststore.pem"],
        verbose=ctx.verbose,
    )


def run(ctx: OnboardContext) -> PhaseResult:
    """Generate CA and device certs if missing. Sets ctx.certs_dir and ctx.subject_dn."""
    cloud_dir = ctx.config.cloud_dir
    certs_dir = ctx.config.repo_root / ctx.config.certs_dir_name
    gen_script = str(cloud_dir / ctx.config.gen_certs_script)

    ca_pem = certs_dir / "ca" / "ca.pem"
    device_dir = certs_dir / "devices" / ctx.device_name
    client_pem = device_dir / "client.pem"

    if ca_pem.exists() and client_pem.exists():
        ctx.certs_dir = certs_dir
        ctx.subject_dn = _extract_subject_dn(client_pem)
        if ctx.mode == "aws" and not ctx.dry_run:
            _upload_truststore(ctx, ca_pem)
        return PhaseResult(PhaseStatus.SKIPPED, "All certs already present")

    try:
        if not ca_pem.exists():
            runner.run(
                [gen_script, "ca"],
                cwd=cloud_dir,
                verbose=ctx.verbose,
                dry_run=ctx.dry_run,
            )

        if not client_pem.exists():
            runner.run(
                [gen_script, "device", ctx.device_name],
                cwd=cloud_dir,
                verbose=ctx.verbose,
                dry_run=ctx.dry_run,
            )
    except subprocess.CalledProcessError as e:
        return PhaseResult(PhaseStatus.FAILED, f"Cert generation failed: {e}")

    ctx.certs_dir = certs_dir
    if not ctx.dry_run:
        ctx.subject_dn = _extract_subject_dn(client_pem)
        if ctx.mode == "aws":
            _upload_truststore(ctx, ca_pem)
    return PhaseResult(PhaseStatus.PASSED)
