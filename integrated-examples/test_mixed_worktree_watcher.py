#!/usr/bin/env python3
"""Verify mixed worktree discovery against an isolated daemon and real containers.

Build coast/coastd first (COAST_SKIP_UI_BUILD=1 cargo build -p coast-cli
-p coast-daemon --bin coast --bin coastd), then run this script. Docker and
socat must be available. No existing Coast state or instance is modified.
"""

import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile
import time
from urllib.request import urlopen


REPO = Path(__file__).resolve().parents[1]


def main():
    coast = Path(os.environ.get("COAST_TEST_BIN", REPO / "target/debug/coast")).resolve()
    coastd = Path(os.environ.get("COAST_TEST_DAEMON", REPO / "target/debug/coastd")).resolve()
    assert coast.is_file() and coastd.is_file(), "Build coast and coastd first"
    endpoint = subprocess.check_output(
        ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"], text=True
    ).strip()
    with tempfile.TemporaryDirectory(prefix="coast-watcher-") as temporary:
        fixture = Path(temporary).resolve()
        root = fixture / "repo"
        root.mkdir()
        home = fixture / "home"
        home.mkdir()
        state = home / ".coast"
        external = fixture / "external"
        project = f"watcher-repro-{os.getpid()}"
        env = dict(os.environ, HOME=str(home), COAST_HOME=str(state),
                   DOCKER_HOST=endpoint, DOCKER_CONFIG=str(Path.home() / ".docker"),
                   GIT_CONFIG_GLOBAL=os.devnull, COAST_API_PORT="0", COAST_DNS_PORT="0", RUST_LOG="info")

        def run(*args, timeout=240):
            result = subprocess.run(args, cwd=root, env=env, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                    timeout=timeout)
            if result.returncode:
                raise RuntimeError(f"{args}:\n{result.stdout}")
            return result.stdout

        def git(*args):
            return run("git", "-c", "user.name=Coast test", "-c",
                       "user.email=test@example.invalid", "-c", "commit.gpgsign=false",
                       "-c", "core.hooksPath=/dev/null", *args)

        (root / "Coastfile").write_text(f'''[coast]
name = "{project}"
worktree_dir = [".worktrees", {json.dumps(str(external))}]

[coast.setup]
packages = ["python3"]

[services.web]
command = "python3 -m http.server 41000 --bind 0.0.0.0 --directory /workspace"
port = 41000

[ports]
web = 41000
''')
        (root / "marker.txt").write_text("main")
        git("init", "-b", "main")
        git("add", "Coastfile", "marker.txt")
        git("commit", "-qm", "fixture")
        for name in ("first", "second"):
            git("switch", "-c", name)
            (root / "marker.txt").write_text(name)
            git("commit", "-qam", name)
            git("switch", "main")
            git("worktree", "add", str(external / name), name)

        daemon = None
        instances = []
        log_path = fixture / "daemon.log"
        log = log_path.open("a")

        def start_daemon():
            process = subprocess.Popen([str(coastd), "--foreground"], cwd=root,
                                       env=env, stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 30
            while not (state / "coastd.sock").exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(log_path.read_text())
                time.sleep(0.2)
            return process

        def stop_daemon(process):
            process.terminate()
            process.wait(timeout=15)
            (state / "coastd.sock").unlink(missing_ok=True)

        def check_preview(name, port):
            with urlopen(f"http://127.0.0.1:{port}/marker.txt", timeout=5) as response:
                actual = response.read().decode()
                assert actual == name, f"{name} preview switched source to {actual!r}"
            with sqlite3.connect(f"file:{state / 'state.db'}?mode=ro", uri=True) as db:
                row = db.execute("SELECT worktree_name FROM instances WHERE project=? AND name=?",
                                 (project, name)).fetchone()
                assert row == (name,), f"{name} assignment changed: {row}"

        try:
            daemon = start_daemon()
            run(str(coast), "build", timeout=600)
            ports = {}
            for name in ("first", "second"):
                instances.append(name)
                run(str(coast), "run", name, "-w", name, timeout=600)
                output = run(str(coast), "ports", name)
                ports[name] = int(re.search(r"web\s+41000\s+(\d+)", output).group(1))
                deadline = time.monotonic() + 30
                while True:
                    try:
                        check_preview(name, ports[name])
                        break
                    except OSError:
                        if time.monotonic() > deadline:
                            raise
                        time.sleep(0.5)
            print("PASS: both external previews serve their own marker", flush=True)
            git("worktree", "add", "-b", "third", ".worktrees/third")
            for _ in range(12):
                time.sleep(1)
                for name, port in ports.items():
                    check_preview(name, port)
            print("PASS: adding an internal worktree preserves both external previews", flush=True)
            stop_daemon(daemon)
            daemon = start_daemon()
            for _ in range(8):
                time.sleep(1)
                for name, port in ports.items():
                    check_preview(name, port)
            assert "auto-unassigning" not in log_path.read_text()
            print("PASS: daemon restart preserves both external previews", flush=True)
        except BaseException:
            print(log_path.read_text()[-12000:], flush=True)
            raise
        finally:
            for name in reversed(instances):
                try:
                    run(str(coast), "rm", name)
                except Exception as error:
                    print(f"Cleanup failed for {project}/{name}: {error}", flush=True)
            if daemon is not None and daemon.poll() is None:
                stop_daemon(daemon)
            log.close()


if __name__ == "__main__":
    main()
