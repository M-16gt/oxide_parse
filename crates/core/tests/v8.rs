//! v8.0: `on_error` in the stream engine, one walk per record in `lite`, `json_path` + parse cache,
//! and the documented selector scope of record fields. Needs every engine: `cargo test --features full`.
#![cfg(all(feature = "engine-scraper", feature = "engine-lite", feature = "engine-stream", feature = "engine-json"))]
use oxide_parse_core::{compile, Engine, Expr, Value};
use serde_json::json;

fn plan(engine: &str, ir: &serde_json::Value) -> Box<dyn Engine> {
    compile(&Expr::from_json(&ir.to_string()).unwrap(), engine).unwrap_or_else(|e| panic!("{engine}: {e}"))
}

fn run(engine: &str, ir: &serde_json::Value, html: &str) -> serde_json::Value {
    plan(engine, ir).extract(html, Value::Null).unwrap_or_else(|e| panic!("{engine}: {e}")).to_json()
}

// ---- stream: on_error --------------------------------------------------------------------------------------------

const CARDS: &str = "<div class=p><b>1</b><i>a</i></div><div class=p><b>oops</b><i>b</i></div><div class=p><b>3</b><i>c</i></div>";

fn record_ir(mode: &str) -> serde_json::Value {
    json!({"object":{"v":{"pipe":[{"css":".p"},{"map_opts":{"on_error":mode,"body":{"object":{
        "n":{"pipe":[{"css":"b"},"text","int"]},
        "t":{"pipe":[{"css":"i"},"text"]},
        "i":{"pipe":["index"]}
    }}}}]}}})
}

#[test]
fn stream_on_error_matches_the_dom_engines() {
    for mode in ["skip", "null", "raise"] {
        let ir = record_ir(mode);
        if mode == "raise" {
            for engine in ["scraper", "lite", "stream"] {
                let err = plan(engine, &ir).extract(CARDS, Value::Null).unwrap_err().to_string();
                assert!(err.contains("$.v[1].n"), "{engine}: {err}");
            }
            continue;
        }
        let want = plan("scraper", &ir).extract_detailed(CARDS, Value::Null).unwrap();
        for engine in ["lite", "stream"] {
            let got = plan(engine, &ir).extract_detailed(CARDS, Value::Null).unwrap();
            assert_eq!(got.value.to_json(), want.value.to_json(), "{engine} {mode}");
            assert_eq!(got.errors, want.errors, "{engine} {mode}");
        }
    }
    // the surviving records keep their original index (`T.index()`), a skipped record leaves a gap
    assert_eq!(
        run("stream", &record_ir("skip"), CARDS),
        json!({"v": [{"n": 1, "t": "a", "i": 0}, {"n": 3, "t": "c", "i": 2}]})
    );
    assert_eq!(plan("auto", &record_ir("skip")).backend(), "stream");
}

