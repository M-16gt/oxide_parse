"""Adaptive selectors: survive a redesign without touching the schema.

T.css(sel, adaptive="key") behaves like T.css(sel) while it matches (and remembers a fingerprint of the match).
When it stops matching, the element most similar to the remembered one is relocated.
"""
import tempfile
from pathlib import Path

from oxide_parse import T, compile

V1 = '<section><article class="product" id="p1"><h3>Product 1</h3><p class="description">Nice lamp</p></article></section>'
V2 = '<div class="wrap"><article class="product new" data-id="p1"><div class="info"><h3>Product 1</h3><p class="new-desc">Nice lamp</p></div></article></div>'

schema = {
    "title": T.css("#p1 h3", adaptive="title").text(),
    "desc": T.css("#p1 .description", adaptive="desc", threshold=0.5).text(),
}

with tempfile.TemporaryDirectory() as directory:
    store = Path(directory) / "fingerprints.json"   # omit it to keep fingerprints in memory
    print("run 1 (old layout):", compile(schema, adaptive_store=store).extract(V1))
    print("run 2 (new layout):", compile(schema, adaptive_store=store).extract(V2))   # selectors broke, data did not
print("without history   :", compile(schema).extract(V2))                          # nothing to compare with -> None
