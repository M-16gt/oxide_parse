//! Cross-engine tests: the same schema must give the same answer on every engine that can run it.
#![cfg(all(feature = "engine-scraper", feature = "engine-lite", feature = "engine-stream", feature = "engine-json"))]
use oxide_parse_core::{available, compile, Expr, Store, Value};
use serde_json::json;

const PAGE: &str = r#"<html><body><h1> Shop </h1>
<ul id="nav"><li><a href="/a">A</a></li><li class="sel"><a href="/b">B</a></li><li><a href="/c">C</a></li></ul>
<div class="product" data-id="1"><span class="name">Lamp</span><span class="price">$19.90</span></div>
<div class="product" data-id="2"><span class="name">Desk &amp; Chair</span><span class="price">$1,149</span></div>
</body></html>"#;

fn run(engine: &str, ir: &str, src: &str) -> serde_json::Value {
    let plan = compile(&Expr::from_json(ir).unwrap(), engine).unwrap_or_else(|e| panic!("{engine}: {e}"));
    plan.extract(src, Value::Null).unwrap_or_else(|e| panic!("{engine}: {e}")).to_json()
}

const STREAMABLE: &str = r##"{"object":{
  "title":{"pipe":[{"css":"h1"},"text","strip"]},
  "n":{"pipe":[{"css":".product"},"count"]},
  "has_nav":{"pipe":[{"css":"#nav"},"exists"]},
  "first_href":{"pipe":[{"css":"#nav a"},{"attr":"href"}]},
  "items":{"pipe":[{"css":".product"},{"map":{"object":{
      "i":{"pipe":["index"]},
      "id":{"pipe":[{"attr":"data-id"},"int"]},
      "name":{"pipe":[{"css":".name"},"text"]},
      "price":{"pipe":[{"css":".price"},"text","float"]}
  }}}]},
  "links":{"pipe":[{"css":"#nav li"},{"map":{"pipe":[{"css":"a"},{"attr":"href"}]}}]},
  "missing":{"pipe":[{"css":".nope"},"text",{"default":{"lit":"-"}}]}
}}"##;

#[test]
fn stream_lite_scraper_agree() {
    let expected = run("scraper", STREAMABLE, PAGE);
    assert_eq!(expected["items"][1]["name"], json!("Desk & Chair"));
    assert_eq!(expected["items"][1]["price"], json!(1149.0));
    assert_eq!(expected["links"], json!(["/a", "/b", "/c"]));
    assert_eq!(expected["missing"], json!("-"));
    assert_eq!(run("lite", STREAMABLE, PAGE), expected);
    assert_eq!(run("stream", STREAMABLE, PAGE), expected);
    assert_eq!(run("auto", STREAMABLE, PAGE), expected);
}

#[test]
fn auto_picks_stream_only_when_possible() {
    let ok = compile(&Expr::from_json(STREAMABLE).unwrap(), "auto").unwrap();
    assert_eq!(ok.backend(), "stream");
    let dom_only = Expr::from_json(r#"{"object":{"p":{"pipe":[{"css":".name"},"first","parent",{"attr":"class"}]}}}"#).unwrap();
    assert!(compile(&dom_only, "stream").is_err());
    assert_ne!(compile(&dom_only, "auto").unwrap().backend(), "stream");
}

#[test]
fn xpath_agrees_between_dom_engines() {
    let ir = r##"{"object":{
      "hrefs":{"pipe":[{"xpath":"//ul[@id='nav']/li/a/@href"}]},
      "second":{"pipe":[{"xpath":"//li[2]/a/text()"},"first"]},
      "names":{"pipe":[{"xpath":"//div[@class='product']"},{"map":{"pipe":[{"xpath":"./span[@class='name']/text()"},"first"]}}]},
      "count":{"pipe":[{"xpath":"count(//div[@class='product'])"}]},
      "sel":{"pipe":[{"xpath":"//li[contains(@class,'sel')]/a"},"text"]},
      "last":{"pipe":[{"xpath":"//li[last()]/a/@href"},"first"]},
      "pricey":{"pipe":[{"xpath":"//div[number(translate(substring-after(span[@class='price'],'$'),',','')) > 100]/@data-id"}]},
      "sib":{"pipe":[{"xpath":"//li[@class='sel']/following-sibling::li/a/@href"}]},
      "up":{"pipe":[{"xpath":"//span[@class='name'][1]/parent::div/@data-id"}]}
    }}"##;
    let expected = json!({
        "hrefs": ["/a", "/b", "/c"],
        "second": "B",
        "names": ["Lamp", "Desk & Chair"],
        "count": 2,
        "sel": "B",
        "last": "/c",
        "pricey": ["2"],
        "sib": ["/c"],
        "up": ["1", "2"]
    });
    assert_eq!(run("scraper", ir, PAGE), expected);
    assert_eq!(run("lite", ir, PAGE), expected);
}

