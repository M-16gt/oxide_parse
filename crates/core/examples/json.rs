use oxide_parse_core::{compile, Engine, Expr, Value};

fn main() {
    let ir = Expr::from_json(
        r#"{"object":{
            "names":{"pipe":[{"key":"users"},{"map":{"pipe":[{"key":"name"},"upper"]}}]},
            "adults":{"pipe":[{"key":"users"},{"filter":{"pipe":[{"key":"age"},{"ne":{"lit":3}}]}},"count"]}
        }}"#,
    )
    .unwrap();
    let plan = compile(&ir, "json").unwrap();
    let out = plan.extract(r#"{"users":[{"name":"ann","age":30},{"name":"bob","age":3}]}"#, Value::Null).unwrap();
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
