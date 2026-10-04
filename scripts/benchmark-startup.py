#!/usr/bin/env python3
"""Linux warm-process benchmark using isolated synthetic specs/config.

Build default/all-feature stripped release binaries with identical Rust/profile
settings. Timing includes process startup, cache loading, fingerprint checks,
command parsing and dry-run preparation. GNU time measures direct-child RSS
(KiB), avoiding Python's pre-exec high-water RSS floor; memory and timing runs
are separate. This is not a cold-filesystem or cross-platform benchmark.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import time


def sample(command, env):
    """Time one successful process; retain stderr only to diagnose failures."""
    with tempfile.TemporaryFile() as errors:
        start = time.perf_counter_ns()
        child = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=errors)
        _, status, _ = os.wait4(child.pid, 0)
        child.returncode = os.waitstatus_to_exitcode(status)
        elapsed = (time.perf_counter_ns() - start) / 1_000_000
        if child.returncode:
            errors.seek(0)
            raise RuntimeError(errors.read().decode(errors="replace"))
        return elapsed


def document(count):
    result = {"openapi": "3.0.3", "info": {"title": "Benchmark", "version": "1"},
              "servers": [{"url": "http://127.0.0.1:9"}], "paths": {}}
    for i in range(count):
        result["paths"][f"/items/{i}"] = {"get": {
            "operationId": f"get_item_{i}", "tags": ["items"],
            "summary": f"Get item {i}", "responses": {"200": {"description": "OK"}}}}
    return result


def measure_case(binary, count, index, env, root, samples, time_binary):
    command = [binary, "api", "bench", "--dry-run", "items", f"get-item-{index}"]
    for _ in range(5):
        sample(command, env)
    timings = [sample(command, env) for _ in range(samples)]
    memory_file = root / "rss.txt"
    memory = []
    for _ in range(7):
        sample([time_binary, "-f", "%M", "-o", str(memory_file), *command], env)
        memory.append(int(memory_file.read_text().strip()))
    return {"operations": count, "selected_index": index,
            "median_ms": statistics.median(timings),
            "median_rss_kib": statistics.median(memory),
            "raw_ms": timings, "raw_rss_kib": memory}


def benchmark(binary, samples, time_binary):
    result = {"binary_bytes": Path(binary).stat().st_size,
              "platform": platform.platform(), "samples": samples, "cases": []}
    with tempfile.TemporaryDirectory(prefix="aperture-startup-") as directory:
        root = Path(directory)
        env = os.environ.copy()
        env["APERTURE_CONFIG_DIR"] = str(root / "config")
        for count in (10, 1000, 5000):
            spec = root / "source.json"
            spec.write_text(json.dumps(document(count)))
            sample([binary, "config", "add", "bench", str(spec), "--force"], env)
            for index in (0, count - 1):
                result["cases"].append(measure_case(
                    binary, count, index, env, root, samples, time_binary))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=35)
    args = parser.parse_args()
    time_binary = shutil.which("time")
    if platform.system() != "Linux" or args.samples < 1 or not time_binary:
        parser.error("requires Linux, GNU time, and a positive sample count")
    result = benchmark(str(args.binary.resolve()), args.samples, time_binary)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k != "cases"}))
    for case in result["cases"]:
        print({k: v for k, v in case.items() if not k.startswith("raw_")})


if __name__ == "__main__":
    main()
