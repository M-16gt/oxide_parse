use oxide_parse_core::{compile, Engine, Expr, Value};

fn main() {
    let html = r#"<h1>Shop</h1>
      <div class="product"><b class="name">Lamp</b><i class="price">$19.90</i><a href="/lamp">more</a></div>
      <div class="product"><b class="name">Desk</b><i class="price">$149.00</i><a href="/desk">more</a></div>"#;

    let ir = Expr::from_json(
        r#"{"object":{
            "title":{"pipe":[{"css":"h1"},"text"]},
            "products":{"pipe":[{"css":".product"},{"map":{"object":{
                "name":{"pipe":[{"css":".name"},"text"]},
                "price":{"pipe":[{"css":".price"},"text","float"]},
                "url":{"pipe":[{"css":"a"},{"attr":"href"}]}
            }}}]}
        }}"#,
    )
    .unwrap();

    let plan = compile(&ir, "html").unwrap(); // compile once, reuse for every document
    println!("{}", plan.explain());
    let out = plan.extract(html, Value::Null).unwrap();
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