#[test]
fn lite_tree_rules() {
    let html = "<ul><li>a<li>b</ul><p>x<p>y<div>z</div><img src=x><br><p class='amp'>a &amp; b</p>\
                <script>if (a<b) { x = '</p>'; }</script><a href='/q?a=1&amp;b=2'>l</a>";
    let ir = r##"{"object":{
      "li":{"pipe":[{"css":"li"},"count"]},
      "p":{"pipe":[{"css":"p"},"count"]},
      "amp":{"pipe":[{"css":"p.amp"},"text"]},
      "href":{"pipe":[{"css":"a"},{"attr":"href"}]},
      "script":{"pipe":[{"css":"script"},"text"]}
    }}"##;
    let out = run("lite", ir, html);
    assert_eq!(out["li"], json!(2));
    assert_eq!(out["p"], json!(3));
    assert_eq!(out["amp"], json!("a & b"));
    assert_eq!(out["href"], json!("/q?a=1&b=2"));
    assert_eq!(out["script"], json!("if (a<b) { x = '</p>'; }"));
    // the stream engine sees the same structure
    assert_eq!(run("stream", ir, html), out);
}

#[test]
fn engines_are_listed() {
    let names: Vec<&str> = available().iter().map(|e| e.name).collect();
    for n in ["scraper", "lite", "stream", "json"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
}

#[test]
fn xpath_errors_are_reported_at_compile_time() {
    for bad in ["//a[", "//a/@href/b", "//a/text()/b", "following::a", "//a[@x='"] {
        let ir = Expr::from_json(&format!(r#"{{"pipe":[{{"xpath":{}}}]}}"#, serde_json::to_string(bad).unwrap())).unwrap();
        assert!(compile(&ir, "lite").is_err(), "{bad} should not compile");
    }
}

const V1: &str = r#"<section><article class="product" id="p1"><h3>Product 1</h3><p class="description">Nice lamp</p></article></section>"#;
const V2: &str = r#"<div class="product-wrapper"><article class="product new-class" data-id="p1"><div class="product-info"><h3>Product 1</h3><p class="new-description">Nice lamp</p></div></article></div>"#;
const ADAPTIVE: &str = r##"{"object":{
  "title":{"pipe":[{"adaptive":{"css":"#p1 h3","key":"title"}},"text"]},
  "desc":{"pipe":[{"adaptive":{"css":"#p1 .description","key":"desc"}},"text"]}
}}"##;

#[test]
fn adaptive_relocates_after_a_redesign() {
    for engine in ["scraper", "lite"] {
        let plan = compile(&Expr::from_json(ADAPTIVE).unwrap(), engine).unwrap();
        // v1: selectors match, fingerprints are learned
        assert_eq!(plan.extract(V1, Value::Null).unwrap().to_json(), json!({"title": "Product 1", "desc": "Nice lamp"}), "{engine}");
        // v2: the ids/classes are gone, the selectors match nothing, the elements are found by similarity
        assert_eq!(plan.extract(V2, Value::Null).unwrap().to_json(), json!({"title": "Product 1", "desc": "Nice lamp"}), "{engine}");
        // a plan that never saw v1 has nothing to compare with
        let fresh = compile(&Expr::from_json(ADAPTIVE).unwrap(), engine).unwrap();
        assert_eq!(fresh.extract(V2, Value::Null).unwrap().to_json(), json!({"title": null, "desc": null}), "{engine}");
    }
}

#[test]
fn adaptive_store_persists_in_a_file() {
    let path = std::env::temp_dir().join(format!("oxide_parse_adaptive_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let ir = Expr::from_json(ADAPTIVE).unwrap();
    {
        let store: std::sync::Arc<dyn oxide_parse_core::Store> = std::sync::Arc::new(oxide_parse_core::FileStore::open(&path).unwrap());
        let plan = oxide_parse_core::compile_with(&ir, "lite", Some(store)).unwrap();
        plan.extract(V1, Value::Null).unwrap();
    }
    let store: std::sync::Arc<dyn oxide_parse_core::Store> = std::sync::Arc::new(oxide_parse_core::FileStore::open(&path).unwrap());
    let plan = oxide_parse_core::compile_with(&ir, "lite", Some(store)).unwrap(); // "next run"
    assert_eq!(plan.extract(V2, Value::Null).unwrap().to_json()["title"], json!("Product 1"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn adaptive_needs_introspection_and_is_not_streamable() {
    let ir = Expr::from_json(ADAPTIVE).unwrap();
    assert!(compile(&ir, "json").is_err());
    assert!(compile(&ir, "stream").is_err());
    assert_ne!(compile(&ir, "auto").unwrap().backend(), "stream");
}

// ---- regressions from the code audit ---------------------------------------------------------------------------

#[test]
fn audit_nth_child_extreme_values_do_not_panic() {
    let html = "<div><p>a</p><p>b</p><p>c</p></div>";
    for sel in ["p:nth-child(-1n-2147483648)", "p:nth-child(2n-2147483648)", "p:nth-child(-1n+2147483647)", "p:nth-child(2147483647n)"] {
        let ir = format!(r#"{{"pipe":[{{"css":{}}},"count"]}}"#, serde_json::to_string(sel).unwrap());
        for engine in ["lite", "stream", "scraper"] {
            let Ok(plan) = compile(&Expr::from_json(&ir).unwrap(), engine) else { continue };
            let _ = plan.extract(html, Value::Null); // must not panic (scraper may reject the selector)
        }
    }
    let ir = Expr::from_json(r#"{"pipe":[{"css":"p:nth-child(-1n+2)"},"count"]}"#).unwrap();
    assert_eq!(compile(&ir, "lite").unwrap().extract(html, Value::Null).unwrap().to_json(), json!(2));
}

#[test]
fn audit_abrupt_comments_do_not_swallow_the_document() {
    let ir = r#"{"object":{"t":{"pipe":[{"css":"h1"},"text"]}}}"#;
    for html in ["<!--><h1>x</h1>", "<!---><h1>x</h1>", "<!-- c --><h1>x</h1>"] {
        assert_eq!(run("lite", ir, html), json!({"t": "x"}), "{html}");
        assert_eq!(run("stream", ir, html), json!({"t": "x"}), "{html}");
    }
}

#[test]
fn audit_duplicate_attributes_first_wins_in_every_engine() {
    let html = r#"<div class="first" class="second">Hello</div>"#;
    let ir = r#"{"object":{"a":{"pipe":[{"css":".first"},"text"]},"b":{"pipe":[{"css":".second"},"text"]}}}"#;
    let expected = json!({"a": "Hello", "b": null});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn audit_script_and_style_text_is_left_out_of_ancestors_but_kept_for_themselves() {
    let html = "<article>A <script>alert(1)</script>B<style>p{}</style></article><script id=d>{\"k\":1}</script>";
    let ir = r##"{"object":{"art":{"pipe":[{"css":"article"},"text"]},"js":{"pipe":[{"css":"#d"},"text"]}}}"##;
    let expected = json!({"art": "A B", "js": "{\"k\":1}"});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn audit_html_tree_rules() {
    // honoured only for void/foreign elements: `<div/>` stays open like in a browser, `<path/>` inside <svg> closes
    let html = "<svg><path d='x'/><circle/></svg><p class=after>t</p>";
    let ir = r#"{"object":{"n":{"pipe":[{"css":"svg > *"},"count"]},"after":{"pipe":[{"css":"svg + p, p.after"},"count"]}}}"#;
    assert_eq!(run("lite", ir, html)["n"], json!(2));
    // a stray </div> inside a table cell must not close the page div around the table
    let html = "<div class=page><table><tr><td>a</div>b</td></tr></table><p class=in>x</p></div>";
    let ir = r#"{"object":{"in":{"pipe":[{"css":".page > p.in"},"count"]}}}"#;
    assert_eq!(run("lite", ir, html), json!({"in": 1}));
    // implied end tags
    let html = "<ul><li>a<li>b</ul><p>x<p>y<div>z</div>";
    let ir = r#"{"object":{"li":{"pipe":[{"css":"li"},"count"]},"p":{"pipe":[{"css":"p"},"count"]},"pd":{"pipe":[{"css":"p > div"},"count"]}}}"#;
    assert_eq!(run("lite", ir, html), json!({"li": 2, "p": 2, "pd": 0}));
}

#[test]
fn audit_text_options_agree_between_dom_engines() {
    let html = "<div><p>one<br>two</p><b class=ad>AD</b><i>3</i></div>";
    let ir = r#"{"object":{"t":{"pipe":[{"css":"div"},{"text_opts":{"sep":"|","strip":true,"without":".ad"}}]}}}"#;
    let expected = json!({"t": "one|two|3"});
    assert_eq!(run("scraper", ir, html), expected);
    assert_eq!(run("lite", ir, html), expected);
}

#[test]
fn stream_handles_has_attr_and_value_only_maps_like_the_dom_engines() {
    let html = r#"<div><button class=buy disabled>Buy</button><button class=other>x</button>
        <script id=d>{"items":[{"name":"a"},{"name":"b"}]}</script></div>"#;
    let ir = r##"{"object":{
      "dis":{"pipe":[{"css":".buy"},{"has_attr":"disabled"}]},
      "dis2":{"pipe":[{"css":".other"},{"has_attr":"disabled"}]},
      "none":{"pipe":[{"css":".nope"},{"has_attr":"disabled"}]},
      "names":{"pipe":[{"css":"script#d"},"text","parse_json",{"key":"items"},{"map":{"pipe":[{"key":"name"},"upper"]}}]}
    }}"##;
    let expected = json!({"dis": true, "dis2": false, "none": false, "names": ["A", "B"]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
    assert_eq!(compile(&Expr::from_json(ir).unwrap(), "auto").unwrap().backend(), "stream");
}

#[test]
fn stream_skips_selectors_it_no_longer_needs_without_changing_results() {
    // the title is found first; many later h1 elements and a noisy region between cards must not matter
    let mut html = String::from("<h1>First</h1>");
    for i in 0..50 {
        html.push_str(&format!(
            "<div class=product data-id={i}><h2 class=name>P{i}</h2><span class=price>${i}.50</span></div><div class=banner><span>ad</span><h1>ad</h1></div>"
        ));
    }
    let ir = r##"{"object":{
      "title":{"pipe":[{"css":"h1"},"text"]},
      "n":{"pipe":[{"css":".price"},"count"]},
      "items":{"pipe":[{"css":".product"},{"map":{"object":{
          "id":{"pipe":[{"attr":"data-id"},"int"]},
          "name":{"pipe":[{"css":".name"},"text"]},
          "price":{"pipe":[{"css":".price"},"text","float"]}}}}]}
    }}"##;
    let expected = run("lite", ir, &html);
    assert_eq!(expected["title"], json!("First"));
    assert_eq!(expected["n"], json!(50));
    assert_eq!(run("stream", ir, &html), expected);
    assert_eq!(run("scraper", ir, &html), expected);
}

#[test]
fn stream_preserves_item_index_in_tail_ops() {
    // `default(index)` in the tail of a record field: the second card has no `.id`, so it falls back to its index
    let html = "<div class=p><span class=id>10</span></div><div class=p></div>";
    let ir = r#"{"object":{"ids":{"pipe":[{"css":".p"},{"map":{"pipe":[{"css":".id"},"text","int",{"default":{"pipe":["index"]}}]}}]}}}"#;
    let expected = json!({"ids": [10, 1]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
    assert_eq!(compile(&Expr::from_json(ir).unwrap(), "auto").unwrap().backend(), "stream");
}

// ---- stage 1 of the v7 plan: fixes in existing code -------------------------------------------------------------

#[test]
fn stage1_script_with_a_slash_is_still_a_raw_text_element() {
    // HTML ignores the slash of `<script ... />`: the element stays open, its content is raw text
    let html = "<article>Text<script src=x.js />alert(\"<b>XSS</b>\")</script></article>";
    let ir = r#"{"object":{"t":{"pipe":[{"css":"article"},"text"]},"b":{"pipe":[{"css":"b"},"count"]}}}"#;
    let expected = json!({"t": "Text", "b": 0});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
    // ... but inside <svg> a slash does close the element, and the tokenizer must agree with the tree rules
    let svg = "<svg><title/><circle/></svg><p>x</p>";
    let ir = r#"{"object":{"n":{"pipe":[{"css":"svg > *"},"count"]},"p":{"pipe":[{"css":"p"},"count"]}}}"#;
    for engine in ["lite", "stream"] {
        assert_eq!(run(engine, ir, svg), json!({"n": 2, "p": 1}), "{engine}");
    }
}

#[test]
fn stage1_first_then_first_still_means_first_child() {
    // select|first -> select_first must not swallow a second `first` (on a node it means "first child")
    let html = "<div><a><b>1</b><i>2</i></a><a><b>3</b></a></div>";
    let ir = r#"{"object":{"t":{"pipe":[{"css":"a"},"first","first","text"]}}}"#;
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, ir, html), json!({"t": "1"}), "{engine}");
    }
}

#[test]
fn stage1_stream_first_strip_and_meta() {
    let html = "<h1>  x </h1>";
    let ir = r#"{"object":{
        "t":{"pipe":[{"css":"h1"},"first","strip"]},
        "m":{"pipe":[{"meta":"k"},"upper"]},
        "d":{"pipe":[{"css":".nope"},"text",{"default":{"pipe":[{"meta":"k"}]}}]}
    }}"#;
    let meta = Value::from_json(json!({"k": "v"}));
    let expected = json!({"t": "x", "m": "V", "d": "v"});
    for engine in ["scraper", "lite", "stream"] {
        let plan = compile(&Expr::from_json(ir).unwrap(), engine).unwrap();
        assert_eq!(plan.extract(html, meta.clone()).unwrap().to_json(), expected, "{engine}");
    }
    assert_eq!(compile(&Expr::from_json(ir).unwrap(), "auto").unwrap().backend(), "stream");
    // `count` / `exists` / `first` after a selector mean something different on a node: not streamed as text
    let dom_only = r#"{"object":{"c":{"pipe":[{"css":"div"},"first","count"]}}}"#;
    assert_ne!(compile(&Expr::from_json(dom_only).unwrap(), "auto").unwrap().backend(), "stream");
}

#[test]
fn stage1_stream_early_exit_keeps_results() {
    let mut html = String::from("<title>A</title><meta property=og:image content=img1><h1>B</h1>");
    for _ in 0..5000 {
        html.push_str("<p>filler</p>");
    }
    html.push_str("<title>C</title><meta property=og:image content=img2><h1>D</h1>");
    let ir = r#"{"object":{
        "title":{"pipe":[{"css":"title"},"text"]},
        "img":{"pipe":[{"css":"meta[property='og:image']"},{"attr":"content"}]},
        "h1":{"pipe":[{"css":"h1"},"text"]}
    }}"#;
    let expected = json!({"title": "A", "img": "img1", "h1": "B"});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, &html), expected, "{engine}");
    }
}

