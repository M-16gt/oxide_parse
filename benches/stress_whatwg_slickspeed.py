#!/usr/bin/env python3
"""Real WHATWG single-page stress test with SlickSpeed-style selectors.

Each engine runs in an isolated process with a timeout. Native crashes and
mismatches are reported and cause a nonzero exit. Synthetic input is explicit.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE_FILE = ROOT / "benchmarks/.cache/whatwg_html_spec.html"
WHATWG_URL = "https://html.spec.whatwg.org/"
SELECTORS = {
    "n_h2": "h2", "n_dfn": "dfn[id]", "n_code_in_pre": "div.example pre > code, pre > code",
    "n_ext_links": "a[href^='https://']", "n_tables": "table > tbody > tr > td",
    "n_not_Note": "p:not(.note)", "n_nth": "ul > li:nth-child(2n+1)",
}


def load_or_fetch_whatwg(offline: bool = False) -> str:
    if CACHE_FILE.exists():
        return CACHE_FILE.read_text(encoding="utf8")
    if offline:
        raise FileNotFoundError(f"No cached WHATWG document at {CACHE_FILE}")
    print(f"Downloading {WHATWG_URL}", flush=True)
    req = urllib.request.Request(WHATWG_URL, headers={"User-Agent": "oxide-parse-conformance/1"})
    with urllib.request.urlopen(req, timeout=60) as response:
        raw = response.read().decode("utf8")
    if "<title>" not in raw or len(raw) < 1_000_000:
        raise ValueError("Response is not the full single-page specification")
    CACHE_FILE.parent.mkdir(parents=True, exist_ok=True)
    temp = CACHE_FILE.with_suffix(".tmp")
    temp.write_text(raw, encoding="utf8", newline="\n")
    temp.replace(CACHE_FILE)
    return raw


def synthetic() -> str:
    chunk = ("<section><h2>Heading</h2><p><dfn id='x'>term</dfn><a href='https://example.org'>link</a></p>"
             "<p class='note'>Note</p><div class='example'><pre><code>let x=42;</code></pre></div>"
             "<ul><li>A</li><li>B</li><li>C</li></ul><table><tbody><tr><td>A</td><td>B</td></tr></tbody></table></section>\n")
    return "<html><head><title>Synthetic Spec</title></head><body>" + chunk * 35000 + "</body></html>"


def worker(engine: str, input_path: Path, repeat: int) -> dict:
    # Compilation, disk IO and imports are excluded. Each timed call includes
    # parsing + extraction + conversion to the same Python dict shape.
    from oxide_parse import T, compile
    html = input_path.read_text(encoding="utf8")
    if engine == "selectolax":
        from selectolax.lexbor import LexborHTMLParser

        def extract():
            doc = LexborHTMLParser(html)
            title = doc.css_first("title")
            return {"title": title.text() if title is not None else None,
                    # Lexbor returns the same node twice for overlapping list
                    # arms. CSS union is a set; deduplicate by native identity.
                    **{name: len({node.mem_id for node in doc.css(sel)}) for name, sel in SELECTORS.items()}}
    else:
        schema = {"title": T.css("title").text(), **{name: T.css(sel).count() for name, sel in SELECTORS.items()}}
        plan = compile(schema, backend=engine)
        extract = lambda: plan.extract(html)
    expected = extract()
    samples = []
    for _ in range(repeat):
        start = time.perf_counter()
        actual = extract()
        samples.append((time.perf_counter() - start) * 1000)
        if actual != expected:
            raise AssertionError(f"Nondeterministic extraction: {expected!r} != {actual!r}")
    return {"engine": engine, "status": "PASS", "result": expected,
            "median_ms": statistics.median(samples), "samples_ms": samples}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--synthetic", action="store_true")
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=180)
    parser.add_argument("--output", type=Path, default=ROOT / "target/conformance/stress.json")
    parser.add_argument("--worker", choices=["lite", "stream", "scraper", "selectolax"], help=argparse.SUPPRESS)
    parser.add_argument("--input", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.repeat < 1 or args.timeout <= 0:
        parser.error("repeat and timeout must be positive")
    if args.worker:
        print(json.dumps(worker(args.worker, args.input, args.repeat), ensure_ascii=True))
        return 0
    if args.synthetic:
        html = synthetic()
        input_path = CACHE_FILE.with_name("synthetic_spec.html")
        input_path.parent.mkdir(parents=True, exist_ok=True)
        input_path.write_text(html, encoding="utf8", newline="\n")
    else:
        html = load_or_fetch_whatwg(args.offline)
        input_path = CACHE_FILE
    from oxide_parse import engines
    import oxide_parse._core as core
    import importlib.metadata
    available = {e.name for e in engines()}
    order = ["lite", "stream", "scraper", "selectolax"]
    random.Random(20261115).shuffle(order)
    payload = {"source": "synthetic" if args.synthetic else WHATWG_URL,
               "input_sha256": hashlib.sha256(input_path.read_bytes()).hexdigest(),
               "bytes": len(html.encode("utf8")), "python": platform.python_version(),
               "native_sha256": hashlib.sha256(Path(core.__file__).read_bytes()).hexdigest(),
               "selectolax_version": importlib.metadata.version("selectolax"),
               "selectors": SELECTORS, "order": order, "repeat": args.repeat, "results": [],
               "selectolax_adapter": "Deduplicates overlapping CSS-list matches by node.mem_id; deduplication is timed"}
    print(f"Input {payload['bytes'] / 1024 / 1024:.2f} MiB; {args.repeat} repeats per engine", flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for engine in order:
        if engine != "selectolax" and engine not in available:
            result = {"engine": engine, "status": "ERROR", "error": "engine not built"}
        else:
            command = [sys.executable, str(Path(__file__).resolve()), "--worker", engine, "--input", str(input_path), "--repeat", str(args.repeat)]
            try:
                process = subprocess.run(command, capture_output=True, encoding="utf8", timeout=args.timeout,
                                         env={**os.environ, "PYTHONUTF8": "1"})
                if process.returncode:
                    result = {"engine": engine, "status": "ERROR", "exit_code": process.returncode, "error": process.stderr}
                else:
                    result = json.loads(process.stdout)
            except subprocess.TimeoutExpired:
                result = {"engine": engine, "status": "ERROR", "error": f"Timeout after {args.timeout}s"}
        payload["results"].append(result)
        args.output.write_text(json.dumps(payload, ensure_ascii=True, indent=2) + "\n", encoding="utf8")
        print(f"{engine}: {result.get('median_ms', result.get('error'))}", flush=True)
    successful = {r["engine"]: r for r in payload["results"] if r["status"] == "PASS"}
    checks = []
    for left, right in [("lite", "stream"), ("lite", "scraper"), ("scraper", "selectolax")]:
        if left in successful and right in successful:
            a, b = successful[left]["result"], successful[right]["result"]
            checks.append({"engines": [left, right], "status": "PASS" if a == b else "FAIL",
                           "differences": {k: {left: a[k], right: b[k]} for k in a if a[k] != b[k]}})
        else:
            checks.append({"engines": [left, right], "status": "ERROR", "error": "missing successful extraction"})
    payload["comparisons"] = checks
    payload["timing_note"] = "Same cached real input; process startup and compilation excluded; one warmup; engine order seeded; no thermal/load isolation. Mismatched results invalidate speed comparisons."
    args.output.write_text(json.dumps(payload, ensure_ascii=True, indent=2) + "\n", encoding="utf8")
    for check in checks:
        print(json.dumps(check, ensure_ascii=True), flush=True)
    return int(any(r["status"] != "PASS" for r in payload["results"] + checks))


if __name__ == "__main__":
    raise SystemExit(main())
