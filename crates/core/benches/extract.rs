use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxide_parse_core::html::{self, HtmlDoc};
use oxide_parse_core::{Engine, Expr, Value};
use scraper::{Html, Selector};

fn page(n: usize) -> String {
    let mut s = String::from("<html><body><h1>Catalog</h1><div id=list>");
    for i in 0..n {
        s.push_str(&format!(
            "<div class=\"product\" data-id=\"{i}\"><h2 class=\"name\">Product {i}</h2><span class=\"price\">${}.99</span><a class=\"link\" href=\"/p/{i}\">more</a><p class=\"desc\">Some description text for product {i}</p></div>",
            i % 500
        ));
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

fn handwritten(src: &str) -> usize {
    let doc = Html::parse_document(src);
    let (p, n, pr, l, d) = (
        Selector::parse(".product").unwrap(),
        Selector::parse(".name").unwrap(),
        Selector::parse(".price").unwrap(),
        Selector::parse(".link").unwrap(),
        Selector::parse(".desc").unwrap(),
    );
    let mut count = 0;
    for el in doc.select(&p) {
        let _id: i64 = el.value().attr("data-id").unwrap().parse().unwrap();
        let _name = el.select(&n).next().map(|x| x.text().collect::<String>());
        let _price: f64 = el.select(&pr).next().map(|x| x.text().collect::<String>()).unwrap().trim_start_matches('$').parse().unwrap();
        let _url = el.select(&l).next().and_then(|x| x.value().attr("href").map(str::to_owned));
        let _desc = el.select(&d).next().map(|x| x.text().collect::<String>());
        count += 1;
    }
    count
}

fn bench(c: &mut Criterion) {
    let plan = html::compile(&Expr::from_json(IR).unwrap()).unwrap();
    let mut g = c.benchmark_group("extract_products");
    for n in [100usize, 1000, 5000] {
        let src = page(n);
        g.throughput(Throughput::Bytes(src.len() as u64));
        g.bench_with_input(BenchmarkId::new("parse_only", n), &src, |b, s| b.iter(|| black_box(HtmlDoc::parse(s))));
        g.bench_with_input(BenchmarkId::new("handwritten_scraper", n), &src, |b, s| b.iter(|| black_box(handwritten(s))));
        g.bench_with_input(BenchmarkId::new("oxide_parse_plan", n), &src, |b, s| {
            b.iter(|| black_box(plan.extract(s, Value::Null).unwrap()))
        });
        // execution only: DOM already parsed
        let doc = HtmlDoc::parse(&src);
        g.bench_with_input(BenchmarkId::new("oxide_parse_exec_only", n), &doc, |b, d| {
            b.iter(|| {
                let dom = d.dom();
                black_box(plan.start_dom(&dom, Value::Null).unwrap())
            })
        });
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