#[test]
fn stream_on_error_applies_to_a_single_pipeline_body_and_reports_paths() {
    let ir = json!({"object":{"v":{"pipe":[{"css":".p"},{"map_opts":{"on_error":"null","body":{"pipe":[{"css":"b"},"text","int"]}}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        let d = plan(engine, &ir).extract_detailed(CARDS, Value::Null).unwrap();
        assert_eq!(d.value.to_json(), json!({"v": [1, null, 3]}), "{engine}");
        assert_eq!(d.errors.len(), 1, "{engine}");
        assert_eq!(d.errors[0].path, "$.v[1]", "{engine}");
    }
}

// ---- lite: one walk per record -------------------------------------------------------------------------------------

const SHOP: &str = r#"<ul>
<li class="card" id="c1"><h2 class="name">A</h2><span class="price">$1,299.50</span><a class="buy" href="/a">x</a><p class="tag">t1</p><p class="tag">t2</p><div class="note"><b class="deep">d1</b></div></li>
<li class="card" id="c2"><h2 class="name">B</h2><a class="buy">y</a><p class="tag">t3</p></li>
<li class="card" id="c3"><span class="price">5</span></li>
</ul>"#;

fn wide_ir() -> serde_json::Value {
    json!({"object":{"cards":{"pipe":[{"css":".card"},{"map":{"object":{
        "id":{"pipe":[{"attr":"id"}]},
        "name":{"pipe":[{"css":".name"},"text","strip",{"default":{"lit":"-"}}]},
        "price":{"pipe":[{"css":".price"},"text","float"]},
        "href":{"pipe":[{"css":".buy"},{"attr":"href"}]},
        "has_buy":{"pipe":[{"css":".buy"},"exists"]},
        "tags":{"pipe":[{"css":".tag"},{"map":{"pipe":["text"]}}]},
        "n_tags":{"pipe":[{"css":".tag"},"count"]},
        "first_tag":{"pipe":[{"css":".tag"},"first","text"]},
        "deep":{"pipe":[{"css":".deep"},"text"]},
        "deep2":{"pipe":[{"css":".deep"},"text",{"default":{"pipe":[{"css":".name"},"text"]}}]},
        "missing1":{"pipe":[{"css":".nope1"},"text"]},
        "missing2":{"pipe":[{"css":".nope2"},"exists"]},
        "missing3":{"pipe":[{"css":".nope3"},"count"]}
    }}}]}}})
}

#[test]
fn fused_record_fields_agree_with_the_per_field_walk() {
    let ir = wide_ir();
    let want = run("scraper", &ir, SHOP);
    assert_eq!(run("lite", &ir, SHOP), want);
    // spot checks so that "both wrong in the same way" cannot pass
    assert_eq!(want["cards"][0]["tags"], json!(["t1", "t2"]));
    assert_eq!(want["cards"][0]["n_tags"], json!(2));
    assert_eq!(want["cards"][1]["has_buy"], json!(true));
    assert_eq!(want["cards"][2]["name"], json!("-"));
    assert_eq!(want["cards"][0]["deep2"], json!("d1"));
    assert_eq!(want["cards"][1]["deep2"], json!("B"));
    assert_eq!(want["cards"][0]["missing2"], json!(false));
    assert_eq!(want["cards"][0]["missing3"], json!(0));
}

#[test]
fn fusion_survives_nested_maps_and_a_selector_used_both_ways() {
    // `.tag` is read with `first` in one field and fully in another, and a nested map re-enters `map_items`
    // (four distinct leading selectors: the fused walk is used from `FUSE_MIN_HEADS` = 4)
    let ir = json!({"object":{"cards":{"pipe":[{"css":".card"},{"map":{"object":{
        "one":{"pipe":[{"css":".tag"},"first","text"]},
        "all":{"pipe":[{"css":".tag"},{"map":{"pipe":["text"]}}]},
        "name":{"pipe":[{"css":".name"},"text"]},
        "price":{"pipe":[{"css":".price"},"text"]},
        "buy":{"pipe":[{"css":".buy"},"exists"]},
        "inner":{"pipe":[{"css":".tag"},{"map":{"object":{
            "t":{"pipe":["text"]},
            "card_name":{"pipe":[{"css":".name"},"text"]}
        }}}]}
    }}}]}}});
    let want = run("scraper", &ir, SHOP);
    assert_eq!(run("lite", &ir, SHOP), want);
    assert_eq!(want["cards"][0]["inner"][0]["t"], json!("t1"));
}

#[test]
fn explain_lists_the_fused_pipeline_unchanged() {
    // fusion is a runtime strategy: the compiled plan (and `explain`) still shows plain selectors
    let e = plan("lite", &wide_ir()).explain();
    assert!(e.contains("select_first"), "{e}");
}

// ---- JSON: one parse, navigation by reference ------------------------------------------------------------------------

fn next_data_page() -> String {
    let products: Vec<serde_json::Value> =
        (0..40).map(|i| json!({"sku": format!("SKU-{i}"), "name": format!("Product number {i}"), "price": i as f64 + 0.5})).collect();
    let data = json!({"props":{"pageProps":{"category":"Electronics","products":products,"n":7}}});
    format!(r#"<html><body><script id="__NEXT_DATA__" type="application/json">{}</script></body></html>"#, data)
}

fn next_data_ir() -> serde_json::Value {
    let nd = json!([{"css":"script#__NEXT_DATA__"},"text","parse_json"]);
    let mut category = nd.as_array().unwrap().clone();
    category.extend([json!({"key":"props"}), json!({"key":"pageProps"}), json!({"key":"category"})]);
    let mut skus = nd.as_array().unwrap().clone();
    skus.extend([
        json!({"key":"props"}),
        json!({"key":"pageProps"}),
        json!({"key":"products"}),
        json!({"map":{"pipe":[{"key":"sku"}]}}),
    ]);
    let mut first_name = nd.as_array().unwrap().clone();
    first_name.extend([
        json!({"key":"props"}),
        json!({"key":"pageProps"}),
        json!({"key":"products"}),
        json!({"at":3}),
        json!({"key":"name"}),
    ]);
    let mut missing = nd.as_array().unwrap().clone();
    missing.extend([json!({"key":"props"}), json!({"key":"nope"}), json!({"key":"deeper"})]);
    json!({"object":{
        "category":{"pipe":category},
        "skus":{"pipe":skus},
        "name3":{"pipe":first_name},
        "missing":{"pipe":missing}
    }})
}

#[test]
fn parse_json_navigation_is_fused_and_correct_on_every_engine() {
    let page = next_data_page();
    let ir = next_data_ir();
    let want = run("scraper", &ir, &page);
    assert_eq!(want["category"], json!("Electronics"));
    assert_eq!(want["skus"].as_array().unwrap().len(), 40);
    assert_eq!(want["skus"][39], json!("SKU-39"));
    assert_eq!(want["name3"], json!("Product number 3"));
    assert_eq!(want["missing"], json!(null));
    assert_eq!(run("lite", &ir, &page), want);
    assert_eq!(run("stream", &ir, &page), want);
    assert!(plan("lite", &ir).explain().contains("json_path"));
}

#[test]
fn json_path_reports_the_same_errors_as_step_by_step_navigation() {
    let page = r#"<script id="d">{"a":"text","l":[1,2]}</script>"#;
    // key on a string / index on a map
    let key_on_string = json!({"object":{"v":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"a"},{"key":"b"}]}}});
    let at_on_map = json!({"object":{"v":{"pipe":[{"css":"#d"},"text","parse_json",{"at":0}]}}});
    for engine in ["scraper", "lite", "stream"] {
        let e1 = plan(engine, &key_on_string).extract(page, Value::Null).unwrap_err().to_string();
        assert!(e1.contains("key: expected a node, got string"), "{engine}: {e1}");
        let e2 = plan(engine, &at_on_map).extract(page, Value::Null).unwrap_err().to_string();
        assert!(e2.contains("at: cannot index a map"), "{engine}: {e2}");
    }
    // text that is not JSON at all
    let broken = json!({"object":{"v":{"pipe":[{"css":"#d"},"text","parse_json",{"key":"a"}]}}});
    for engine in ["scraper", "lite", "stream"] {
        let e = plan(engine, &broken).extract("<script id=d>{oops</script>", Value::Null).unwrap_err().to_string();
        assert!(e.contains("parse_json"), "{engine}: {e}");
    }
}

// ---- documented scope: a record's selectors see ancestors outside the record ------------------------------------------

#[test]
fn field_selectors_use_query_selector_all_scope_in_every_engine() {
    // `.header .title` inside a card matches because the page-level `.header` is an ancestor of the card.
    // Same rule in scraper, lite and stream (it is `querySelectorAll` semantics, see docs/archive/audit_v7_v8.md).
    let html = r#"<div class="header"><div class="card"><span class="title">T1</span></div><div class="card"><span class="title">T2</span></div></div>"#;
    let ir = json!({"object":{"v":{"pipe":[{"css":".card"},{"map":{"object":{"t":{"pipe":[{"css":".header .title"},"text"]}}}}]}}});
    let want = json!({"v": [{"t": "T1"}, {"t": "T2"}]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &ir, html), want, "{engine}");
    }
}

// ---- v8.1: markdown, :scope, br, adaptive lists ----------------------------------------------------------------------

#[test]
fn markdown_renders_the_common_structure_on_every_dom_engine() {
    let html = r#"<body><article><h1> Title </h1><p>Some <b>bold</b> and <a href="/x">a link</a><script>var x=1;</script></p>
<ul><li>one</li><li>two <i>it</i></li></ul><div class="ad">BUY</div>
<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table></article></body>"#;
    let ir = json!({"object":{"md":{"pipe":[{"css":"article"},{"markdown":{"without":".ad"}}]}}});
    let want = "# Title\n\nSome **bold** and [a link](/x)\n\n- one\n- two *it*\n\n| A | B |\n| --- | --- |\n| 1 | 2 |";
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &ir, html), json!({"md": want}), "{engine}");
    }
    // not streamable: `auto` falls back to a DOM engine
    assert_ne!(plan("auto", &ir).backend(), "stream");
}

#[test]
fn scope_pseudo_class_isolates_a_record_in_lite() {
    let html = r#"<div class="header"><div class="card"><span class="title">T1</span><b><span class="title">deep</span></b></div><div class="card"><span class="title">T2</span></div></div>"#;
    let ir = json!({"object":{"v":{"pipe":[{"css":".card"},{"map":{"object":{
        "outer": {"pipe":[{"css":".header .title"},"text"]},
        "scoped":{"pipe":[{"css":":scope > .title"},"text"]},
        "none":  {"pipe":[{"css":":scope .header .title"},"text"]}
    }}}]}}});
    let got = run("lite", &ir, html);
    assert_eq!(got["v"][0]["outer"], json!("T1"));
    assert_eq!(got["v"][0]["scoped"], json!("T1")); // direct child only, not the nested "deep"
    assert_eq!(got["v"][0]["none"], json!(null)); // the page-level `.header` is outside the scope
    assert_eq!(got["v"][1]["scoped"], json!("T2"));
    // v8.2: the stream engine knows `:scope` too (the open record while its fields are matched)
    assert_eq!(run("stream", &ir, html), got);
    assert_eq!(plan("auto", &ir).backend(), "stream");
    // scraper does not know `:scope` as "the record": refuse instead of answering differently
    assert!(compile(&Expr::from_json(&ir.to_string()).unwrap(), "scraper").is_err());
    // `html` (best DOM engine) falls back to `lite` for such a schema
    assert_eq!(run("html", &ir, html), got);
    assert_eq!(plan("html", &ir).backend(), "lite");
}

