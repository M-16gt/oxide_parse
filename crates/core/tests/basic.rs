#![cfg(all(feature = "engine-scraper", feature = "engine-json"))]
use oxide_parse_core::{compile, Engine, Expr, Value};
use serde_json::json;

const PAGE: &str = r#"<html><body><h1> Shop </h1>
<div class="product" data-active="1"><span class="name">A</span><span class="price">$1,299.50</span><a href="/a">x</a></div>
<div class="product" data-active="0"><span class="name">B</span><span class="price">5</span><a href="/b">y</a></div>
</body></html>"#;

fn engine(ir: &str, backend: &str) -> Box<dyn Engine> {
    compile(&Expr::from_json(ir).unwrap(), backend).unwrap()
}

#[test]
fn html_nested_map() {
    let e = engine(
        r#"{"object":{
            "title":{"pipe":[{"css":"h1"},"text","strip"]},
            "items":{"pipe":[{"css":".product"},{"map":{"object":{
                "name":{"pipe":[{"css":".name"},"text"]},
                "price":{"pipe":[{"css":".price"},"text","float"]},
                "href":{"pipe":[{"css":"a"},{"attr":"href"}]},
                "i":{"pipe":["index"]}
            }}}]}
        }}"#,
        "html",
    );
    let out = e.extract(PAGE, Value::Null).unwrap().to_json();
    assert_eq!(
        out,
        json!({"title":"Shop","items":[
            {"name":"A","price":1299.5,"href":"/a","i":0},
            {"name":"B","price":5.0,"href":"/b","i":1}
        ]})
    );
}

#[test]
fn missing_is_null_and_default_works() {
    let e = engine(
        r#"{"object":{
            "a":{"pipe":[{"css":".nope"},"text","float"]},
            "b":{"pipe":[{"css":".nope"},"text",{"default":{"lit":"x"}}]}
        }}"#,
        "html",
    );
    assert_eq!(e.extract(PAGE, Value::Null).unwrap().to_json(), json!({"a":null,"b":"x"}));
}

#[test]
fn filter_exists_parent_root() {
    let e = engine(
        r#"{"object":{
            "active":{"pipe":[{"css":".product"},{"filter":{"pipe":[{"attr":"data-active"},{"eq":{"lit":"1"}}]}},
                              {"map":{"pipe":[{"css":".name"},"text"]}}]},
            "parent_class":{"pipe":[{"css":".name"},"first","parent",{"attr":"class"}]},
            "has":{"pipe":[{"css":".price"},"exists"]},
            "n":{"pipe":[{"css":".product"},"count"]}
        }}"#,
        "html",
    );
    assert_eq!(
        e.extract(PAGE, Value::Null).unwrap().to_json(),
        json!({"active":["A"],"parent_class":"product","has":true,"n":2})
    );
}

#[test]
fn bad_selector_is_a_compile_error() {
    let ir = Expr::from_json(r#"{"pipe":[{"css":"div[["}]}"#).unwrap();
    assert!(compile(&ir, "html").is_err());
}

#[test]
fn xpath_is_supported_on_dom_engines_but_not_on_json() {
    let ir = Expr::from_json(r#"{"pipe":[{"xpath":"//a"}]}"#).unwrap();
    assert!(compile(&ir, "html").is_ok());
    assert!(compile(&ir, "json").is_err());
}

#[test]
fn json_backend() {
    let e = engine(
        r#"{"object":{
            "names":{"pipe":[{"key":"users"},{"map":{"pipe":[{"key":"name"}]}}]},
            "first_age":{"pipe":[{"key":"users"},{"at":0},{"key":"age"}]}
        }}"#,
        "json",
    );
    let out = e.extract(r#"{"users":[{"name":"a","age":3},{"name":"b","age":4}]}"#, Value::Null).unwrap();
    assert_eq!(out.to_json(), json!({"names":["a","b"],"first_age":3}));
}

#[test]
fn follow_state_machine() {
    let e = engine(
        r#"{"object":{"links":{"pipe":[{"css":"a.l"},{"map":{"pipe":[{"follow":{
            "request":{"object":{"url":{"pipe":[{"attr":"href"}]},"method":{"lit":"GET"}}},
            "meta":{"object":{"label":{"pipe":["text"]}}},
            "schema":{"object":{
                "label":{"pipe":[{"meta":"label"}]},
                "h2":{"pipe":[{"css":"h2"},"text"]}
            }}
        }}]}}]}}}"#,
        "html",
    );
    let page = r#"<a class="l" href="/1">one</a><a class="l" href="/2">two</a>"#;
    let mut s = e.start(page, Value::Null).unwrap();
    let urls: Vec<String> = s.pending().iter().map(|r| r.url().unwrap().to_string()).collect();
    assert_eq!(urls, vec!["/1", "/2"]);
    assert_eq!(s.pending()[0].params.get("method"), Some(&Value::Str("GET".into())));
    assert!(e.extract(page, Value::Null).is_err());
    let docs: Vec<(u64, String)> = s.pending().iter().map(|r| (r.id, format!("<h2>H{}</h2>", r.id))).collect();
    e.resume(&mut s, &docs).unwrap();
    assert!(s.is_done());
    assert_eq!(
        s.take_result().unwrap().to_json(),
        json!({"links":[{"label":"one","h2":"H0"},{"label":"two","h2":"H1"}]})
    );
}

