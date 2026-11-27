from oxide_parse import T, compile

HTML = """
<h1>Shop</h1>
<div class="product"><b class="name">Lamp</b><i class="price">$19.90</i><a href="/lamp">more</a></div>
<div class="product"><b class="name">Desk</b><i class="price">$1,149.00</i><a href="/desk">more</a></div>
"""

plan = compile({
    "title": T.css("h1").text(),
    "products": T.css(".product").map({
        "name": T.css(".name").text(),
        "price": T.css(".price").text().float(),   # lenient: "$1,149.00" -> 1149.0
        "url": T.css("a").attr("href"),
        "pos": T.index(),
    }),
    "cheapest_url": T.css(".product").first().css("a").attr("href"),
})

print(plan.explain())
print(plan.extract(HTML))
