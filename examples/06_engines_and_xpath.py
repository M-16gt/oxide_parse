"""Pick an engine, use XPath, and let `auto` choose between streaming and the DOM.

Build with several engines first:  maturin develop --release --features full
"""
import oxide_parse as op
from oxide_parse import T

HTML = """
<h1>Shop</h1>
<ul id="nav"><li><a href="/a">A</a></li><li class="sel"><a href="/b">B</a></li><li><a href="/c">C</a></li></ul>
<div class="product" data-id="1"><b class="name">Lamp</b><i class="price">$19.90</i></div>
<div class="product" data-id="2"><b class="name">Desk</b><i class="price">$1,149.00</i></div>
"""

print("compiled-in engines:")
for e in op.engines():
    print(f"  {e.name:<8} {e.kind:<6} xpath={e.xpath}  {e.description}")

# 1) streamable schema -> `auto` picks the no-DOM engine when it is compiled in
fast = op.compile({
    "title": T.css("h1").text(),
    "items": T.css(".product").map({
        "id": T.attr("data-id").int(),
        "name": T.css(".name").text(),
        "price": T.css(".price").text().float(),
    }),
}, backend="auto")
print("\nauto ->", fast.engine)
print(fast.extract(HTML))

# 2) XPath (typed like parsel: nodes, strings, numbers)
xp = op.compile({
    "hrefs": T.xpath("//ul[@id='nav']/li/a/@href"),
    "selected": T.xpath("//li[contains(@class,'sel')]/a").text(),
    "n_products": T.xpath("count(//div[@class='product'])"),
    "pricey": T.xpath("//div[number(translate(substring-after(i[@class='price'],'$'),',','')) > 100]/@data-id"),
})
print("\nxpath ->", xp.engine)
print(xp.extract(HTML))

# 3) not streamable (uses parent) -> `auto` falls back to the DOM; explain() says what runs
dom = op.compile({"cat": T.css(".name").first().parent().attr("data-id")}, backend="auto")
print("\nauto ->", dom.engine)
