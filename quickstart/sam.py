"""SAM lifecycle management: build, start local, wait, stop, deploy."""

from __future__ import annotations

import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

from quickstart import runner
from quickstart.config import ProjectConfig


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

    def start(self) -> None:
        """Start sam local start-api in background."""
        runner.step(f"Starting sam local on port {self._config.sam_local_port}")
        self._proc = runner.start_background(
            [
                "sam",
                "local",
                "start-api",
                "--port",
                str(self._config.sam_local_port),
                "--skip-pull-image",
            ],
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
        """Terminate sam local process."""
        if self._proc is None or self._proc.poll() is not None:
            return
        print("  Stopping sam local...")
        self._proc.terminate()
        try:
            self._proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait()

    def deploy(self, config_env: str, *, force_upload: bool = False) -> bool:
        """Run sam deploy --config-env <env>. Returns True if changes deployed."""
        log_path = self._config.log_dir / f"sam_deploy_{config_env}.log"
        runner.step(f"Deploying to {config_env} stack")
        cmd = ["sam", "deploy", "--config-env", config_env]
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
