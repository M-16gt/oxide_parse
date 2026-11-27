//! Compare engines on the same schema. Run: cargo bench -p oxide_parse-core --features full --bench engines
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxide_parse_core::{compile, Engine, Expr, Value};

fn page(n: usize) -> String {
    let mut s = String::from("<html><body><h1>Catalog</h1><div id=list>");
    for i in 0..n {
        s.push_str(&format!(
            "<div class=\"product\" data-id=\"{i}\"><h2 class=\"name\">Product {i}</h2><span class=\"price\">${}.99</span><a class=\"link\" href=\"/p/{i}\">more</a><p class=\"desc\">Some description text for product {i}</p></div>",
            i % 500
        ));
        // noise the schema does not care about: this is where "no DOM" pays off
        s.push_str("<div class=\"banner\"><span>ad</span><span>ad</span><span>ad</span><ul><li>a</li><li>b</li><li>c</li></ul></div>");
    }
    s.push_str("</div></body></html>");
    s
}

const CAPTURE_SCHEMA: &str = r#"{"object":{
  "title":{"pipe":[{"css":"h1"},"text"]},
  "items":{"pipe":[{"css":".product"},{"map":{"object":{
    "id":{"pipe":[{"attr":"data-id"},"int"]},
    "name":{"pipe":[{"css":".name"},"text"]},
    "price":{"pipe":[{"css":".price"},"text","float"]},
    "url":{"pipe":[{"css":".link"},{"attr":"href"}]},
    "desc":{"pipe":[{"css":".desc"},"text"]}
  }}}]}}}"#;

fn bench(c: &mut Criterion) {
    let ir = Expr::from_json(CAPTURE_SCHEMA).unwrap();
    let plans: Vec<(&str, Box<dyn Engine>)> =
        ["scraper", "lite", "stream"].iter().map(|n| (*n, compile(&ir, n).unwrap())).collect();
    let mut g = c.benchmark_group("engines_capture_schema");
    for n in [100usize, 1000, 5000] {
        let src = page(n);
        g.throughput(Throughput::Bytes(src.len() as u64));
        for (name, plan) in &plans {
            g.bench_with_input(BenchmarkId::new(*name, n), &src, |b, s| b.iter(|| black_box(plan.extract(s, Value::Null).unwrap())));
        }
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
