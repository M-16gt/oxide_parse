//! Where does the time go? Each stage on its own.
//! Run: cargo bench -p oxide_parse-core --features "engine-lite engine-stream" --bench stages
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxide_parse_core::lite::{self, LiteDoc};
use oxide_parse_core::markup::arena::Doc;
use oxide_parse_core::markup::events::Events;
use oxide_parse_core::{Engine, Expr, Value};

fn page(n: usize, noise: bool) -> String {
    let mut s = String::from("<html><body><h1>Catalog</h1><div id=list>");
    for i in 0..n {
        s.push_str(&format!(
            "<div class=\"product\" data-id=\"{i}\"><h2 class=\"name\">Product {i}</h2><span class=\"price\">${}.99</span><a class=\"link\" href=\"/p/{i}\">more</a><p class=\"desc\">Some description text for product {i}</p></div>",
            i % 500
        ));
        if noise {
            s.push_str("<div class=\"banner\"><span>ad</span><span>ad</span><span>ad</span><ul><li>a</li><li>b</li><li>c</li></ul></div>");
        }
    }
    s.push_str("</div></body></html>");
    s
}

const IR: &str = r#"{"object":{
  "title":{"pipe":[{"css":"h1"},"text"]},
  "items":{"pipe":[{"css":".product"},{"map":{"object":{
    "id":{"pipe":[{"attr":"data-id"},"int"]},
    "name":{"pipe":[{"css":".name"},"text"]},
    "price":{"pipe":[{"css":".price"},"text","float"]},
    "url":{"pipe":[{"css":".link"},{"attr":"href"}]},
    "desc":{"pipe":[{"css":".desc"},"text"]}
  }}}]}}}"#;

fn bench(c: &mut Criterion) {
    let ir = Expr::from_json(IR).unwrap();
    let lite_plan = lite::compile(&ir).unwrap();
    let stream_plan = oxide_parse_core::stream::StreamPlan::compile(&ir).unwrap();
    let mut g = c.benchmark_group("stages");
    for (n, noise) in [(1000usize, false), (10000, false), (10000, true)] {
        let src = page(n, noise);
        let id = format!("{n}{}", if noise { "+noise" } else { "" });
        g.throughput(Throughput::Bytes(src.len() as u64));
        // 1) tokenizer + tree rules only (what both lite and stream pay first)
        g.bench_with_input(BenchmarkId::new("1_events_only", &id), &src, |b, s| b.iter(|| black_box(Events::new(s).count())));
        // 2) + arena DOM construction
        g.bench_with_input(BenchmarkId::new("2_lite_parse", &id), &src, |b, s| b.iter(|| black_box(Doc::parse(s).nodes.len())));
        // 3) executing the plan on an already parsed DOM (selectors + Value construction)
        let doc = LiteDoc::parse(&src);
        g.bench_with_input(BenchmarkId::new("3_lite_exec_only", &id), &doc, |b, d| {
            b.iter(|| black_box(lite_plan.start_dom(&d.dom(), Value::Null).unwrap()))
        });
        // 4) everything
        g.bench_with_input(BenchmarkId::new("4_lite_total", &id), &src, |b, s| b.iter(|| black_box(lite_plan.extract(s, Value::Null).unwrap())));
        g.bench_with_input(BenchmarkId::new("5_stream_total", &id), &src, |b, s| b.iter(|| black_box(stream_plan.extract(s, Value::Null).unwrap())));
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