#[test]
fn stage1_adaptive_value_guard() {
    use std::sync::Arc;
    let store = Arc::new(oxide_parse_core::MemoryStore::default());
    let dyn_store: Arc<dyn oxide_parse_core::Store> = store.clone();
    let ir = Expr::from_json(
        r##"{"object":{"p":{"pipe":[{"adaptive":{"css":"#x .price","key":"price","threshold":0.2}},"text","float"]}}}"##,
    )
    .unwrap();
    let plan = oxide_parse_core::compile_with(&ir, "lite", Some(dyn_store)).unwrap();
    // the selector matches a "sold out" block but the pipeline fails: nothing may be learned from it
    assert!(plan.extract("<div id=x><span class=price>Sold out</span></div>", Value::Null).is_err());
    assert!(store.keys().is_empty());
    // a good page teaches the baseline
    let good = plan.extract("<div id=x><span class=price>$5</span></div>", Value::Null).unwrap();
    assert_eq!(good.to_json(), json!({"p": 5.0}));
    assert_eq!(store.keys(), vec!["price".to_string()]);
    // a redesign whose relocated element is garbage fails too: it must not be remembered as "healed"
    assert!(plan.extract("<section><span class=amount>n/a</span></section>", Value::Null).is_err());
    assert_eq!(store.keys(), vec!["price".to_string()]);
}