#[test]
fn br_option_keeps_line_breaks_in_text_on_every_engine() {
    let html = "<div class=d><p>Screen: OLED<br>Rate: <b>120</b> Hz<br/>Done</p></div><div class=d><p> x <br> y </p></div>";
    let one = json!({"object":{"v":{"pipe":[{"css":"p"},{"text_opts":{"sep":" ","strip":true,"br":"\n"}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &one, html), json!({"v": "Screen: OLED\nRate: 120 Hz\nDone"}), "{engine}");
    }
    let list = json!({"object":{"v":{"pipe":[{"css":".d"},{"map":{"pipe":[{"css":"p"},{"text_opts":{"sep":" ","strip":true,"br":"\n"}}]}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &list, html), json!({"v": ["Screen: OLED\nRate: 120 Hz\nDone", "x\ny"]}), "{engine}");
    }
    // text(sep, strip) without `without` / `br` is streamed now
    let plain = json!({"object":{"v":{"pipe":[{"css":"p"},{"text_opts":{"sep":"|","strip":true}}]}}});
    assert_eq!(plan("auto", &plain).backend(), "stream");
    let want = run("scraper", &plain, html);
    assert_eq!(run("lite", &plain, html), want);
    assert_eq!(run("stream", &plain, html), want);
}

#[test]
fn adaptive_before_map_relocates_the_whole_list_and_keeps_samples() {
    let before = r#"<div id="shop"><div class="product"><h2>A</h2><span class="p">$19.90</span></div>
<div class="product"><h2>B</h2><span class="p">$149.00</span></div><div class="product"><h2>C</h2><span class="p">$5.00</span></div></div>"#;
    let after = r#"<div id="shop"><div class="item-x"><h2>A</h2><span class="price-x">$19.90</span></div>
<div class="item-x"><h2>B</h2><span class="price-x">$149.00</span></div><div class="item-x"><h2>C</h2><span class="price-x">$5.00</span></div></div>"#;
    let ir = json!({"object":{"cards":{"pipe":[{"adaptive":{"css":".product","key":"cards"}},{"map":{"object":{
        "name":{"pipe":[{"css":"h2"},"text"]},
        "price":{"pipe":[{"css":"span"},"text"]}
    }}}]}}});
    for engine in ["scraper", "lite"] {
        let p = plan(engine, &ir);
        let first = p.extract(before, Value::Null).unwrap().to_json();
        assert_eq!(first["cards"].as_array().unwrap().len(), 3, "{engine}");
        let healed = p.extract(after, Value::Null).unwrap().to_json();
        assert_eq!(healed, first, "{engine}: the redesigned list is found again, all of it");
    }
}

// ---- v8.2: :scope in stream, <base href>, js_json cache, store export --------------------------------------------------

#[test]
fn stream_scope_follows_the_open_record_also_when_records_nest() {
    // nested cards: each record's `:scope > .t` must see its own direct child only
    let html = r#"<div class="c" id="o"><span class="t">outer</span><div class="c" id="i"><span class="t">inner</span></div></div>"#;
    let ir = json!({"object":{"v":{"pipe":[{"css":".c"},{"map":{"object":{
        "id":{"pipe":[{"attr":"id"}]},
        "t":{"pipe":[{"css":":scope > .t"},"text"]}
    }}}]}}});
    let want = run("lite", &ir, html);
    assert_eq!(want, json!({"v": [{"id": "o", "t": "outer"}, {"id": "i", "t": "inner"}]}));
    assert_eq!(run("stream", &ir, html), want);
    // top level: `:scope` is the root element
    let top = json!({"object":{"v":{"pipe":[{"css":":scope > .t"},"text"]},"w":{"pipe":[{"css":":scope .t"},"text"]}}});
    let page = r#"<div class="r"><span class="t">A</span><p><span class="t">B</span></p></div>"#;
    assert_eq!(run("stream", &top, page), run("lite", &top, page));
    assert_eq!(run("stream", &top, page), json!({"v": "A", "w": "A"}));
}

#[test]
fn url_uses_the_documents_base_href_unless_meta_overrides_it() {
    let html = r#"<html><head><base href="https://shop.example/catalog/"><base href="https://ignored.example/"></head>
<body><a href="item?id=1">x</a><a href="/root">y</a><a href="https://abs.example/z">z</a></body></html>"#;
    let ir = json!({"object":{"v":{"pipe":[{"css":"a"},{"map":{"pipe":[{"attr":"href"},{"resolve_url":{}}]}}]}}});
    let want = json!({"v": ["https://shop.example/catalog/item?id=1", "https://shop.example/root", "https://abs.example/z"]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &ir, html), want, "{engine}");
    }
    // an absolute `<base href>` stays what it is, with or without the page URL in `meta`
    for engine in ["scraper", "lite", "stream"] {
        let got = plan(engine, &ir).extract(html, Value::from_json(json!({"base_url": "https://other.example/dir/page"}))).unwrap().to_json();
        assert_eq!(got["v"][0], json!("https://shop.example/catalog/item?id=1"), "{engine}");
    }
    // a relative `<base href>` is resolved against the page URL (`meta["base_url"]`) first
    let rel = r#"<head><base href="/shop/"></head><body><a href="123">x</a></body>"#;
    let one_link = json!({"object":{"v":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        let got = plan(engine, &one_link).extract(rel, Value::from_json(json!({"base_url": "https://site.com/catalog/item"}))).unwrap().to_json();
        assert_eq!(got, json!({"v": "https://site.com/shop/123"}), "{engine}");
        // meta alone, without a <base>
        let plain = plan(engine, &one_link).extract("<a href='123'>x</a>", Value::from_json(json!({"base_url": "https://site.com/catalog/item"}))).unwrap().to_json();
        assert_eq!(plain, json!({"v": "https://site.com/catalog/123"}), "{engine}");
    }
    // an explicit base in the schema overrides everything
    let explicit = json!({"object":{"v":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{"base":{"lit":"https://x.example/a/"}}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &explicit, html), json!({"v": "https://x.example/a/item?id=1"}), "{engine}");
    }
    // a single field: the stream engine must not stop before it has seen the whole head
    let one = json!({"object":{"v":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{}}]}}});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &one, html), json!({"v": "https://shop.example/catalog/item?id=1"}), "{engine}");
    }
    // no <base>, no meta: an error that says so
    let err = plan("lite", &one).extract("<a href='/x'>x</a>", Value::Null).unwrap_err().to_string();
    assert!(err.contains("no base URL"), "{err}");
}

#[test]
fn js_json_is_parsed_once_for_several_fields_and_stays_correct() {
    let items: Vec<String> = (0..60).map(|i| format!(r#"{{"id":{i},"name":"Item number {i}"}}"#)).collect();
    let script = format!(r#"<script>window.__STATE__ = {{"user":{{"name":"Ann"}},"items":[{}],"n":60}}; // trailing</script>"#, items.join(","));
    let page = format!("<html><body>{script}</body></html>");
    let field = |tail: serde_json::Value| {
        let mut ops = vec![json!({"css":"script"}), json!("text"), json!({"js_json":{"var":"window.__STATE__"}})];
        ops.extend(tail.as_array().unwrap().clone());
        json!({"pipe": ops})
    };
    let ir = json!({"object":{
        "user": field(json!([{"key":"user"},{"key":"name"}])),
        "count": field(json!([{"key":"n"}])),
        "third": field(json!([{"key":"items"},{"at":2},{"key":"name"}]))
    }});
    let want = json!({"user": "Ann", "count": 60, "third": "Item number 2"});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &ir, &page), want, "{engine}");
    }
}

#[test]
fn adaptive_store_exports_and_imports_without_files() {
    let before = r#"<div id="s"><div class="product"><h2>A</h2><i>1</i></div><div class="product"><h2>B</h2><i>2</i></div></div>"#;
    let after = r#"<div id="s"><div class="item-x"><h2>A</h2><i>1</i></div><div class="item-x"><h2>B</h2><i>2</i></div></div>"#;
    let ir = json!({"object":{"c":{"pipe":[{"adaptive":{"css":".product","key":"cards"}},{"map":{"object":{
        "n":{"pipe":[{"css":"h2"},"text"]}
    }}}]}}});
    let ex = Expr::from_json(&ir.to_string()).unwrap();
    let learned = compile(&ex, "lite").unwrap();
    let first = learned.extract(before, Value::Null).unwrap().to_json();
    let state = learned.adaptive_store().export_json();
    assert!(state.contains("\"cards\""), "{state}");
    assert!(state.contains("\"samples\""), "{state}");

    // a fresh plan (another process, another Lambda invocation) given the exported state heals the redesigned page
    let store = std::sync::Arc::new(oxide_parse_core::MemoryStore::from_json(&state).unwrap());
    let fresh = oxide_parse_core::compile_with(&ex, "lite", Some(store.clone())).unwrap();
    assert_eq!(fresh.extract(after, Value::Null).unwrap().to_json(), first);
    // `to_json` / `from_json` round trip
    assert_eq!(oxide_parse_core::MemoryStore::from_json(&store.to_json()).unwrap().to_json(), store.to_json());
    // merging into a running plan
    let other = compile(&ex, "lite").unwrap();
    assert_eq!(other.adaptive_store().import_json(&state).unwrap(), 1);
    assert!(oxide_parse_core::MemoryStore::from_json("not json").is_err());
}

#[test]
fn stream_stops_early_with_url_once_the_head_is_behind_it() {
    // the link sits right after <body> opens, 200 KB of text follows: the answer must still be correct
    let big = "x ".repeat(100_000);
    let html = format!(r#"<html><head><base href="https://b.example/p/"></head><body><a href="q">l</a><p>{big}</p></body></html>"#);
    let ir = json!({"object":{"v":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{}}]}}});
    for engine in ["lite", "stream"] {
        assert_eq!(run(engine, &ir, &html), json!({"v": "https://b.example/p/q"}), "{engine}");
    }
    // <base> after the first link but still in the head is honoured as well (no early exit before <body>)
    let late = r#"<html><head><a href="q">l</a><base href="https://late.example/"></head><body></body></html>"#;
    for engine in ["lite", "stream"] {
        assert_eq!(run(engine, &ir, late), json!({"v": "https://late.example/q"}), "{engine}");
    }
}

