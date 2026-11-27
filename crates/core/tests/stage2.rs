//! Stage 2 (v7.2): new features. Needs every engine: `cargo test --features full`.
#![cfg(all(feature = "engine-scraper", feature = "engine-lite", feature = "engine-stream", feature = "engine-json"))]
use oxide_parse_core::{compile, Engine, Expr, Value};
use serde_json::json;

fn plan(engine: &str, ir: &serde_json::Value) -> Box<dyn Engine> {
    compile(&Expr::from_json(&ir.to_string()).unwrap(), engine).unwrap_or_else(|e| panic!("{engine}: {e}"))
}

fn run(engine: &str, ir: &serde_json::Value, html: &str) -> serde_json::Value {
    plan(engine, ir).extract(html, Value::Null).unwrap_or_else(|e| panic!("{engine}: {e}")).to_json()
}

#[test]
fn on_error_quarantines_failing_records() {
    let html = "<div class=p><b>1</b></div><div class=p><b>oops</b></div><div class=p><b>3</b></div>";
    let ir = |mode: &str| {
        json!({"object":{"v":{"pipe":[{"css":".p"},{"map_opts":{"on_error":mode,"body":{"pipe":[{"css":"b"},"text","int"]}}}]}}})
    };
    for engine in ["scraper", "lite", "stream"] {
        let skip = plan(engine, &ir("skip")).extract_detailed(html, Value::Null).unwrap();
        assert_eq!(skip.value.to_json(), json!({"v": [1, 3]}), "{engine}");
        assert_eq!(skip.errors.len(), 1, "{engine}");
        assert_eq!(skip.errors[0].path, "$.v[1]", "{engine}");
        assert!(skip.errors[0].message.contains("int"), "{engine}: {}", skip.errors[0].message);
        assert_eq!(run(engine, &ir("null"), html), json!({"v": [1, null, 3]}), "{engine}");
        // `raise` (and plain `map`) keep failing the page, with the path of the record
        let err = plan(engine, &ir("raise")).extract(html, Value::Null).unwrap_err().to_string();
        assert!(err.contains("$.v[1]"), "{engine}: {err}");
    }
    // v8: the stream engine quarantines records too, so `auto` keeps the fast path
    assert_eq!(plan("auto", &ir("skip")).backend(), "stream");
}

#[test]
fn has_and_contains_in_the_lite_engine() {
    let html = "<ul><li class=a><b>x</b></li><li class=b><i>y</i></li><li class=c>hello world</li></ul>";
    let ir = json!({"object":{
        "has":{"pipe":[{"css":"li:has(b)"},"count"]},
        "child":{"pipe":[{"css":"ul:has(> li > i)"},"count"]},
        "not_has":{"pipe":[{"css":"li:not(:has(b, i))"},"count"]},
        "contains":{"pipe":[{"css":"li:contains('hello')"},"count"]},
        "adjacent":{"pipe":[{"css":"li:has(+ li.c)"},"count"]}
    }});
    assert_eq!(run("lite", &ir, html), json!({"has": 1, "child": 1, "not_has": 1, "contains": 1, "adjacent": 1}));
    // they need the tree: the stream engine refuses them, `auto` falls back to a DOM engine
    assert!(compile(&Expr::from_json(&ir.to_string()).unwrap(), "stream").is_err());
}

#[test]
fn resolve_url_with_meta_or_explicit_base() {
    let html = r#"<a href="../img/a.png?x=1#f">i</a>"#;
    let ir = json!({"object":{
        "u":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{}}]},
        "v":{"pipe":[{"css":"a"},{"attr":"href"},{"resolve_url":{"base":{"lit":"https://other.example/x/"}}}]}
    }});
    let meta = Value::from_json(json!({"base_url": "https://shop.example/cat/list.html"}));
    let expected = json!({"u": "https://shop.example/img/a.png?x=1#f", "v": "https://other.example/img/a.png?x=1#f"});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(plan(engine, &ir).extract(html, meta.clone()).unwrap().to_json(), expected, "{engine}");
    }
    // a relative link without any base is an error, not a silent wrong answer
    assert!(plan("lite", &ir).extract(html, Value::Null).is_err());
}

#[test]
fn js_json_extracts_state_from_script() {
    let html = r#"<script>window.__STATE__ = {"product":{"price":1990,"tags":["a","b"]}};</script>"#;
    let ir = json!({"object":{
        "price":{"pipe":[{"css":"script"},"text",{"js_json":{"var":"window.__STATE__"}},{"key":"product"},{"key":"price"}]},
        "tags":{"pipe":[{"css":"script"},"text",{"js_json":{}},{"key":"product"},{"key":"tags"},{"map":{"pipe":["upper"]}}]}
    }});
    let expected = json!({"price": 1990, "tags": ["A", "B"]});
    for engine in ["scraper", "lite", "stream"] {
        assert_eq!(run(engine, &ir, html), expected, "{engine}");
    }
}

#[test]
fn sections_group_flat_markup() {
    let html = "<div class=specs><h3>Size</h3><p>10</p><p>20</p><h3>Color</h3><p>red</p></div>";
    let expected = json!({"s": [{"header": "Size", "items": ["10", "20"]}, {"header": "Color", "items": ["red"]}]});
    for sections in [json!({"header": "h3"}), json!({"header": "h3", "items": "p"})] {
        let ir = json!({"object":{"s":{"pipe":[{"css":".specs"},{"sections": sections}]}}});
        for engine in ["scraper", "lite"] {
            assert_eq!(run(engine, &ir, html), expected, "{engine}");
        }
    }
}

#[test]
fn adaptive_misses_are_reported_for_repair() {
    let ir = json!({"object":{"x":{"pipe":[{"adaptive":{"css":".gone","key":"k"}},"text"]}}});
    let d = plan("lite", &ir).extract_detailed("<p>hi</p>", Value::Null).unwrap();
    assert_eq!(d.value.to_json(), json!({"x": null}));
    assert_eq!(d.misses.len(), 1);
    assert_eq!((d.misses[0].key.as_str(), d.misses[0].css.as_str()), ("k", ".gone"));
    assert!(d.misses[0].fingerprint.is_none());
}

#[cfg(feature = "engine-lite")]
#[test]
fn distill_is_exported() {
    let out = oxide_parse_core::distill("<div><script>x</script><p>hi</p></div>", &oxide_parse_core::DistillOptions::default());
    assert!(out.contains("hi") && !out.contains("script"), "{out}");
}