#[test]
fn ir_roundtrip() {
    let src = r#"{"object":{"a":{"pipe":[{"css":"h1"},"text",{"regex":{"pattern":"\\d+","group":0}}]}}}"#;
    let ir = Expr::from_json(src).unwrap();
    assert_eq!(Expr::from_json(&ir.to_json()).unwrap(), ir);
}

#[test]
fn optimizer_rewrites_first() {
    let e = engine(r#"{"pipe":[{"css":".product"},"first",{"css":".name"},"first","text"]}"#, "html");
    assert!(e.explain().contains("select_first"));
}

#[test]
fn follow_accepts_string_shorthand_and_skips_null() {
    // a plain string request is shorthand for `{"url": ...}`
    let e = engine(
        r#"{"object":{"links":{"pipe":[{"css":"a"},{"map":{"pipe":[{"follow":{
            "request":{"pipe":[{"attr":"href"}]},
            "schema":{"object":{"h":{"pipe":[{"css":"h2"},"text"]}}}
        }}]}}]}}}"#,
        "html",
    );
    let page = r#"<a href="/1">x</a><a>no link</a>"#;
    let s = e.start(page, Value::Null).unwrap();
    assert_eq!(s.pending().len(), 1); // the <a> without href produced no request
    assert_eq!(s.pending()[0].url(), Some("/1"));
}

#[test]
fn follow_rejects_the_removed_url_key() {
    // v8: the only contract is `request`; the old `url` key in the IR is an error, not a silent alias
    let src = r#"{"follow":{"url":{"pipe":[{"attr":"href"}]},"schema":{"lit":1}}}"#;
    assert!(serde_json::from_str::<oxide_parse_core::ir::Op>(src).is_err());
}

// ---- regressions from the code audit (see docs/archive/audit_v7_v8.md) -----------------------------------------------

#[test]
fn audit_numbers() {
    // fields start at the document root, so every pipeline first selects the <p> that carries the attributes
    let ir = r#"{"object":{
        "eu":{"pipe":[{"css":"p"},{"attr":"data-a"},"float"]},
        "ru":{"pipe":[{"css":"p"},{"attr":"data-b"},"float"]},
        "word":{"pipe":[{"css":"p"},{"attr":"data-c"},"float"]},
        "neg":{"pipe":[{"css":"p"},{"attr":"data-d"},"float"]},
        "us":{"pipe":[{"css":"p"},{"attr":"data-e"},"float"]},
        "big":{"pipe":[{"css":"p"},{"attr":"data-f"},"int"]},
        "plain_f":{"pipe":[{"css":"p"},{"attr":"data-g"},"float"]},
        "plain_i":{"pipe":[{"css":"p"},{"attr":"data-h"},"int"]},
        "money":{"pipe":[{"css":"p"},{"attr":"data-i"},"float"]},
        "negint":{"pipe":[{"css":"p"},{"attr":"data-j"},"int"]},
        "range":{"pipe":[{"css":"p"},{"attr":"data-k"},"int"]}
    }}"#;
    let page = "<p data-a='19,99 \u{20ac}' data-b='12 990 \u{20bd}' data-c='Model-3: $100' data-d='-.5' data-e='$1,299.50' \
                data-f='9007199254740993' data-g=' 42.5 ' data-h='42' data-i='$19.99' data-j='-7' data-k='10-20'>";
    let out = engine(ir, "html").extract(page, Value::Null).unwrap().to_json();
    assert_eq!(
        out,
        json!({"eu": 19.99, "ru": 12990.0, "word": 3.0, "neg": -0.5, "us": 1299.5, "big": 9007199254740993i64,
               "plain_f": 42.5, "plain_i": 42, "money": 19.99, "negint": -7, "range": 10})
    );
}