#[test]
fn follow_hands_its_url_down_as_the_base_of_the_followed_page() {
    let ir = json!({"object":{"p":{"pipe":[{"follow":{
        "request":{"pipe":[{"css":"a"},{"attr":"href"}]},
        "schema":{"object":{"l":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{}}]}}}
    }}]}}});
    let e = compile(&Expr::from_json(&ir.to_string()).unwrap(), "lite").unwrap();
    let mut s = e.start("<a href='https://site.example/cat/item'>go</a>", Value::Null).unwrap();
    let (id, base) = {
        let reqs = s.pending();
        assert_eq!(reqs.len(), 1);
        (reqs[0].id, reqs[0].meta.to_json())
    };
    assert_eq!(base, json!({"base_url": "https://site.example/cat/item"}));
    let doc = "<a href='next'>n</a>".to_string();
    e.resume(&mut s, &[(id, doc)]).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"p": {"l": "https://site.example/cat/next"}}));
}

#[test]
fn adaptive_samples_of_a_record_hold_up_to_three_values() {
    let page = r#"<div id="s"><div class="product"><b>Sale</b><h2>iPhone 16</h2><i>$999</i><u>extra</u></div></div>"#;
    let ir = json!({"object":{"c":{"pipe":[{"adaptive":{"css":".product","key":"cards"}},{"map":{"object":{
        "badge":{"pipe":[{"css":"b"},"text"]},"name":{"pipe":[{"css":"h2"},"text"]},
        "price":{"pipe":[{"css":"i"},"text"]},"more":{"pipe":[{"css":"u"},"text"]}
    }}}]}}});
    let e = compile(&Expr::from_json(&ir.to_string()).unwrap(), "lite").unwrap();
    e.extract(page, Value::Null).unwrap();
    let state: serde_json::Value = serde_json::from_str(&e.adaptive_store().export_json()).unwrap();
    assert_eq!(state["cards"]["samples"], json!(["Sale", "iPhone 16", "$999"]));
}

// ---- v8.3: streamed text(without), group_by, json_ld, meta_tags -------------------------------------------------------

#[test]
fn stream_text_without_matches_the_dom_engines() {
    let html = r#"<article><h1>Title</h1><p>Intro <b class="badge">NEW</b> text.</p><div class="ad">BUY <i>now</i></div>
<script>var x = 1;</script><p>Second <span class="ad">promo</span> paragraph<br>line</p></article>"#;
    for without in [".ad", ".ad, .badge", "b, i, .ad", "p .ad"] {
        let ir = json!({"object":{"t":{"pipe":[{"css":"article"},{"text_opts":{"sep":" ","strip":true,"without":without}}]}}});
        let want = run("scraper", &ir, html);
        assert_eq!(run("lite", &ir, html), want, "lite {without}");
        assert_eq!(run("stream", &ir, html), want, "stream {without}");
        assert_eq!(plan("auto", &ir).backend(), "stream", "{without}");
    }
    // per record, and together with `br`
    let list = json!({"object":{"v":{"pipe":[{"css":".card"},{"map":{"pipe":[{"text_opts":{"sep":"","strip":false,"without":".x"}}]}}]}}});
    let cards = r#"<div class="card">a<span class="x">HIDE<b>deep</b></span>b</div><div class="card"><span class="x">only</span>c</div>"#;
    let want = run("scraper", &list, cards);
    assert_eq!(want, json!({"v": ["ab", "c"]}));
    assert_eq!(run("stream", &list, cards), want);
    let br = json!({"object":{"t":{"pipe":[{"css":"p"},{"text_opts":{"sep":" ","strip":true,"without":".ad","br":"\n"}}]}}});
    let two = r#"<p>a<span class="ad">x<br>y</span><br>b</p>"#;
    assert_eq!(run("stream", &br, two), run("lite", &br, two));
    assert_eq!(run("stream", &br, two), json!({"t": "a\nb"}));
}

#[test]
fn stream_without_uses_the_capture_scope_and_excludes_matching_br() {
    let html = r#"<main><p>a<br class="ad">b<span class="ad">hidden</span><i><span class="ad">keep</span></i><br>c</p></main>"#;
    let ir = json!({"object":{
        "scoped":{"pipe":[{"css":"p"},{"text_opts":{"without":":scope > .ad","br":"\n"}}]},
        "all":{"pipe":[{"css":"p"},{"text_opts":{"without":".ad","br":"\n"}}]}
    }});
    let want = json!({"scoped":"abkeep\nc","all":"ab\nc"});
    for engine in ["lite", "stream"] {
        assert_eq!(run(engine, &ir, html), want, "{engine}");
    }
}

const SPECS: &str = r#"<div class="specs"><p>intro</p>
<h3>Display</h3><a href="/d1">d1</a><p>text <a href="/d2">d2</a></p>
<h3>Battery</h3><a href="/b1">b1</a>
<h3>Empty</h3>
</div>"#;

