use crate::runner::{RunOptions, Runner};

use super::{OnboardContext, PhaseResult};

fn extract_subject_dn(cert_path: &std::path::Path, r: &dyn Runner) -> Result<String, String> {
    let cert_str = cert_path.to_string_lossy().to_string();
    let result = r
        .run(
            &["openssl", "x509", "-in", &cert_str, "-noout", "-subject"],
            &RunOptions {
                capture: true,
                ..Default::default()
            },
        )
        .map_err(|e| format!("failed to read subject DN: {}", e))?;

    Ok(result
        .stdout
        .trim()
        .strip_prefix("subject=")
        .unwrap_or(result.stdout.trim())
        .trim()
        .to_string())
}

fn upload_truststore(ctx: &OnboardContext, ca_pem: &std::path::Path) {
    let ca_str = ca_pem.to_string_lossy().to_string();
    let _ = ctx.runner.run(
        &[
            "aws",
            "s3",
            "cp",
            &ca_str,
            "s3://supervictor/truststore.pem",
        ],
        &RunOptions {
            verbose: ctx.verbose,
            ..Default::default()
        },
    );
}

/// Generate CA and device certificates if they do not already exist.
pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    let script_cwd = ctx.config.repo_root.clone();
    let certs_dir = ctx.config.certs_dir();
    let gen_script = ctx.config.gen_certs_script_path();
    let gen_script_str = gen_script.to_string_lossy().to_string();

    let ca_pem = certs_dir.join("ca/ca.pem");
    let client_pem = certs_dir
        .join("devices")
        .join(&ctx.device_name)
        .join("client.pem");

    // Check if certs already exist
    if ca_pem.exists() && client_pem.exists() {
        ctx.certs_dir = Some(certs_dir);
        match extract_subject_dn(&client_pem, ctx.runner) {
            Ok(dn) => ctx.subject_dn = Some(dn),
            Err(e) => return PhaseResult::failed(e),
        }
        if ctx.mode == "aws" && !ctx.dry_run {
            upload_truststore(ctx, &ca_pem);
        }
        return PhaseResult::skipped("All certs already present");
    }

    let opts = RunOptions {
        cwd: Some(script_cwd),
        verbose: ctx.verbose,
        dry_run: ctx.dry_run,
        ..Default::default()
    };

    if !ca_pem.exists() {
        if let Err(e) = ctx.runner.run(&[&gen_script_str, "ca"], &opts) {
            return PhaseResult::failed(format!("CA generation failed: {}", e));
        }
    }

    if !client_pem.exists() {
        if let Err(e) = ctx
            .runner
            .run(&[&gen_script_str, "device", &ctx.device_name], &opts)
        {
            return PhaseResult::failed(format!("Device cert generation failed: {}", e));
        }
    }

    ctx.certs_dir = Some(certs_dir);
    if !ctx.dry_run {
        match extract_subject_dn(&client_pem, ctx.runner) {
            Ok(dn) => ctx.subject_dn = Some(dn),
            Err(e) => return PhaseResult::failed(e),
        }
        if ctx.mode == "aws" {
            upload_truststore(ctx, &ca_pem);
        }
    }

    PhaseResult::passed()
}