#[test]
fn audit_new_ops() {
    let ir = r#"{"object":{
        "disabled":{"pipe":[{"css":"button"},{"has_attr":"disabled"}]},
        "hidden":{"pipe":[{"css":"button"},{"has_attr":"hidden"}]},
        "price":{"pipe":[{"css":".price"},"text","non_empty",{"default":{"lit":"on request"}}]},
        "json":{"pipe":[{"css":"script#d"},"text","parse_json",{"key":"props"},{"key":"price"}]},
        "joined":{"pipe":[{"css":"p"},{"text_opts":{"sep":" ","strip":true}}]},
        "clean":{"pipe":[{"css":"article"},{"text_opts":{"sep":" ","strip":true,"without":".ad"}}]},
        "plain":{"pipe":[{"css":"article"},"text"]}
    }}"#;
    let page = r#"<div><button disabled>Buy</button><span class="price">  </span>
        <script id="d" type="application/json">{"props":{"price":1990}}</script>
        <p>one<br>two</p><article>A <div class="ad">AD</div>B<script>alert(1)</script></article></div>"#;
    let expected = json!({"disabled": true, "hidden": false, "price": "on request", "json": 1990,
                          "joined": "one two", "clean": "A B", "plain": "A ADB"});
    assert_eq!(engine(ir, "html").extract(page, Value::Null).unwrap().to_json(), expected);
}

#[test]
fn audit_ops_after_follow_move_into_the_sub_schema() {
    let e = engine(
        r#"{"object":{"t":{"pipe":[{"follow":{"request":{"pipe":[{"css":"a"},{"attr":"href"}]},
            "schema":{"pipe":[{"css":"h1"},"text"]}}},"strip","lower"]}}}"#,
        "html",
    );
    let mut s = e.start(r#"<a href="/x">x</a>"#, Value::Null).unwrap();
    let docs: Vec<(u64, String)> = s.pending().iter().map(|r| (r.id, "<h1>  HELLO </h1>".to_string())).collect();
    e.resume(&mut s, &docs).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"t": "hello"}));
}

#[test]
fn follow_without_a_link_applies_the_tail_to_null() {
    let e = engine(
        r#"{"object":{"a":{"pipe":[{"follow":{"request":{"pipe":[{"css":"a"},{"attr":"href"}]},
            "schema":{"pipe":[{"css":"h1"},"text"]}}},{"default":{"lit":"x"}},"upper"]}}}"#,
        "html",
    );
    // no <a>: no request, and `default("x") | upper` still runs on null
    assert_eq!(e.extract("<p>no link</p>", Value::Null).unwrap().to_json(), json!({"a": "X"}));
    // with a link the same operations run on the fetched page's result
    let mut s = e.start(r#"<a href="/x">x</a>"#, Value::Null).unwrap();
    let docs: Vec<(u64, String)> = s.pending().iter().map(|r| (r.id, "<h1>hello</h1>".to_string())).collect();
    e.resume(&mut s, &docs).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"a": "HELLO"}));
}

#[test]
fn chained_follows_run_in_the_context_of_the_previous_page() {
    let e = engine(
        r#"{"object":{"t":{"pipe":[
            {"follow":{"request":{"pipe":[{"css":"a"},{"attr":"href"}]},"schema":{"lit":null}}},
            {"follow":{"request":{"pipe":[{"css":"a.next"},{"attr":"href"}]},"schema":{"pipe":[{"css":"h1"},"text"]}}},
            {"default":{"lit":"-"}},"strip","lower"
        ]}}}"#,
        "html",
    );
    // no first link: the chain stops and what follows the last follow still applies to null
    assert_eq!(e.extract("<p>nothing</p>", Value::Null).unwrap().to_json(), json!({"t": "-"}));

    let mut s = e.start(r#"<a href="/step1">1</a>"#, Value::Null).unwrap();
    assert_eq!(s.pending()[0].url(), Some("/step1"));
    // the second request is computed from the FIRST fetched page
    let id1 = s.pending()[0].id;
    e.resume(&mut s, &[(id1, r#"<a class="next" href="/step2">2</a>"#.to_string())]).unwrap();
    assert_eq!(s.pending().len(), 1);
    assert_eq!(s.pending()[0].url(), Some("/step2"));
    let id2 = s.pending()[0].id;
    e.resume(&mut s, &[(id2, "<h1>  FINAL </h1>".to_string())]).unwrap();
    assert_eq!(s.take_result().unwrap().to_json(), json!({"t": "final"}));
}