#[test]
fn group_by_gives_each_header_its_run_of_siblings() {
    let ir = json!({"object":{"g":{"pipe":[{"css":".specs"},{"group_by":{"header":"h3"}},{"map":{"object":{
        "title":{"pipe":["first","text"]},
        "links":{"pipe":[{"css":"a"},{"map":{"pipe":[{"attr":"href"}]}}]},
        "n":{"pipe":[{"css":"a"},"count"]}
    }}}]}}});
    let want = json!({"g": [
        {"title": "Display", "links": ["/d1", "/d2"], "n": 2},
        {"title": "Battery", "links": ["/b1"], "n": 1},
        {"title": "Empty", "links": [], "n": 0}
    ]});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &ir, SPECS), want, "{engine}");
    }
    // `items` narrows the members: only `<p>` blocks, so the bare `<a>` siblings are left out
    let only_p = json!({"object":{"g":{"pipe":[{"css":".specs"},{"group_by":{"header":"h3","items":"p"}},{"map":{"object":{
        "title":{"pipe":["first","text"]},
        "links":{"pipe":[{"css":"a"},{"map":{"pipe":[{"attr":"href"}]}}]}
    }}}]}}});
    for engine in ["scraper", "lite"] {
        let got = run(engine, &only_p, SPECS);
        assert_eq!(got["g"][0]["links"], json!(["/d2"]), "{engine}");
        assert_eq!(got["g"][1]["links"], json!([]), "{engine}");
    }
    // without `map` it is an error that says what to do
    let bare = json!({"object":{"g":{"pipe":[{"css":".specs"},{"group_by":{"header":"h3"}}]}}});
    let err = plan("lite", &bare).extract(SPECS, Value::Null).unwrap_err().to_string();
    assert!(err.contains("map"), "{err}");
    // `on_error="skip"` works on groups too
    let safe = json!({"object":{"g":{"pipe":[{"css":".specs"},{"group_by":{"header":"h3"}},{"map_opts":{"on_error":"skip","body":{"pipe":["first","text","int"]}}}]}}});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &safe, SPECS), json!({"g": []}), "{engine}");
    }
}

const LD_PAGE: &str = r#"<head>
<script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"Product","name":"Phone","offers":{"price":"99"}},{"@type":["Thing","Brand"],"name":"Acme"}]}</script>
<script type="application/ld+json">[{"@type":"https://schema.org/Product","name":"Tablet"},{"@type":"Person","name":"Ann"}]</script>
<script type="application/ld+json">{ broken json </script>
<script type="application/ld+json"><!-- {"@type":"Organization","name":"Org"} --></script>
<script type="text/javascript">var ignored = 1;</script>
<meta property="og:title" content="Hello">
<meta property="og:image" content="a.png"><meta property="og:image" content="b.png">
<meta name="twitter:card" content="summary"><meta name="description" content="plain">
<meta property="OG:Locale" content="en"><meta property="og:empty">
</head>"#;

#[test]
fn json_ld_unfolds_graphs_and_filters_by_type() {
    let all = json!({"object":{"v":{"pipe":[{"json_ld":{}}, {"map":{"pipe":[{"key":"name"}]}}]}}});
    let product = json!({"object":{"v":{"pipe":[{"json_ld":{"schema_type":"Product"}}, {"map":{"pipe":[{"key":"name"}]}}]}}});
    let brand = json!({"object":{"v":{"pipe":[{"json_ld":{"schema_type":"Brand"}}, {"map":{"pipe":[{"key":"name"}]}}]}}});
    let first = json!({"object":{"price":{"pipe":[{"json_ld":{"schema_type":"Product"}},{"at":0},{"key":"offers"},{"key":"price"}]}}});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &all, LD_PAGE), json!({"v": ["Phone", "Acme", "Tablet", "Ann", "Org"]}), "{engine}");
        assert_eq!(run(engine, &product, LD_PAGE), json!({"v": ["Phone", "Tablet"]}), "{engine}");
        assert_eq!(run(engine, &brand, LD_PAGE), json!({"v": ["Acme"]}), "{engine}");
        assert_eq!(run(engine, &first, LD_PAGE), json!({"price": "99"}), "{engine}");
        // no JSON-LD at all: an empty list, not an error
        assert_eq!(run(engine, &all, "<p>x</p>"), json!({"v": []}), "{engine}");
    }
}

#[test]
fn json_ld_checks_media_type_instead_of_substring() {
    let html = r#"<script type="text/not-ld+json">{"name":"ignored"}</script>
    <script type="Application/LD+JSON; charset=utf-8">{"name":"kept"}</script>"#;
    let ir = json!({"object":{"v":{"pipe":[{"json_ld":{}}]}}});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &ir, html), json!({"v":[{"name":"kept"}]}), "{engine}");
    }
}

#[test]
fn meta_tags_collects_opengraph_and_twitter_into_a_dict() {
    let og = json!({"object":{"m":{"pipe":[{"meta_tags":{"prefixes":["og:","twitter:"]}}]}}});
    let want = json!({"m": {"og:title": "Hello", "og:image": ["a.png", "b.png"], "twitter:card": "summary", "og:locale": "en"}});
    let all = json!({"object":{"m":{"pipe":[{"meta_tags":{"prefixes":[]}}]}}});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &og, LD_PAGE), want, "{engine}");
        let everything = run(engine, &all, LD_PAGE);
        assert_eq!(everything["m"]["description"], json!("plain"), "{engine}");
    }
}

// ---- v8.4: one Session loop for follow fetches and repair requests ------------------------------------------------------

fn answer(k: &str, v: &str) -> oxide_parse_core::IndexMap<String, String> {
    let mut m = oxide_parse_core::IndexMap::new();
    m.insert(k.to_string(), v.to_string());
    m
}

fn price_ir() -> serde_json::Value {
    json!({"object":{"price":{"pipe":[{"adaptive":{"css":".price","key":"price","threshold":0.99}},"first","text"]}}})
}

const GOOD: &str = r#"<aside class="unrelated-sidebar">ads</aside><div id="shop" class="catalog"><span class="price">$5</span></div>"#;
const BROKEN: &str = r#"<aside class="unrelated-sidebar">ads</aside><div id="shop" class="catalog"><section class="cost-box"><em>5 USD</em></section></div>"#;

#[test]
fn a_broken_selector_becomes_a_repair_request_with_a_local_skeleton() {
    let e = compile(&Expr::from_json(&price_ir().to_string()).unwrap(), "lite").unwrap();
    assert_eq!(e.extract(GOOD, Value::Null).unwrap().to_json(), json!({"price": "$5"}));

    let mut s = e.start(BROKEN, Value::Null).unwrap();
    assert!(s.pending().is_empty());
    assert!(s.is_done(), "repairs are optional: no fetch is pending");
    let reps = s.repairs();
    assert_eq!(reps.len(), 1);
    let (id, rep) = (reps[0].id, reps[0].clone());
    assert_eq!((rep.key.as_str(), rep.css.as_str(), rep.doc), ("price", ".price", None));
    assert_eq!(rep.scope.as_deref(), Some("container"));
    assert_eq!(rep.container.as_deref(), Some("div#shop.catalog"));
    let sk = rep.skeleton.clone().unwrap();
    assert!(sk.contains("cost-box"), "{sk}");
    assert!(!sk.contains("unrelated-sidebar"), "the skeleton is the container, not the page: {sk}");
    assert_eq!(rep.fingerprint.as_ref().unwrap().samples, vec!["$5".to_string()]);

    e.repair(&mut s, &[(id, answer("price", "em"))]).unwrap();
    assert!(s.repairs().is_empty());
    assert_eq!(s.overrides().get("price").map(String::as_str), Some("em"));
    assert!(s.misses().is_empty());
    assert_eq!(s.take_result().unwrap().to_json(), json!({"price": "5 USD"}));
    // the element the new selector found is the new reference
    assert!(e.adaptive_store().export_json().contains("\"tag\":\"em\""));
}

