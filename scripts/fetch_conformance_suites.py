#!/usr/bin/env python3
"""Fetch pinned upstream data. Tests themselves never use the network.

Run with --offline to verify/reconvert cached sources. All downloads and
parsing must succeed before external_suites.json is replaced.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import urllib.request
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parents[1] / "tests/fixtures/conformance"
REVISIONS = {
    "html5lib-tests": ("html5lib/html5lib-tests", "9329e64694e7835d0dcff9811e22856ef6ad16f9"),
    "cascadia": ("andybalholm/cascadia", "a34645470811a2e5829f87b3128949bce96c2a67"),
    "libxml2": ("GNOME/libxml2", "21c8223ac091b81d5914c52eb20b1b063e29cb92"),
    "css-select": ("fb55/css-select", "84ec1620ddf514d132ff75a4771f503f59da2b85"),
}


def parse_html5lib_dat(raw: str, source_name: str) -> list[dict]:
    cases = []
    for index, block in enumerate(re.split(r"(?m)^#data\n", raw.replace("\r\n", "\n"))[1:], 1):
        parts = re.split(r"(?m)^#([\w-]+)\n", block)
        sections = dict(zip(parts[1::2], parts[2::2]))
        if "document" not in sections:
            raise ValueError(f"Missing #document in {source_name}:{index}")
        cases.append({"id": f"{source_name}:{index}", "html": parts[0].removesuffix("\n"),
                      "expected_outline": sections["document"].rstrip("\n"),
                      "fragment": sections.get("document-fragment", "").strip(),
                      "script_off": "script-off" in sections})
    if not cases:
        raise ValueError(f"No .dat cases in {source_name}")
    return cases


GO_STRING = r'`[^`]*`|"(?:\\.|[^"\\])*"'


def decode_go_string(value: str) -> str:
    if value.startswith("`"):
        return value[1:-1].replace("\r", "")
    value = re.sub(r"\\x([0-9a-fA-F]{2})", lambda m: "\\u00" + m[1], value)
    value = value.replace(r"\v", r"\u000b").replace(r"\a", r"\u0007")
    return json.loads(value)


def parse_cascadia_go_tests(raw: str) -> list[dict]:
    table = raw.split("var selectorTests = []selectorTest{", 1)[1].split("\nfunc ", 1)[0]
    pattern = re.compile(
        rf"\{{\s*({GO_STRING})\s*,\s*({GO_STRING})\s*,\s*(nil|\[\]string\{{(.*?)\}})\s*,?\s*\}}", re.S)
    cases = []
    for index, match in enumerate(pattern.finditer(table), 1):
        expected = [decode_go_string(s[0]) for s in re.finditer(f"({GO_STRING})", match[4] or "")]
        cases.append({"id": f"cascadia:{index}", "html": decode_go_string(match[1]),
                      "selector": decode_go_string(match[2]), "expected_count": len(expected),
                      "expected_html": expected})
    residue = re.sub(r"//[^\n]*", "", pattern.sub("", table))
    if residue.strip(" \t\r\n},"):
        raise ValueError(f"Unparsed cascadia table content: {residue[:200]!r}")
    if not cases:
        raise ValueError("No cascadia tests parsed")
    return cases


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    sources = []
    previous = OUT_DIR / "external_suites.json"
    old_hashes = {}
    if previous.exists():
        old_hashes = {s["path"]: s["sha256"] for s in json.loads(previous.read_text(encoding="utf8"))["sources"]}

    def fetch(suite: str, path: str) -> str:
        repo, rev = REVISIONS[suite]
        url = f"https://raw.githubusercontent.com/{repo}/{rev}/{path}"
        relative = f"{suite}/{path}"
        dest = OUT_DIR / relative
        if dest.exists():
            data = dest.read_bytes()
        elif args.offline:
            raise FileNotFoundError(dest)
        else:
            req = urllib.request.Request(url, headers={"User-Agent": "oxide-parse-conformance/1"})
            with urllib.request.urlopen(req, timeout=60) as response:
                data = response.read()
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        if relative in old_hashes and old_hashes[relative] != digest:
            raise ValueError(f"Cached source hash changed: {relative}")
        sources.append({"path": relative, "url": url, "revision": rev, "sha256": digest})
        return data.decode("utf8")

    html_cases = []
    for name in ("tests1", "tests2", "tables01", "scriptdata01", "entities01"):
        html_cases.extend(parse_html5lib_dat(fetch("html5lib-tests", f"tree-construction/{name}.dat"), name))
    tokenizer_cases = []
    for name in ("test1", "entities", "numericEntities"):
        raw = json.loads(fetch("html5lib-tests", f"tokenizer/{name}.test"))
        for i, case in enumerate(raw["tests"], 1):
            tokenizer_cases.append({"id": f"{name}:{i}", **case})
    css_cases = parse_cascadia_go_tests(fetch("cascadia", "selector_test.go"))
    sizzle = fetch("css-select", "test/sizzle.ts")
    sizzle_html = fetch("css-select", "test/fixtures/sizzle.html")
    # Only initial, immutable, document-context Sizzle assertions. Later tests
    # mutate the fixture or pass JS context nodes and need a JS runner.
    prefix = sizzle.split('it("element",', 1)[1].split("const object1", 1)[0]
    sizzle_cases = []
    for i, match in enumerate(re.finditer(r'\bt\(\s*("(?:\\.|[^"\\])*")\s*,\s*(\[[^\]]*\])\s*\)', prefix), 1):
        sizzle_cases.append({"id": f"sizzle:{i}", "selector": json.loads(match[1]),
                             "expected_ids": json.loads(match[2])})
    if not sizzle_cases:
        raise ValueError("No static Sizzle cases parsed")
    xpath_cases = []
    for name in ("base", "floats", "functions", "strings"):
        raw = fetch("libxml2", f"test/XPath/expr/{name}")
        for i, line in enumerate(raw.splitlines(), 1):
            if line.strip():
                xpath_cases.append({"id": f"libxml2/{name}:{i}", "xpath": line.strip()})
    fetch("libxml2", "test/XPath/docs/simple")
    fetch("libxml2", "test/XPath/tests/simplebase")
    for suite, license_path in (("html5lib-tests", "LICENSE"), ("cascadia", "LICENSE"),
                                ("css-select", "LICENSE"), ("css-select", "test/LICENSE"),
                                ("libxml2", "Copyright")):
        fetch(suite, license_path)
    payload = {"schema_version": 1, "sources": sources, "html5lib_tree_cases": html_cases,
               "html5lib_tokenizer_cases": tokenizer_cases, "cascadia_css_cases": css_cases,
               "sizzle_html": sizzle_html, "sizzle_css_cases": sizzle_cases, "libxml2_xpath_cases": xpath_cases}
    temp = OUT_DIR / "external_suites.json.tmp"
    temp.write_text(json.dumps(payload, ensure_ascii=True, indent=2) + "\n", encoding="utf8")
    temp.replace(previous)
    print(f"Saved {len(html_cases)} tree, {len(tokenizer_cases)} tokenizer, {len(css_cases)} cascadia, "
          f"{len(sizzle_cases)} Sizzle, {len(xpath_cases)} XPath cases; {len(sources)} pinned source files.")


if __name__ == "__main__":
    main()
