from oxide_parse import T, coalesce, compile, if_

HTML = """
<div class="cat"><h2>Tools</h2>
  <div class="p" data-on="1"><b>Saw</b><s class="sale">$5</s><i>$9</i></div>
  <div class="p" data-on="0"><b>Axe</b><i>$20</i></div></div>
<div class="cat"><h2>Toys</h2>
  <div class="p" data-on="1"><b>Ball</b><i>$3</i></div></div>
"""

plan = compile({
    "categories": T.css(".cat").map({
        "name": T.css("h2").text(),
        "active": T.css(".p").filter(T.attr("data-on") == "1").map({
            "name": T.css("b").text(),
            "category": T.parent().css("h2").text(),                 # navigate up
            "price": coalesce(T.css(".sale").text(), T.css("i").text()).float(),
            "kind": if_(T.css(".sale").exists(), "sale", "regular"),
            "page_title": T.root().css("h2").first().text(),          # back to the document root
        }),
    }),
})
for c in plan.extract(HTML)["categories"]:
    print(c)