#[test]
fn declining_or_a_bad_answer_leaves_the_session_usable() {
    let e = compile(&Expr::from_json(&price_ir().to_string()).unwrap(), "lite").unwrap();
    e.extract(GOOD, Value::Null).unwrap();

    // a selector that does not parse is not an error: the request stays open and carries the parser's message,
    // the second invalid answer declines it
    let mut s = e.start(BROKEN, Value::Null).unwrap();
    let id = s.repairs()[0].id;
    e.repair(&mut s, &[(id, answer("price", "[[["))]).unwrap();
    assert_eq!(s.repairs().len(), 1);
    assert_eq!(s.repairs()[0].id, id);
    assert!(s.repairs()[0].previous_error.as_deref().unwrap_or("").contains("price"));
    assert!(s.overrides().is_empty());
    // an answer for an unknown id is an error
    assert!(e.repair(&mut s, &[(9999, answer("price", "em"))]).is_err());
    e.repair(&mut s, &[(id, answer("price", "div >"))]).unwrap();
    assert!(s.repairs().is_empty(), "the second invalid answer declines the request");
    assert_eq!(s.misses().len(), 1);
    let mut s = e.start(BROKEN, Value::Null).unwrap();
    let id = s.repairs()[0].id;

    // declining closes the request and the result simply has no price
    e.repair(&mut s, &[(id, oxide_parse_core::IndexMap::new())]).unwrap();
    assert!(s.repairs().is_empty());
    assert_eq!(s.misses().len(), 1);
    assert_eq!(s.take_result().unwrap().to_json(), json!({"price": null}));
}

#[test]
fn followed_documents_are_repaired_in_the_same_loop() {
    let sub = json!({"object":{"price":{"pipe":[{"adaptive":{"css":".price","key":"price","threshold":0.99}},"first","text"]}}});
    let ir = json!({"object":{"items":{"pipe":[{"css":"a"},{"map":{"pipe":[{"follow":{
        "request":{"pipe":[{"attr":"href"}]},"schema": sub
    }}]}}]}}});
    let e = compile(&Expr::from_json(&ir.to_string()).unwrap(), "lite").unwrap();
    let mut s = e.start(r#"<a href="/p1">1</a><a href="/p2">2</a>"#, Value::Null).unwrap();
    assert!(s.repairs().is_empty());
    let ids: Vec<u64> = s.pending().iter().map(|r| r.id).collect();
    assert_eq!(ids.len(), 2);
    e.resume(&mut s, &[(ids[0], r#"<div class="price">$5</div>"#.to_string()), (ids[1], r#"<div class="cost">7</div>"#.to_string())]).unwrap();
    assert!(s.is_done());
    let reps: Vec<_> = s.repairs().into_iter().cloned().collect();
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].doc, Some(ids[1]));
    assert_eq!(reps[0].url.as_deref(), Some("/p2"));
    e.repair(&mut s, &[(reps[0].id, answer("price", ".cost"))]).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"items": [{"price": "$5"}, {"price": "7"}]}));
}

#[test]
fn repairing_the_root_keeps_the_ids_of_requests_in_flight() {
    let ir = json!({"object":{
        "title":{"pipe":[{"adaptive":{"css":"h1.t","key":"title","threshold":0.99}},"first","text"]},
        "next":{"pipe":[{"follow":{"request":{"pipe":[{"css":"a"},{"attr":"href"}]},
            "schema":{"object":{"x":{"pipe":[{"css":"b"},"text"]}}}}}]}
    }});
    let e = compile(&Expr::from_json(&ir.to_string()).unwrap(), "lite").unwrap();
    // learn the reference on a good page
    let _ = e.start(r#"<h1 class="t">Hi</h1><a href="/n">n</a>"#, Value::Null).unwrap();

    let mut s = e.start(r#"<h2 class="t2">Hi</h2><a href="/n">n</a>"#, Value::Null).unwrap();
    let fetch_id = s.pending()[0].id;
    let rep_id = s.repairs()[0].id;
    e.repair(&mut s, &[(rep_id, answer("title", "h2"))]).unwrap();
    // the repeated follow request is the same request: a fetch already under way stays valid
    assert_eq!(s.pending().len(), 1);
    assert_eq!(s.pending()[0].id, fetch_id);
    e.resume(&mut s, &[(fetch_id, "<b>X</b>".to_string())]).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"title": "Hi", "next": {"x": "X"}}));
}

#[test]
fn several_broken_fields_share_one_distilled_container() {
    let ir = json!({"object":{"a":{"pipe":[{"adaptive":{"css":".a","key":"a","threshold":0.99}},"first","text"]},
        "b":{"pipe":[{"adaptive":{"css":".b","key":"b","threshold":0.99}},"first","text"]}}});
    let good = r#"<div id="card" class="box"><i class="a">1</i><i class="b">2</i></div>"#;
    let bad = r#"<div id="card" class="box"><u>1</u><s>2</s></div>"#;
    let e = compile(&Expr::from_json(&ir.to_string()).unwrap(), "lite").unwrap();
    e.extract(good, Value::Null).unwrap();
    let s = e.start(bad, Value::Null).unwrap();
    let reps = s.repairs();
    assert_eq!(reps.len(), 2);
    assert_eq!(reps[0].skeleton, reps[1].skeleton);
    assert_eq!(reps[0].container.as_deref(), Some("div#card.box"));
}

#[test]
fn meta_tags_values_mode_and_unresolved_follow_is_an_error() {
    let page = r#"<meta property="og:image" content="a.png"><meta property="og:image" content="b.png"><meta property="og:title" content="T">"#;
    let mk = |v: &str| json!({"object":{"m":{"pipe":[{"meta_tags":{"prefixes":["og:"],"values":v}}]}}});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &mk("first"), page), json!({"m": {"og:image": "a.png", "og:title": "T"}}), "{engine}");
        assert_eq!(run(engine, &mk("list"), page), json!({"m": {"og:image": ["a.png", "b.png"], "og:title": ["T"]}}), "{engine}");
        assert_eq!(run(engine, &mk("auto"), page), json!({"m": {"og:image": ["a.png", "b.png"], "og:title": "T"}}), "{engine}");
    }
    assert!(compile(&Expr::from_json(&mk("nope").to_string()).unwrap(), "lite").is_err());
}

// ---- v8.5: slice / rest, distill, streamed meta_tags ---------------------------------------------------------------------

