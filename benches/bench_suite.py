"""Sequential, reproducible parse-and-extract comparison on synthetic product pages."""
import argparse
import gc
import hashlib
import importlib.metadata
import json
import platform
import random
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

from oxide_parse import _core

from bench_python import build_cases, make_page


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", nargs="+", type=int, default=[100, 1000, 10000])
    parser.add_argument("--repeat", type=int, default=7)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.repeat < 1 or any(size < 1 for size in args.sizes):
        parser.error("sizes and repeat must be positive")

    native = Path(_core.__file__)
    root = Path(__file__).resolve().parents[1]
    source_paths = sorted(
        path for directory in ("crates", "python", "benches")
        for path in (root / directory).rglob("*")
        if path.is_file() and path.suffix in (".py", ".pyi", ".rs", ".toml")
        and "__pycache__" not in path.parts
    )
    report = {
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "python": sys.version,
        "executable": sys.executable,
        "processor": platform.processor(),
        "native_sha256": hashlib.sha256(native.read_bytes()).hexdigest(),
        "source_files_sha256": {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                                for path in source_paths},
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "working_tree_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"])).hexdigest(),
        "versions": {name: importlib.metadata.version(name) for name in
                     ["oxide-parse", "beautifulsoup4", "lxml", "parsel", "selectolax"]},
        "repeat": args.repeat,
        "warmups": 1,
        "seed": 20261108,
        "gc": "enabled; collect before each timed call, outside measurement",
        "measurement": "parse + extraction + Python result creation; precompiled oxide_parse plans; single document",
        "scenarios": [],
    }
    rng = random.Random(report["seed"])
    for size in args.sizes:
        for noise in (False, True):
            html = make_page(size, noise)
            plan, cases = build_cases(html)
            if len(cases) != 7:
                raise RuntimeError(f"Expected all 7 implementations, found {list(cases)}")
            expected = plan.extract(html)
            assert len(expected["items"]) == size
            for name, fn in cases.items():
                assert fn() == expected, f"Warmup mismatch: {name}"
            samples = {name: [] for name in cases}
            orders = []
            print(f"START products={size} noise={noise} bytes={len(html.encode('utf-8'))}", flush=True)
            for iteration in range(args.repeat):
                order = list(cases)
                rng.shuffle(order)
                orders.append(order)
                for name in order:
                    gc.collect()
                    started = time.perf_counter_ns()
                    result = cases[name]()
                    elapsed = (time.perf_counter_ns() - started) / 1_000_000
                    assert result == expected, f"Result mismatch: {name}"
                    samples[name].append(elapsed)
                    del result
                print(f"  round {iteration + 1}/{args.repeat} complete", flush=True)
            scenario = {
                "products": size, "noise": noise, "bytes": len(html.encode("utf-8")),
                "orders": orders,
                "results": {name: {"samples_ms": values, "median_ms": statistics.median(values),
                                   "min_ms": min(values), "max_ms": max(values)}
                            for name, values in samples.items()},
            }
            report["scenarios"].append(scenario)
            report["updated_utc"] = datetime.now(timezone.utc).isoformat()
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
            for name, values in sorted(samples.items(), key=lambda item: statistics.median(item[1])):
                print(f"  {name:28s} {statistics.median(values):10.3f} ms", flush=True)


if __name__ == "__main__":
    main()
