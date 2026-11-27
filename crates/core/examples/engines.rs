//! Same schema, every engine. Run: cargo run -p oxide_parse-core --example engines --features full --release
use oxide_parse_core::{available, compile, Engine, Expr, Value};
use std::time::Instant;

fn page(n: usize) -> String {
    let mut s = String::from("<html><body><h1>Catalog</h1><div id=list>");
    for i in 0..n {
        s.push_str(&format!(
            "<div class=\"product\" data-id=\"{i}\"><h2 class=\"name\">Product {i}</h2><span class=\"price\">${}.99</span><a class=\"link\" href=\"/p/{i}\">more</a></div>",
            i % 500
        ));
    }
    s.push_str("</div></body></html>");
    s
}

fn main() {
    println!("compiled-in engines:");
    for e in available() {
        println!("  {:<8} {:<6} xpath={:<5} {}", e.name, e.kind, e.xpath, e.description);
    }

    let ir = Expr::from_json(
        r#"{"object":{
            "title":{"pipe":[{"css":"h1"},"text"]},
            "items":{"pipe":[{"css":".product"},{"map":{"object":{
                "id":{"pipe":[{"attr":"data-id"},"int"]},
                "name":{"pipe":[{"css":".name"},"text"]},
                "price":{"pipe":[{"css":".price"},"text","float"]},
                "url":{"pipe":[{"css":".link"},{"attr":"href"}]}
            }}}]}
        }}"#,
    )
    .unwrap();

    let src = page(5_000);
    println!("\n{} KiB document, 5000 products", src.len() / 1024);
    for name in ["scraper", "lite", "stream"] {
        let plan = compile(&ir, name).unwrap();
        let t = Instant::now();
        let mut n = 0;
        for _ in 0..20 {
            if let Value::Map(m) = plan.extract(&src, Value::Null).unwrap() {
                n += m.len();
            }
        }
        println!("{name:<8} {:>8.2} ms / document", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        assert_eq!(n, 40);
    }

    println!("\n{}", compile(&ir, "auto").unwrap().explain());

    let xp = Expr::from_json(r#"{"pipe":[{"xpath":"//div[@data-id='42']/a/@href"},"first"]}"#).unwrap();
    println!("xpath on lite: {:?}", compile(&xp, "lite").unwrap().extract(&src, Value::Null).unwrap());
}