#[test]
fn slice_and_rest_follow_python_rules_on_every_value_kind() {
    let html = "<ul><li>a</li><li>b</li><li>c</li><li>d</li></ul>";
    let texts = |start: serde_json::Value, stop: serde_json::Value| {
        let mut spec = serde_json::Map::new();
        if !start.is_null() {
            spec.insert("start".into(), start);
        }
        if !stop.is_null() {
            spec.insert("stop".into(), stop);
        }
        json!({"object":{"v":{"pipe":[{"css":"li"},{"slice":spec},{"map":{"pipe":["text"]}}]}}})
    };
    for engine in ["scraper", "lite"] {
        let go = |a: serde_json::Value, b: serde_json::Value| run(engine, &texts(a, b), html)["v"].clone();
        assert_eq!(go(json!(1), json!(null)), json!(["b", "c", "d"]), "{engine} rest");
        assert_eq!(go(json!(null), json!(2)), json!(["a", "b"]), "{engine} head");
        assert_eq!(go(json!(-2), json!(null)), json!(["c", "d"]), "{engine} tail");
        assert_eq!(go(json!(1), json!(-1)), json!(["b", "c"]), "{engine} middle");
        assert_eq!(go(json!(3), json!(1)), json!([]), "{engine} crossed bounds are empty");
        assert_eq!(go(json!(10), json!(20)), json!([]), "{engine} out of range");
        assert_eq!(go(json!(-99), json!(99)), json!(["a", "b", "c", "d"]), "{engine} clamped");
    }
    // strings count characters, lists are lists, `null` stays `null`
    let html2 = "<ul><li>a</li><li>b</li></ul><b>привет</b><i>[1,2,3,4]</i>";
    let ir = json!({"object":{
        "s":{"pipe":[{"css":"li"},"first","text",{"slice":{"start":-1}}]},
        "u":{"pipe":[{"css":"b"},"first","text",{"slice":{"start":1,"stop":4}}]},
        "l":{"pipe":[{"css":"i"},"first","text","parse_json",{"slice":{"stop":2}}]},
        "n":{"pipe":[{"css":".nope"},"first","text",{"slice":{"start":1}}]}
    }});
    let want = json!({"s": "a", "u": "рив", "l": [1, 2], "n": null});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &ir, html2), want, "{engine}");
    }
    // the children of a single node
    let kids = json!({"object":{"v":{"pipe":[{"css":"ul"},"first",{"slice":{"start":2}},{"map":{"pipe":["text"]}}]}}});
    assert!(compile(&Expr::from_json(&kids.to_string()).unwrap(), "stream").is_err());
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &kids, html), json!({"v": ["c", "d"]}), "{engine}");
    }
}

#[test]
fn rest_drops_the_header_of_a_group() {
    let ir = json!({"object":{"g":{"pipe":[{"css":".specs"},{"group_by":{"header":"h3"}},{"map":{"object":{
        "section":{"pipe":["first","text","strip"]},
        "items":{"pipe":[{"slice":{"start":1}},{"map":{"pipe":["text","strip"]}}]}
    }}}]}}});
    let html = "<div class=specs><h3> A </h3><p>one</p><p>two</p><h3>B</h3><p>three</p><h3>C</h3></div>";
    let want = json!({"g": [{"section": "A", "items": ["one", "two"]}, {"section": "B", "items": ["three"]}, {"section": "C", "items": []}]});
    for engine in ["scraper", "lite"] {
        assert_eq!(run(engine, &ir, html), want, "{engine}");
    }
}

