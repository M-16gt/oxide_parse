"""Compare two installed package snapshots in alternating, isolated Python workers."""
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


def worker(package_root):
    sys.path.insert(0, str(package_root))
    import oxide_parse
    from oxide_parse import _core
    from bench_python import build_cases, make_page

    package = Path(oxide_parse.__file__).parent
    if package.resolve() != (package_root / "oxide_parse").resolve():
        raise RuntimeError(f"Wrong package imported: {package}")
    metadata = {
        "package": str(package), "native_sha256": hashlib.sha256(Path(_core.__file__).read_bytes()).hexdigest(),
        "files_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                         for p in package.iterdir() if p.is_file()},
        "engines": [e.name for e in oxide_parse.engines()],
    }
    print(json.dumps(metadata), flush=True)
    cache = {}
    for line in sys.stdin:
        request = json.loads(line)
        key = (request["products"], request["noise"])
        if key not in cache:
            size, noise = key
            html = make_page(size, noise)
            _, cases = build_cases(html)
            expected = {"title": "Catalog", "items": [
                {"id": i, "name": f"Product {i}", "price": float(f"{i % 500}.99"),
                 "url": f"/p/{i}", "desc": f"Some description text for product {i}"}
                for i in range(size)
            ]}
            cache[key] = (cases, expected, len(html.encode()))
        cases, expected, size_bytes = cache[key]
        if request["op"] == "warmup":
            for name in request["cases"]:
                if cases[name]() != expected:
                    raise RuntimeError(f"Warmup mismatch: {key}, {name}")
            response = {"bytes": size_bytes, "cases": list(cases)}
        else:
            gc.collect()
            start = time.perf_counter_ns()
            result = cases[request["case"]]()
            elapsed = (time.perf_counter_ns() - start) / 1_000_000
            if result != expected:
                raise RuntimeError(f"Result mismatch: {key}, {request['case']}")
            del result
            response = {"ms": elapsed}
        print(json.dumps(response), flush=True)


def send(process, request):
    process.stdin.write(json.dumps(request) + "\n")
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        raise RuntimeError(f"Benchmark worker exited: {process.poll()}")
    return json.loads(line)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--sizes", nargs="+", type=int, default=[100, 1000, 10000])
    parser.add_argument("--repeat", type=int, default=7)
    parser.add_argument("--cooldown", type=float, default=0.25)
    args = parser.parse_args()
    if args.worker:
        worker(args.worker.resolve())
        return
    if not all((args.baseline, args.candidate, args.output)):
        parser.error("baseline, candidate and output are required")
    if args.repeat < 1 or min(args.sizes) < 1 or args.cooldown < 0:
        parser.error("sizes/repeat must be positive; cooldown must be nonnegative")
    root = Path(__file__).resolve().parents[1]
    engines = [f"oxide_parse [{e}]" for e in ("lite", "stream", "scraper")]
    external = ["selectolax", "parsel", "bs4 + lxml", "bs4 + html.parser"]
    processes = {}
    report = {
        "started_utc": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
        "python": sys.version, "repeat": args.repeat, "cooldown_seconds": args.cooldown,
        "seed": 20261110, "warmups": 1, "workers": {}, "samples": [],
        "measurement": "parse + extract + Python objects; precompiled plans; GC collection/equality/deletion/IPC outside timer",
        "versions": {n: importlib.metadata.version(n) for n in
                     ("beautifulsoup4", "lxml", "parsel", "selectolax")},
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "source_files_sha256": {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                                for directory in ("crates", "python", "benches")
                                for p in (root / directory).rglob("*")
                                if p.is_file() and p.suffix in (".rs", ".py", ".pyi", ".toml")},
        "harness_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in (Path(__file__), root / "benches/bench_python.py")},
    }
    scenarios = [(n, noise) for n in args.sizes for noise in (False, True)]
    rng = random.Random(report["seed"])
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        report["updated_utc"] = datetime.now(timezone.utc).isoformat()
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    try:
        for label, package in (("v6", args.baseline), ("v7", args.candidate)):
            process = subprocess.Popen(
                [sys.executable, "-I", "-u", "-c",
                 "import runpy,sys; sys.path.insert(0,sys.argv[1]); script=sys.argv[2]; sys.argv=[script]+sys.argv[3:]; runpy.run_path(script,run_name='__main__')",
                 str(root / "benches"), str(Path(__file__).resolve()), "--worker", str(package.resolve())],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
            )
            processes[label] = process
            line = process.stdout.readline()
            if not line:
                raise RuntimeError(f"Startup failed: {label}")
            report["workers"][label] = json.loads(line)
        for n, noise in scenarios:
            for label in processes:
                response = send(processes[label], {"op": "warmup", "products": n, "noise": noise,
                                                  "cases": engines + (external if label == "v7" else [])})
                print(f"WARMUP {label} products={n} noise={noise} bytes={response['bytes']}", flush=True)
        for iteration in range(args.repeat):
            order = scenarios.copy()
            rng.shuffle(order)
            for n, noise in order:
                groups = [[(label, engine) for label in ("v6", "v7")] for engine in engines]
                groups += [[("v7", name)] for name in external]
                rng.shuffle(groups)
                for group in groups:
                    rng.shuffle(group)
                    for label, name in group:
                        time.sleep(args.cooldown)
                        response = send(processes[label], {"op": "measure", "products": n,
                                                           "noise": noise, "case": name})
                        report["samples"].append({"round": iteration + 1, "products": n, "noise": noise,
                                                  "version": label, "case": name, "ms": response["ms"]})
                print(f"ROUND {iteration + 1}/{args.repeat} products={n} noise={noise}", flush=True)
                save()
        report["summary"] = []
        for n, noise in scenarios:
            results = {}
            for label, names in (("v6", engines), ("v7", engines + external)):
                for name in names:
                    values = [s["ms"] for s in report["samples"] if
                              (s["products"], s["noise"], s["version"], s["case"]) == (n, noise, label, name)]
                    results[f"{label}: {name}"] = {"samples_ms": values, "median_ms": statistics.median(values),
                                                  "min_ms": min(values), "max_ms": max(values)}
            paired = {}
            for name in engines:
                ratios = [a / b for a, b in zip(results[f"v6: {name}"]["samples_ms"],
                                               results[f"v7: {name}"]["samples_ms"])]
                paired[name] = {"ratios_v6_over_v7": ratios, "median_ratio": statistics.median(ratios),
                                "v7_faster_pairs": sum(r > 1 for r in ratios)}
            report["summary"].append({"products": n, "noise": noise, "results": results, "paired": paired})
        save()
        print(f"COMPLETE {len(report['samples'])} verified measurements", flush=True)
    finally:
        for process in processes.values():
            process.stdin.close()
            process.wait(timeout=30)


if __name__ == "__main__":
    main()