#[test]
fn stage1_deeply_nested_selectors_are_errors_not_crashes() {
    let n = 20000;
    let cases = [
        ("xpath", format!("{}1{}", "(".repeat(n), ")".repeat(n))),
        ("xpath", format!("{}1", "-".repeat(n))),
        ("css", format!("a{}{}", ":not(a".repeat(n), ")".repeat(n))),
    ];
    for (kind, src) in cases {
        let ir = Expr::from_json(&format!(r#"{{"pipe":[{{"{kind}":{}}}]}}"#, serde_json::to_string(&src).unwrap())).unwrap();
        assert!(compile(&ir, "lite").is_err(), "{kind}: too deep must be an error");
    }
}

// ---- v7.3: audit of v7.2 ----------------------------------------------------------------------------------------------

#[test]
fn v73_json_navigation_moves_instead_of_cloning_and_keeps_semantics() {
    let html = r#"<script id=d>{"props":{"page":{"items":[{"sku":"a"},{"sku":"b"}],"cat":"E"}}}</script>"#;
    let ir = r##"{"object":{
        "cat":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"props"},{"key":"page"},{"key":"cat"}]},
        "skus":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"props"},{"key":"page"},{"key":"items"},{"map":{"pipe":[{"key":"sku"}]}}]},
        "second":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"props"},{"key":"page"},{"key":"items"},{"at":1},{"key":"sku"}]},
        "missing":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"nope"},{"key":"x"}]},
        "oob":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"props"},{"key":"page"},{"key":"items"},{"at":5}]}
    }}"##;
    let expected = json!({"cat": "E", "skus": ["a", "b"], "second": "b", "missing": null, "oob": null});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn v73_default_does_not_launder_a_failed_adaptive_pipeline() {
    use std::sync::Arc;
    let store = Arc::new(oxide_parse_core::MemoryStore::default());
    let dyn_store: Arc<dyn Store> = store.clone();
    let ir = Expr::from_json(
        r##"{"object":{"p":{"pipe":[{"adaptive":{"css":"#x .price","key":"price"}},"text","non_empty",{"default":{"lit":"N/A"}}]}}}"##,
    )
    .unwrap();
    let plan = oxide_parse_core::compile_with(&ir, "lite", Some(dyn_store)).unwrap();
    // the selector matches an empty <span>; `default` fills in "N/A": the span must not become the reference
    let out = plan.extract("<div id=x><span class=price> </span></div>", Value::Null).unwrap();
    assert_eq!(out.to_json(), json!({"p": "N/A"}));
    assert!(store.keys().is_empty());
    // a real value is learned as usual
    let out = plan.extract("<div id=x><span class=price>$5</span></div>", Value::Null).unwrap();
    assert_eq!(out.to_json(), json!({"p": "$5"}));
    assert_eq!(store.keys(), vec!["price".to_string()]);
}