#[test]
fn distill_op_shrinks_one_element_inside_the_schema() {
    let items: String = (0..30).map(|i| format!(r#"<li class="item"><a href="/p/{i}" onclick="x()" data-id="{i}">Product {i}</a></li>"#)).collect();
    let html = format!(r#"<body><nav>menu</nav><main id="m"><script>var a=1;</script><ul>{items}</ul></main></body>"#);
    let ir = json!({"object":{"skel":{"pipe":[{"css":"main"},{"distill":{"keep":2,"max_text":40,"max_chars":4000}}]},
                              "none":{"pipe":[{"css":".nope"},{"distill":{}}]}}});
    for engine in ["scraper", "lite"] {
        let got = run(engine, &ir, &html);
        let sk = got["skel"].as_str().unwrap_or_else(|| panic!("{engine}: {got}"));
        assert!(sk.contains("id=\"m\""), "{engine}: {sk}");
        assert!(sk.contains("similar"), "{engine}: runs of siblings are folded: {sk}");
        assert!(!sk.contains("var a=1") && !sk.contains("onclick") && !sk.contains("menu"), "{engine}: {sk}");
        assert!(sk.len() < html.len() / 2, "{engine}");
        assert!(got["none"].is_null(), "{engine}");
    }
    // `lite` distils the parsed node directly, `scraper` goes through its HTML: same result
    assert_eq!(run("lite", &ir, &html), run("scraper", &ir, &html));
}

#[test]
fn stream_meta_tags_matches_the_dom_engines_and_stops_at_the_body() {
    let head = r#"<html><head><title>T</title>
<meta property="og:title" content="Hello &amp; bye"><meta property="og:image" content="a.png"><meta property="og:image" content="b.png">
<meta name="twitter:card" content="summary"><meta name="description" content="plain"><meta property="og:empty">
<meta property="OG:Locale" content="en"></head><body><p>x</p>"#;
    let big = "<p>filler text</p>".repeat(20_000);
    let page = format!("{head}{big}<meta property=\"og:late\" content=\"in body\"></body></html>");
    for values in ["auto", "first", "list"] {
        let ir = json!({"object":{
            "m":{"pipe":[{"meta_tags":{"prefixes":["og:","twitter:"],"values":values}}]},
            "all":{"pipe":[{"meta_tags":{"prefixes":[]}}]},
            "title":{"pipe":[{"meta_tags":{"prefixes":["og:"]}},{"key":"og:title"}]}
        }});
        // the DOM engines see `og:late` (it is in the document), the stream engine reads the head: compare on the head
        let want = run("lite", &ir, head);
        assert_eq!(run("scraper", &ir, head), want, "scraper {values}");
        assert_eq!(run("stream", &ir, head), want, "stream {values}");
        assert_eq!(plan("auto", &ir).backend(), "stream", "{values}");
    }
    let ir = json!({"object":{"m":{"pipe":[{"meta_tags":{"prefixes":["og:"]}}]}}});
    let got = run("stream", &ir, &page);
    assert_eq!(got["m"]["og:title"], json!("Hello & bye"));
    assert!(got["m"].get("og:late").is_none(), "the stream engine stops once the body has started");
    assert_eq!(run("lite", &ir, &page)["m"]["og:late"], json!("in body"));
    // one single-value capture next to it still works (title), and the plan says what it does
    let mixed = json!({"object":{"m":{"pipe":[{"meta_tags":{"prefixes":["og:"]}}]},"t":{"pipe":[{"css":"title"},"text"]}}});
    assert_eq!(run("stream", &mixed, head)["t"], json!("T"));
    assert!(plan("stream", &mixed).explain().contains("meta_tags"));
}

// ---- paginate: next-page loops in the state machine ------------------------------------------------------------------

fn paginate_ir(max_pages: Option<usize>, max_items: Option<usize>) -> serde_json::Value {
    let mut spec = serde_json::Map::new();
    spec.insert("items".into(), json!({"pipe":[{"css":".u"},{"map":{"pipe":["text"]}}]}));
    spec.insert("next".into(), json!({"pipe":[{"css":"a.next"},{"attr":"href"},{"resolve_url":{}}]}));
    if let Some(mp) = max_pages {
        spec.insert("max_pages".into(), json!(mp));
    }
    if let Some(mi) = max_items {
        spec.insert("max_items".into(), json!(mi));
    }
    json!({"object":{"friends":{"pipe":[{"paginate": spec}]}}})
}

fn drive<F>(e: &dyn Engine, src: &str, meta: Value, mut fetch: F) -> serde_json::Value
where
    F: FnMut(&oxide_parse_core::Request) -> std::result::Result<String, String>,
{
    let mut s = e.start(src, meta).unwrap();
    while !s.is_done() {
        let docs: Vec<(u64, String)> = s
            .pending()
            .into_iter()
            .map(|r| (r.id, fetch(r).unwrap()))
            .collect();
        e.resume(&mut s, &docs).unwrap();
    }
    s.take_result().unwrap().to_json()
}

#[test]
fn paginate_collects_pages_limits_cycles_and_post_ops() {
    let pages: std::collections::HashMap<&str, &str> = [
        ("https://ex.test/p1", r#"<b class="u">A</b><b class="u">B</b><a class="next" href="?p=2">n</a>"#),
        ("https://ex.test/p1?p=2", r#"<b class="u">C</b><b class="u">D</b><a class="next" href="?p=3">n</a>"#),
        ("https://ex.test/p1?p=3", r#"<b class="u">E</b><b class="u">F</b><a class="next" href="?p=4">n</a>"#),
        ("https://ex.test/p1?p=4", r#"<b class="u">G</b>"#),
    ]
    .into_iter()
    .collect();

    let full_ir = paginate_ir(None, None);
    // stream refuses paginate; auto falls back to DOM
    assert!(compile(&Expr::from_json(&full_ir.to_string()).unwrap(), "stream").is_err());
    assert_ne!(plan("auto", &full_ir).backend(), "stream");
    // schema without paginate keeps explain() free of paginate noise
    assert!(!plan("lite", &json!({"object":{"v":{"pipe":[{"css":"b"},"text"]}}})).explain().contains("paginate plans:"));

    for engine in ["lite", "scraper"] {
        let p = plan(engine, &full_ir);
        let mut seen_urls = Vec::new();
        let got = drive(
            &*p,
            pages["https://ex.test/p1"],
            Value::from_json(json!({"base_url": "https://ex.test/p1"})),
            |r| {
                let u = r.url().unwrap().to_string();
                seen_urls.push(u.clone());
                Ok(pages[u.as_str()].to_string())
            },
        );
        assert_eq!(got, json!({"friends": ["A", "B", "C", "D", "E", "F", "G"]}), "{engine}");
        assert_eq!(seen_urls, vec!["https://ex.test/p1?p=2", "https://ex.test/p1?p=3", "https://ex.test/p1?p=4"]);

        // max_pages=1 does not request page 2
        let p1 = plan(engine, &paginate_ir(Some(1), None));
        let mut fetched1 = 0;
        let got1 = drive(
            &*p1,
            pages["https://ex.test/p1"],
            Value::from_json(json!({"base_url": "https://ex.test/p1"})),
            |_| {
                fetched1 += 1;
                Ok(String::new())
            },
        );
        assert_eq!(got1, json!({"friends": ["A", "B"]}));
        assert_eq!(fetched1, 0);

        // max_pages=2 stops after page 2
        let p2 = plan(engine, &paginate_ir(Some(2), None));
        let mut urls2 = Vec::new();
        let got2 = drive(
            &*p2,
            pages["https://ex.test/p1"],
            Value::from_json(json!({"base_url": "https://ex.test/p1"})),
            |r| {
                let u = r.url().unwrap().to_string();
                urls2.push(u.clone());
                Ok(pages[u.as_str()].to_string())
            },
        );
        assert_eq!(got2, json!({"friends": ["A", "B", "C", "D"]}));
        assert_eq!(urls2, vec!["https://ex.test/p1?p=2"]);

        // max_items=5 fetches up to page 3, slices to 5, never requests page 4
        let p5 = plan(engine, &paginate_ir(None, Some(5)));
        let mut urls5 = Vec::new();
        let got5 = drive(
            &*p5,
            pages["https://ex.test/p1"],
            Value::from_json(json!({"base_url": "https://ex.test/p1"})),
            |r| {
                let u = r.url().unwrap().to_string();
                urls5.push(u.clone());
                Ok(pages[u.as_str()].to_string())
            },
        );
        assert_eq!(got5, json!({"friends": ["A", "B", "C", "D", "E"]}));
        assert_eq!(urls5, vec!["https://ex.test/p1?p=2", "https://ex.test/p1?p=3"]);

        // Cycle detection (p2 points back to base_url with #frag; href="#" is also ignored)
        let mut cyc_urls = Vec::new();
        let cyc = drive(
            &*p,
            r#"<b class="u">A</b><a class="next" href="https://ex.test/c2">n</a>"#,
            Value::from_json(json!({"base_url": "https://ex.test/c1"})),
            |r| {
                let u = r.url().unwrap().to_string();
                cyc_urls.push(u);
                Ok(r#"<b class="u">B</b><a class="next" href="https://ex.test/c1#top">loop</a>"#.to_string())
            },
        );
        assert_eq!(cyc, json!({"friends": ["A", "B"]}));
        assert_eq!(cyc_urls, vec!["https://ex.test/c2"]);

        let hash_stop = drive(
            &*p,
            r##"<b class="u">A</b><a class="next" href="#">stop</a>"##,
            Value::from_json(json!({"base_url": "https://ex.test/c1"})),
            |_| unreachable!("`#` must not fetch"),
        );
        assert_eq!(hash_stop, json!({"friends": ["A"]}));
    }

    // Composed follow -> paginate + trailing count(), plus meta propagation across pages
    let composed = json!({"object":{"count":{"pipe":[{"follow":{
        "request":{"pipe":[{"css":"a.f"},{"attr":"href"},{"resolve_url":{}}]},
        "meta":{"object":{"tag":{"pipe":[{"meta":"tag"}]}}},
        "schema":{"pipe":[{"paginate":{
            "items":{"pipe":[{"css":".u"},{"map":{"object":{"name":{"pipe":["text"]},"tag":{"pipe":[{"meta":"tag"}]}}}}]},
            "next":{"pipe":[{"css":"a.next"},{"attr":"href"},{"resolve_url":{}}]}
        }}, "count"]}
    }}]}}});
    let mut got_meta_items = Vec::new();
    let e = plan("lite", &composed);
    let cnt = drive(
        &*e,
        r#"<a class="f" href="/u/1/friends">f</a>"#,
        Value::from_json(json!({"base_url": "https://ex.test/u/1", "tag": "vip"})),
        |r| {
            let u = r.url().unwrap();
            got_meta_items.push((u.to_string(), r.meta.to_json()));
            Ok(match u {
                "https://ex.test/u/1/friends" => r#"<i class="u">X</i><a class="next" href="?p=2">n</a>"#.into(),
                "https://ex.test/u/1/friends?p=2" => r#"<i class="u">Y</i><i class="u">Z</i>"#.into(),
                other => panic!("unexpected {other}"),
            })
        },
    );
    assert_eq!(cnt, json!({"count": 3}));
    assert_eq!(got_meta_items[1].1, json!({"tag": "vip", "base_url": "https://ex.test/u/1/friends?p=2"}));
}

#[test]
fn paginate_repairs_broken_adaptive_selector_on_later_page() {
    let ir = json!({"object":{"items":{"pipe":[{"paginate":{
        "items":{"pipe":[{"adaptive":{"css":".card","key":"card","threshold":0.99}},{"map":{"pipe":["text"]}}]},
        "next":{"pipe":[{"css":"a.next"},{"attr":"href"}]}
    }}]}}});
    let e = plan("lite", &ir);
    let mut s = e
        .start(r#"<div id="list"><div class="card">A</div></div><a class="next" href="/p2">n</a>"#, Value::Null)
        .unwrap();
    let id2 = s.pending()[0].id;
    let p2_html = r#"<div id="list"><article class="tile">B</article><article class="tile">C</article></div>"#.to_string();
    e.resume(&mut s, &[(id2, p2_html)]).unwrap();
    assert!(s.is_done());
    let rep_id = s.repairs()[0].id;
    e.repair(&mut s, &[(rep_id, answer("card", ".tile"))]).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"items": ["A", "B", "C"]}));
}
