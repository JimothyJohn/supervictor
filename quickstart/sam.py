"""SAM lifecycle management: build, start local, wait, stop, deploy."""

from __future__ import annotations

import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

from quickstart import runner
from quickstart.config import ProjectConfig

# Lambda env var overrides for sam local (no DynamoDB available locally)
_LOCAL_ENV_OVERRIDES = {
    "HelloWorldFunction": {
        "STORE_BACKEND": "sqlite",
    }
}


class SamLocal:
    """Context manager for sam local start-api lifecycle."""

    def __init__(
        self,
        config: ProjectConfig,
        *,
        env: dict[str, str] | None = None,
        verbose: bool = False,
        dry_run: bool = False,
    ):
        self._config = config
        self._env = env
        self._verbose = verbose
        self._dry_run = dry_run
        self._proc: subprocess.Popen | None = None

    @property
    def url(self) -> str:
        return f"http://localhost:{self._config.sam_local_port}"

    def build(self, *, no_cache: bool = False) -> None:
        """Export runtime deps and run sam build."""
        log_dir = self._config.log_dir

        runner.step("Exporting runtime dependencies")
        runner.run(
            ["uv", "export", "--no-dev", "--no-hashes", "-o", "requirements.txt"],
            cwd=self._config.cloud_dir / "uplink",
            env=self._env,
            verbose=self._verbose,
            dry_run=self._dry_run,
            log_to=log_dir / "uv_export.log",
        )

        runner.step("Building SAM artifacts")
        cmd = ["sam", "build", "--skip-pull-image"]
        if no_cache:
            cmd.append("--no-cached")
        runner.run(
            cmd,
            cwd=self._config.cloud_dir,
            env=self._env,
            verbose=self._verbose,
            dry_run=self._dry_run,
            log_to=log_dir / "sam_build.log",
        )
        runner.success("SAM build complete")

    def _write_env_overrides(self) -> Path:
        """Write Lambda env var overrides to a temp JSON file for --env-vars."""
        import json
        import tempfile

        env_file = Path(tempfile.mktemp(suffix=".json", prefix="sam_env_"))
        env_file.write_text(json.dumps(_LOCAL_ENV_OVERRIDES))
        return env_file

    def start(self, *, extra_args: list[str] | None = None) -> None:
        """Start sam local start-api in background."""
        runner.step(f"Starting sam local on port {self._config.sam_local_port}")
        self._env_file = self._write_env_overrides()
        cmd = [
            "sam",
            "local",
            "start-api",
            "--port",
            str(self._config.sam_local_port),
            "--skip-pull-image",
            "--env-vars",
            str(self._env_file),
        ]
        if extra_args:
            cmd.extend(extra_args)
        self._proc = runner.start_background(
            cmd,
            cwd=self._config.cloud_dir,
            env=self._env,
            log_file=self._config.sam_log_file,
            verbose=self._verbose,
            dry_run=self._dry_run,
        )

    def wait_ready(self) -> None:
        """Poll until sam local's HTTP server is up (any HTTP response)."""
        if self._dry_run:
            print("  [dry-run] wait for sam local ready")
            return

        # Hit a non-existent path — SAM responds immediately with 403/404
        # without invoking a Lambda, so we don't pay the cold-start penalty here.
        probe_url = f"{self.url}/_qs_health_probe"
        print(f"  Waiting for sam local at {self.url} ...")
        deadline = time.monotonic() + self._config.sam_ready_timeout

        while time.monotonic() < deadline:
            try:
                resp = urllib.request.urlopen(probe_url, timeout=2)
                runner.success(f"  sam local ready (HTTP {resp.status}).")
                return
            except urllib.error.HTTPError as e:
                # 4xx still means the HTTP server is up
                runner.success(f"  sam local ready (HTTP {e.code}).")
                return
            except (urllib.error.URLError, OSError):
                time.sleep(1)

        raise TimeoutError(
            f"sam local did not start within {self._config.sam_ready_timeout}s. "
            f"Check logs: {self._config.sam_log_file}"
        )

    def stop(self) -> None:
        """Terminate sam local process and clean up temp files."""
        if self._proc is not None and self._proc.poll() is None:
            print("  Stopping sam local...")
            self._proc.terminate()
            try:
                self._proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._proc.kill()
                self._proc.wait()
        if hasattr(self, "_env_file") and self._env_file.exists():
            self._env_file.unlink()

    def _read_stack_name(self, config_env: str) -> str:
        """Read stack_name from env vars, falling back to samconfig.toml."""
        # CLI flags from .env files take precedence (see deploy())
        env = self._env or {}
        if env.get("SAM_STACK_NAME"):
            return env["SAM_STACK_NAME"]

        import tomllib

        samconfig = self._config.cloud_dir / "samconfig.toml"
        with open(samconfig, "rb") as f:
            data = tomllib.load(f)

        env_section = data.get(config_env, {})
        # Try deploy.parameters first, then global.parameters
        for path in (("deploy", "parameters"), ("global", "parameters")):
            section = env_section
            for key in path:
                section = section.get(key, {})
            name = section.get("stack_name")
            if name:
                return name.strip('"')

        raise RuntimeError(
            f"No stack_name found in samconfig.toml for config-env '{config_env}'"
        )

    def stack_endpoint(self, config_env: str) -> str:
        """Query CloudFormation for the deployed API endpoint URL."""
        if self._dry_run:
            return f"https://DRY-RUN.execute-api.us-east-1.amazonaws.com/{config_env}"

        stack_name = self._read_stack_name(config_env)
        result = runner.run(
            [
                "aws", "cloudformation", "describe-stacks",
                "--stack-name", stack_name,
                "--query",
                "Stacks[0].Outputs[?OutputKey=='SupervictorApiEndpoint'].OutputValue",
                "--output", "text",
            ],
            env=self._env,
            verbose=self._verbose,
            dry_run=self._dry_run,
            capture=True,
        )
        url = result.stdout.strip().rstrip("/")
        if not url:
            raise RuntimeError(
                f"No SupervictorApiEndpoint output found for stack '{stack_name}'"
            )
        return url

    def deploy(self, config_env: str, *, force_upload: bool = False) -> bool:
        """Run sam deploy --config-env <env>. Returns True if changes deployed.

        If the process env (self._env) contains SAM_* variables, they are passed
        as CLI flags so that .env.dev / .env.staging drives the deploy instead of
        hardcoded values in samconfig.toml.
        """
        log_path = self._config.log_dir / f"sam_deploy_{config_env}.log"
        runner.step(f"Deploying to {config_env} stack")
        cmd = ["sam", "deploy", "--config-env", config_env]

        # Override samconfig values from env vars when present
        env = self._env or {}
        if env.get("SAM_STACK_NAME"):
            cmd.extend(["--stack-name", env["SAM_STACK_NAME"]])
        if env.get("SAM_REGION"):
            cmd.extend(["--region", env["SAM_REGION"]])
        if env.get("SAM_S3_PREFIX"):
            cmd.extend(["--s3-prefix", env["SAM_S3_PREFIX"]])

        # Build --parameter-overrides from SAM_* env vars
        param_parts: list[str] = []
        param_map = {
            "SAM_ENVIRONMENT": "Environment",
            "SAM_APP_NAME": "AppName",
            "SAM_STACK_NAME": "StackName",
            "SAM_TRUSTSTORE_URI": "TruststoreUri",
        }
        for env_key, cfn_param in param_map.items():
            val = env.get(env_key)
            if val:
                param_parts.append(f"{cfn_param}={val}")
        if param_parts:
            cmd.extend(["--parameter-overrides", " ".join(param_parts)])

        if force_upload:
            cmd.append("--force-upload")
        result = runner.run(
            cmd,
            cwd=self._config.cloud_dir,
            env=self._env,
            verbose=self._verbose,
            dry_run=self._dry_run,
            check=False,
            log_to=log_path,
        )
        if result.returncode != 0:
            if "No changes to deploy" in (result.stderr or "") + (result.stdout or ""):
                runner.success("Stack is already up to date.")
                return False
            else:
                runner.error(f"sam deploy failed (see {log_path})")
                raise subprocess.CalledProcessError(result.returncode, result.args)
        runner.success(f"Deployed to {config_env}")
        return True

    def __enter__(self) -> SamLocal:
        self.start()
        self.wait_ready()
        return self

    def __exit__(self, *exc: object) -> None:
        self.stop()
