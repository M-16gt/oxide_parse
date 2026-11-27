"""v7.2: quarantine bad records, resolve URLs, read JSON from scripts, sections, label/value rows,
batch extraction and the zero-IO repair loop.   Build: maturin develop --release --features full"""
import oxide_parse as op
from oxide_parse import T

HTML = """
<div class="card"><b class="price">$10</b></div>
<div class="card"><b class="price">sold out</b></div>
<div class="card"><b class="price">$30</b></div>
<a class="more" href="../next?page=2">next</a>
<script>window.__STATE__ = {"sku": "A-1", "stock": 7};</script>
<div class="specs"><h3>Size</h3><p>10x20</p><p>mm</p><h3>Colour</h3><p>red</p></div>
<table><tr><th>Weight:</th><td> 5 kg </td></tr></table>
"""

plan = op.compile({
    # a broken card is reported instead of failing the whole page
    "prices": T.css(".card").map(T.css(".price").text().float(), on_error="skip"),
    "next": T.css("a.more").attr("href").url(),                       # needs meta["base_url"]
    "sku": T.css("script").text().js_json("window.__STATE__")["sku"],
    "specs": T.css(".specs").sections("h3"),                          # flat markup -> [{header, items}]
    "weight": T.kv("Weight").text().strip(),                          # label -> value, no selector gymnastics
})
res = plan.extract_detailed(HTML, meta={"base_url": "https://shop.example/catalog/list"})
print(res.data)
print("quarantined:", res.errors)

# many documents, one failing: collect instead of raising
print(plan.extract_batch([HTML, "<p>no cards</p>"], errors="collect")[1])

# zero-IO self-healing: the library builds the request, YOU decide who answers (LLM, human, lookup table)
broken = op.compile({"price": T.css(".gone", adaptive="price").text()})
page = '<div><span class="cost">$5</span></div>'


def ask_llm(request):
    print("repair request keys:", sorted(request))        # misses, errors, skeleton, schema, task
    return {"price": ".cost"}                              # <- the model's answer


out = broken.extract_or_repair(page, ask_llm)
print(out.repaired, out.data)
print(out.plan.to_python())                                # the patched schema as code
