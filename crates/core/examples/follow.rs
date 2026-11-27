//! `follow` without IO: the engine returns requests, the caller decides how to fetch.
use oxide_parse_core::html;
use oxide_parse_core::{Engine, Expr, Value};
use std::collections::HashMap;

fn main() {
    let ir = Expr::from_json(
        r#"{"object":{"books":{"pipe":[{"css":".book"},{"map":{"pipe":[{"follow":{
            "request":{"object":{
                "url":{"pipe":[{"css":"a"},{"attr":"href"}]},
                "headers":{"object":{"Accept":{"lit":"text/html"}}}
            }},
            "meta":{"object":{"title":{"pipe":[{"css":"h2"},"text"]}}},
            "schema":{"object":{
                "title":{"pipe":[{"meta":"title"}]},
                "pages":{"pipe":[{"css":".pages"},"text","int"]}
            }}
        }}]}}]}}}"#,
    )
    .unwrap();
    let plan = html::compile(&ir).unwrap();

    // a fake "network"
    let mut site: HashMap<&str, &str> = HashMap::new();
    site.insert("/rust", r#"<span class="pages">552 pages</span>"#);
    site.insert("/py", r#"<span class="pages">1,200 pages</span>"#);

    let catalog = r#"<div class="book"><h2>Rust</h2><a href="/rust">d</a></div>
                     <div class="book"><h2>Python</h2><a href="/py">d</a></div>"#;

    let out = plan
        .extract_with(catalog, Value::Null, |req| {
            // req.params is the whole request dict: url, headers, ...
            site.get(req.url().unwrap_or("")).map(|s| s.to_string()).ok_or_else(|| "404".to_string())
        })
        .unwrap();
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