#[test]
fn v73_xpath_sibling_axes() {
    let html = "<ul><li class=a><a href=/a>A</a></li><li class=sel><a href=/b>B</a></li><li class=c><a href=/c>C</a></li></ul>";
    let ir = r##"{"object":{
        "following":{"pipe":[{"xpath":"//li[@class='sel']/following-sibling::li/a/@href"}]},
        "preceding":{"pipe":[{"xpath":"//li[@class='sel']/preceding-sibling::li/a/@href"}]},
        "nearest":{"pipe":[{"xpath":"//li[@class='c']/preceding-sibling::li[1]/a/@href"}]},
        "all_before_c":{"pipe":[{"xpath":"//li[@class='c']/preceding-sibling::li/a/@href"}]}
    }}"##;
    let expected = json!({"following": ["/c"], "preceding": ["/a"], "nearest": ["/b"], "all_before_c": ["/b", "/a"]});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn v73_selectors_inside_a_card_still_see_ancestors_outside_it() {
    // `element.select(sel)` limits the *candidates*, the selector itself is matched against the whole document
    // (querySelectorAll semantics). All engines must agree on that.
    let html = "<div class=header><div class=card><span class=title>T</span></div></div>";
    let ir = r##"{"object":{"t":{"pipe":[{"css":".card"},{"map":{"object":{"x":{"pipe":[{"css":".header .title"},"text"]}}}}]}}}"##;
    let expected = json!({"t": [{"x": "T"}]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn conformance_table_containers_close_with_their_rows() {
    let html = "<table><thead><tr><th>A<th>B<tbody><tr><td>1<td>2<tr><td>3</table>";
    let ir = r#"{"object":{
        "sections":{"pipe":[{"css":"table > tbody"},"count"]},
        "rows":{"pipe":[{"css":"tbody > tr"},"count"]},
        "cells":{"pipe":[{"css":"tbody > tr > td"},"count"]}
    }}"#;
    let expected = json!({"sections": 1, "rows": 2, "cells": 3});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, html), expected, "{engine}");
    }
}

#[test]
fn conformance_head_ignores_stray_paragraph_end() {
    let ir = r#"{"object":{"paragraphs":{"pipe":[{"css":"p"},"count"]}}}"#;
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, ir, "<head></p><meta><p>"), json!({"paragraphs": 1}), "{engine}");
        assert_eq!(run(engine, ir, "<p><hr></p>"), json!({"paragraphs": 2}), "{engine}");
    }
}
