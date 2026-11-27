//! Engine-independent XPath 1.0 (subset) evaluator. It only needs the navigation primitives of
//! [`Dom`], so it works on every DOM engine (scraper, lite, ...) with a single implementation.
//!
//! Results are typed like in parsel: element paths give `Nodes`, `@attr` / `text()` paths give `Strings`,
//! functions give `Num` / `Str` / `Bool`.
//!
//! Known limits: no `following`/`preceding` axes, no namespaces, `@attr`/`text()` only as the last step,
//! text nodes are modelled as strings (document order of `//text()` is "element by element").
mod parser;

pub use parser::{parse, XPathExpr};

use crate::dom::Dom;
use parser::{Axis, BinOp, Expr, Start, Step, Test};
use std::collections::HashSet;

/// Result of evaluating an XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum XValue<N> {
    Nodes(Vec<N>),
    Strings(Vec<String>),
    Num(f64),
    Str(String),
    Bool(bool),
}

/// Evaluate `xp` with `ctx` as the context node.
pub fn evaluate<D: Dom>(dom: &D, ctx: &D::Node, xp: &XPathExpr) -> Result<XValue<D::Node>, String> {
    let cx = Cx {
        item: Item::El(ctx.clone()),
        pos: 1,
        size: 1,
    };
    Ok(match eval(dom, &xp.expr, &cx)? {
        V::Items(items) => XValue::Nodes(
            items
                .into_iter()
                .filter_map(|i| match i {
                    Item::El(n) => Some(n),
                    Item::Doc => None,
                })
                .collect(),
        ),
        V::Strings(s) => XValue::Strings(s),
        V::Num(n) => XValue::Num(n),
        V::Str(s) => XValue::Str(s),
        V::Bool(b) => XValue::Bool(b),
    })
}

// ---------------------------------------------------------------------------------------------

type Nd<D> = <D as Dom>::Node;

/// A node of the XPath data model that we can hold: the virtual document node or an element.
#[derive(Clone)]
enum Item<N> {
    Doc,
    El(N),
}

enum V<N> {
    Items(Vec<Item<N>>),
    Strings(Vec<String>),
    Num(f64),
    Str(String),
    Bool(bool),
}

struct Cx<N> {
    item: Item<N>,
    pos: usize,
    size: usize,
}

fn key_of<D: Dom>(dom: &D, it: &Item<Nd<D>>) -> Option<usize> {
    match it {
        Item::Doc => None,
        Item::El(n) => Some(dom.node_key(n)),
    }
}

fn descendants<D: Dom>(dom: &D, n: &Nd<D>, out: &mut Vec<Item<Nd<D>>>) {
    let mut stack: Vec<Nd<D>> = Vec::new();
    let mut tmp: Vec<Nd<D>> = Vec::new();
    dom.children(n, &mut tmp);
    stack.extend(tmp.into_iter().rev());
    while let Some(x) = stack.pop() {
        out.push(Item::El(x.clone()));
        let mut ch: Vec<Nd<D>> = Vec::new();
        dom.children(&x, &mut ch);
        stack.extend(ch.into_iter().rev());
    }
}

fn axis_items<D: Dom>(dom: &D, it: &Item<Nd<D>>, axis: Axis, out: &mut Vec<Item<Nd<D>>>) {
    match it {
        Item::Doc => {
            let mut tops: Vec<Nd<D>> = Vec::new();
            dom.document_children(&mut tops);
            match axis {
                Axis::Child => out.extend(tops.into_iter().map(Item::El)),
                Axis::Descendant | Axis::DescendantOrSelf => {
                    if axis == Axis::DescendantOrSelf {
                        out.push(Item::Doc);
                    }
                    for t in tops {
                        out.push(Item::El(t.clone()));
                        descendants(dom, &t, out);
                    }
                }
                Axis::SelfAxis | Axis::AncestorOrSelf => out.push(Item::Doc),
                _ => {}
            }
        }
        Item::El(n) => match axis {
            Axis::Child => {
                let mut ch: Vec<Nd<D>> = Vec::new();
                dom.children(n, &mut ch);
                out.extend(ch.into_iter().map(Item::El));
            }
            Axis::Descendant => descendants(dom, n, out),
            Axis::DescendantOrSelf => {
                out.push(it.clone());
                descendants(dom, n, out);
            }
            Axis::SelfAxis => out.push(it.clone()),
            Axis::Parent => out.push(match dom.parent(n) {
                Some(p) => Item::El(p),
                None => Item::Doc,
            }),
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if axis == Axis::AncestorOrSelf {
                    out.push(it.clone());
                }
                let mut cur = dom.parent(n);
                while let Some(p) = cur {
                    cur = dom.parent(&p);
                    out.push(Item::El(p));
                }
                out.push(Item::Doc);
            }
            Axis::FollowingSibling => {
                let mut cur = dom.next_sibling(n);
                while let Some(s) = cur {
                    cur = dom.next_sibling(&s);
                    out.push(Item::El(s));
                }
            }
            Axis::PrecedingSibling => {
                let mut cur = dom.prev_sibling(n);
                while let Some(s) = cur {
                    cur = dom.prev_sibling(&s);
                    out.push(Item::El(s));
                }
            }
            Axis::Attribute => {}
        },
    }
}

fn test_ok<D: Dom>(dom: &D, it: &Item<Nd<D>>, test: &Test) -> bool {
    match (test, it) {
        (Test::Node, _) => true,
        (Test::Any, Item::El(_)) => true,
        (Test::Name(want), Item::El(n)) => dom.name(n).map_or(false, |have| {
            if let Some(pfx) = want.strip_suffix(":*") {
                have.split_once(':')
                    .map_or(false, |(p, _)| p.eq_ignore_ascii_case(pfx))
            } else {
                have.eq_ignore_ascii_case(want)
            }
        }),
        _ => false,
    }
}

fn eval_step<D: Dom>(
    dom: &D,
    input: &[Item<Nd<D>>],
    step: &Step,
) -> Result<Vec<Item<Nd<D>>>, String> {
    let mut out: Vec<Item<Nd<D>>> = Vec::new();
    let mut seen: HashSet<Option<usize>> = HashSet::new();
    for it in input {
        let mut cand: Vec<Item<Nd<D>>> = Vec::new();
        axis_items(dom, it, step.axis, &mut cand);
        cand.retain(|c| test_ok(dom, c, &step.test));
        for pred in &step.preds {
            cand = apply_pred(dom, cand, pred)?;
        }
        for c in cand {
            if seen.insert(key_of(dom, &c)) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

fn apply_pred<D: Dom>(
    dom: &D,
    cand: Vec<Item<Nd<D>>>,
    pred: &Expr,
) -> Result<Vec<Item<Nd<D>>>, String> {
    let size = cand.len();
    let mut out = Vec::with_capacity(size);
    for (i, c) in cand.into_iter().enumerate() {
        let cx = Cx {
            item: c.clone(),
            pos: i + 1,
            size,
        };
        let v = eval(dom, pred, &cx)?;
        let keep = match &v {
            V::Num(n) => *n == (i + 1) as f64,
            other => to_bool(other),
        };
        if keep {
            out.push(c);
        }
    }
    Ok(out)
}

fn terminal_values<D: Dom>(dom: &D, input: &[Item<Nd<D>>], step: &Step) -> Vec<String> {
    let mut out = Vec::new();
    for it in input {
        let n = match it {
            Item::El(n) => n,
            Item::Doc => continue,
        };
        if step.axis == Axis::Attribute {
            match &step.test {
                Test::Name(a) => out.extend(dom.attr(n, a)),
                _ => {
                    let mut all = Vec::new();
                    dom.attrs(n, &mut all);
                    out.extend(all.into_iter().map(|(_, v)| v));
                }
            }
        } else {
            out.extend(dom.direct_texts(n));
            if step.axis != Axis::Child {
                let mut d = Vec::new();
                descendants(dom, n, &mut d);
                for x in d {
                    if let Item::El(e) = x {
                        out.extend(dom.direct_texts(&e));
                    }
                }
            }
        }
    }
    out
}

fn eval_steps<D: Dom>(
    dom: &D,
    mut cur: Vec<Item<Nd<D>>>,
    steps: &[Step],
) -> Result<V<Nd<D>>, String> {
    for step in steps {
        if step.is_terminal() {
            return Ok(V::Strings(terminal_values(dom, &cur, step)));
        }
        cur = eval_step(dom, &cur, step)?;
    }
    Ok(V::Items(cur))
}

// ---- value conversions -----------------------------------------------------------------------

fn fmt_num(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f.is_sign_negative() {
            "-Infinity".into()
        } else {
            "Infinity".into()
        }
    } else if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else if f.abs() >= 1e15 {
        let s = format!("{f:.14e}");
        let (mant, exp) = s.split_once('e').unwrap();
        let mant = mant.trim_end_matches('0').trim_end_matches('.');
        let (sign, digits) = if let Some(d) = exp.strip_prefix('-') {
            ('-', d)
        } else {
            ('+', exp.strip_prefix('+').unwrap_or(exp))
        };
        format!("{mant}e{sign}{digits:0>2}")
    } else {
        f.to_string()
    }
}

fn xpath_round(n: f64) -> f64 {
    if !n.is_finite() || n.fract() == 0.0 {
        return n;
    }
    if (-0.5..0.0).contains(&n) {
        return -0.0;
    }
    let r = (n + 0.5).floor();
    if r == 0.0 && n.is_sign_negative() {
        -0.0
    } else {
        r
    }
}

fn item_string<D: Dom>(dom: &D, it: &Item<Nd<D>>) -> String {
    match it {
        Item::Doc => String::new(),
        Item::El(n) => dom.text(n),
    }
}

fn to_bool<N>(v: &V<N>) -> bool {
    match v {
        V::Items(i) => !i.is_empty(),
        V::Strings(s) => !s.is_empty(),
        V::Num(n) => *n != 0.0 && !n.is_nan(),
        V::Str(s) => !s.is_empty(),
        V::Bool(b) => *b,
    }
}

fn to_str<D: Dom>(dom: &D, v: &V<Nd<D>>) -> String {
    match v {
        V::Items(i) => i.first().map(|x| item_string(dom, x)).unwrap_or_default(),
        V::Strings(s) => s.first().cloned().unwrap_or_default(),
        V::Num(n) => fmt_num(*n),
        V::Str(s) => s.clone(),
        V::Bool(b) => b.to_string(),
    }
}

fn str_to_num(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(f64::NAN)
}

fn to_num<D: Dom>(dom: &D, v: &V<Nd<D>>) -> f64 {
    match v {
        V::Num(n) => *n,
        V::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        other => str_to_num(&to_str(dom, other)),
    }
}

/// String values of a set-like value, `None` for scalars.
fn set_strings<D: Dom>(dom: &D, v: &V<Nd<D>>) -> Option<Vec<String>> {
    match v {
        V::Items(i) => Some(i.iter().map(|x| item_string(dom, x)).collect()),
        V::Strings(s) => Some(s.clone()),
        _ => None,
    }
}

fn cmp_num(op: BinOp, a: f64, b: f64) -> bool {
    match op {
        BinOp::Eq => a == b,
        BinOp::Ne => a != b,
        BinOp::Lt => a < b,
        BinOp::Le => a <= b,
        BinOp::Gt => a > b,
        BinOp::Ge => a >= b,
        _ => false,
    }
}

fn cmp_strs(op: BinOp, a: &str, b: &str) -> bool {
    match op {
        BinOp::Eq => a == b,
        BinOp::Ne => a != b,
        _ => cmp_num(op, str_to_num(a), str_to_num(b)),
    }
}

fn compare<D: Dom>(dom: &D, op: BinOp, a: &V<Nd<D>>, b: &V<Nd<D>>) -> bool {
    if matches!(a, V::Bool(_)) || matches!(b, V::Bool(_)) {
        let (x, y) = (to_bool(a), to_bool(b));
        return match op {
            BinOp::Eq => x == y,
            BinOp::Ne => x != y,
            _ => cmp_num(op, x as u8 as f64, y as u8 as f64),
        };
    }
    match (set_strings(dom, a), set_strings(dom, b)) {
        (Some(x), Some(y)) => x.iter().any(|p| y.iter().any(|q| cmp_strs(op, p, q))),
        (Some(x), None) => x.iter().any(|p| cmp_scalar(dom, op, p, b)),
        (None, Some(y)) => y.iter().any(|q| cmp_scalar_rev(dom, op, a, q)),
        (None, None) => match (a, b) {
            (V::Num(_), _) | (_, V::Num(_)) => cmp_num(op, to_num(dom, a), to_num(dom, b)),
            _ => cmp_strs(op, &to_str(dom, a), &to_str(dom, b)),
        },
    }
}

/// `item_string <op> scalar`
fn cmp_scalar<D: Dom>(dom: &D, op: BinOp, item: &str, scalar: &V<Nd<D>>) -> bool {
    match scalar {
        V::Num(n) => cmp_num(op, str_to_num(item), *n),
        other => cmp_strs(op, item, &to_str(dom, other)),
    }
}

/// `scalar <op> item_string`
fn cmp_scalar_rev<D: Dom>(dom: &D, op: BinOp, scalar: &V<Nd<D>>, item: &str) -> bool {
    match scalar {
        V::Num(n) => cmp_num(op, *n, str_to_num(item)),
        other => cmp_strs(op, &to_str(dom, other), item),
    }
}

// ---- expressions -----------------------------------------------------------------------------

fn eval<D: Dom>(dom: &D, e: &Expr, cx: &Cx<Nd<D>>) -> Result<V<Nd<D>>, String> {
    match e {
        Expr::Literal(s) => Ok(V::Str(s.clone())),
        Expr::Number(n) => Ok(V::Num(*n)),
        Expr::Neg(inner) => {
            let v = eval(dom, inner, cx)?;
            Ok(V::Num(-to_num(dom, &v)))
        }
        Expr::Path { start, steps } => {
            let items: Vec<Item<Nd<D>>> = match start {
                Start::Root => vec![Item::Doc],
                Start::Context => vec![cx.item.clone()],
                Start::Expr(base, preds) => {
                    let mut items = match eval(dom, base, cx)? {
                        V::Items(i) => i,
                        _ => return Err("a path step can only be applied to a node-set".into()),
                    };
                    for p in preds {
                        items = apply_pred(dom, items, p)?;
                    }
                    items
                }
            };
            eval_steps(dom, items, steps)
        }
        Expr::Binary(op, a, b) => match op {
            BinOp::Or => {
                let l = eval(dom, a, cx)?;
                if to_bool(&l) {
                    return Ok(V::Bool(true));
                }
                Ok(V::Bool(to_bool(&eval(dom, b, cx)?)))
            }
            BinOp::And => {
                let l = eval(dom, a, cx)?;
                if !to_bool(&l) {
                    return Ok(V::Bool(false));
                }
                Ok(V::Bool(to_bool(&eval(dom, b, cx)?)))
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (l, r) = (eval(dom, a, cx)?, eval(dom, b, cx)?);
                Ok(V::Bool(compare(dom, *op, &l, &r)))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let (l, r) = (eval(dom, a, cx)?, eval(dom, b, cx)?);
                let (x, y) = (to_num(dom, &l), to_num(dom, &r));
                Ok(V::Num(match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                    _ => x % y,
                }))
            }
            BinOp::Union => {
                let (l, r) = (eval(dom, a, cx)?, eval(dom, b, cx)?);
                match (l, r) {
                    (V::Items(mut x), V::Items(y)) => {
                        let mut seen: HashSet<Option<usize>> =
                            x.iter().map(|i| key_of(dom, i)).collect();
                        for i in y {
                            if seen.insert(key_of(dom, &i)) {
                                x.push(i);
                            }
                        }
                        Ok(V::Items(x))
                    }
                    (V::Strings(mut x), V::Strings(y)) => {
                        x.extend(y);
                        Ok(V::Strings(x))
                    }
                    _ => Err("`|` needs two node-sets (or two attribute/text sets)".into()),
                }
            }
        },
        Expr::Func(name, args) => call(dom, name, args, cx),
    }
}

fn arg<'a>(name: &str, args: &'a [Expr], i: usize) -> Result<&'a Expr, String> {
    args.get(i)
        .ok_or_else(|| format!("{name}() is missing argument {}", i + 1))
}

/// Value of the optional first argument, defaulting to the context node.
fn arg_or_ctx<D: Dom>(
    dom: &D,
    name: &str,
    args: &[Expr],
    cx: &Cx<Nd<D>>,
) -> Result<V<Nd<D>>, String> {
    let _ = name;
    match args.first() {
        Some(a) => eval(dom, a, cx),
        None => Ok(V::Items(vec![cx.item.clone()])),
    }
}

fn call<D: Dom>(dom: &D, name: &str, args: &[Expr], cx: &Cx<Nd<D>>) -> Result<V<Nd<D>>, String> {
    let s = |i: usize| -> Result<String, String> {
        Ok(to_str(dom, &eval(dom, arg(name, args, i)?, cx)?))
    };
    Ok(match name {
        "position" => V::Num(cx.pos as f64),
        "last" => V::Num(cx.size as f64),
        "true" => V::Bool(true),
        "false" => V::Bool(false),
        "not" => V::Bool(!to_bool(&eval(dom, arg(name, args, 0)?, cx)?)),
        "boolean" => V::Bool(to_bool(&eval(dom, arg(name, args, 0)?, cx)?)),
        "count" => match eval(dom, arg(name, args, 0)?, cx)? {
            V::Items(i) => V::Num(i.len() as f64),
            V::Strings(s) => V::Num(s.len() as f64),
            _ => return Err("count() needs a node-set".into()),
        },
        "string" => V::Str(to_str(dom, &arg_or_ctx(dom, name, args, cx)?)),
        "number" => V::Num(to_num(dom, &arg_or_ctx(dom, name, args, cx)?)),
        "string-length" => V::Num(
            to_str(dom, &arg_or_ctx(dom, name, args, cx)?)
                .chars()
                .count() as f64,
        ),
        "normalize-space" => {
            let t = to_str(dom, &arg_or_ctx(dom, name, args, cx)?);
            V::Str(t.split_whitespace().collect::<Vec<_>>().join(" "))
        }
        "lower-case" => V::Str(s(0)?.to_lowercase()),
        "upper-case" => V::Str(s(0)?.to_uppercase()),
        "contains" => V::Bool(s(0)?.contains(s(1)?.as_str())),
        "starts-with" => V::Bool(s(0)?.starts_with(s(1)?.as_str())),
        "ends-with" => V::Bool(s(0)?.ends_with(s(1)?.as_str())),
        "substring" => {
            if args.len() > 3 {
                return Err("substring() takes 2 or 3 arguments".into());
            }
            let src = s(0)?;
            let start = xpath_round(to_num(dom, &eval(dom, arg(name, args, 1)?, cx)?));
            if let Some(len_arg) = args.get(2) {
                let len = xpath_round(to_num(dom, &eval(dom, len_arg, cx)?));
                let end = start + len;
                V::Str(
                    src.chars()
                        .enumerate()
                        .filter(|&(i, _)| {
                            let p = (i + 1) as f64;
                            p >= start && p < end
                        })
                        .map(|(_, c)| c)
                        .collect(),
                )
            } else {
                V::Str(
                    src.chars()
                        .enumerate()
                        .filter(|&(i, _)| ((i + 1) as f64) >= start)
                        .map(|(_, c)| c)
                        .collect(),
                )
            }
        }
        "lang" => {
            let want = s(0)?.to_ascii_lowercase();
            let mut found = None;
            let mut cur = match &cx.item {
                Item::El(n) => Some(n.clone()),
                Item::Doc => None,
            };
            while let Some(n) = cur {
                if let Some(v) = dom.attr(&n, "xml:lang").or_else(|| dom.attr(&n, "lang")) {
                    found = Some(v.to_ascii_lowercase());
                    break;
                }
                cur = dom.parent(&n);
            }
            let ok = found.map_or(false, |have| {
                have == want
                    || (have.starts_with(want.as_str())
                        && have.as_bytes().get(want.len()) == Some(&b'-'))
            });
            V::Bool(ok)
        }
        "substring-before" => {
            let (a, b) = (s(0)?, s(1)?);
            V::Str(
                a.find(b.as_str())
                    .map(|p| a[..p].to_string())
                    .unwrap_or_default(),
            )
        }
        "substring-after" => {
            let (a, b) = (s(0)?, s(1)?);
            V::Str(
                a.find(b.as_str())
                    .map(|p| a[p + b.len()..].to_string())
                    .unwrap_or_default(),
            )
        }
        "concat" => {
            let mut out = String::new();
            for i in 0..args.len() {
                out.push_str(&s(i)?);
            }
            V::Str(out)
        }
        "translate" => {
            let (src, from, to) = (s(0)?, s(1)?, s(2)?);
            let (from, to): (Vec<char>, Vec<char>) = (from.chars().collect(), to.chars().collect());
            V::Str(
                src.chars()
                    .filter_map(|c| match from.iter().position(|f| *f == c) {
                        Some(i) => to.get(i).copied(),
                        None => Some(c),
                    })
                    .collect(),
            )
        }
        "matches" => {
            let (text, pat) = (s(0)?, s(1)?);
            let re = regex::Regex::new(&pat).map_err(|e| format!("matches(): bad regex: {e}"))?;
            V::Bool(re.is_match(&text))
        }
        "sum" => match eval(dom, arg(name, args, 0)?, cx)? {
            V::Items(i) => V::Num(i.iter().map(|x| str_to_num(&item_string(dom, x))).sum()),
            V::Strings(s) => V::Num(s.iter().map(|x| str_to_num(x)).sum()),
            _ => return Err("sum() needs a node-set".into()),
        },
        "floor" => V::Num(to_num(dom, &eval(dom, arg(name, args, 0)?, cx)?).floor()),
        "ceiling" => V::Num(to_num(dom, &eval(dom, arg(name, args, 0)?, cx)?).ceil()),
        "round" => V::Num(xpath_round(to_num(
            dom,
            &eval(dom, arg(name, args, 0)?, cx)?,
        ))),
        "name" => {
            let v = arg_or_ctx(dom, name, args, cx)?;
            let first = match &v {
                V::Items(i) => i.first().cloned(),
                _ => None,
            };
            match first {
                Some(Item::El(n)) => {
                    V::Str(dom.name(&n).map(|c| c.to_lowercase()).unwrap_or_default())
                }
                _ => V::Str(String::new()),
            }
        }
        "local-name" => {
            let v = arg_or_ctx(dom, name, args, cx)?;
            let first = match &v {
                V::Items(i) => i.first().cloned(),
                _ => None,
            };
            match first {
                Some(Item::El(n)) => {
                    let raw = dom.name(&n).map(|c| c.to_lowercase()).unwrap_or_default();
                    let local = raw.rsplit(':').next().unwrap_or(raw.as_str()).to_string();
                    V::Str(local)
                }
                _ => V::Str(String::new()),
            }
        }
        "namespace-uri" => {
            let v = arg_or_ctx(dom, name, args, cx)?;
            let first = match &v {
                V::Items(i) => i.first().cloned(),
                _ => None,
            };
            match first {
                Some(Item::El(n)) => {
                    let raw = dom.name(&n).map(|c| c.into_owned()).unwrap_or_default();
                    let uri = match raw.split_once(':') {
                        Some((pfx, _)) => {
                            if pfx.eq_ignore_ascii_case("xml") {
                                Some("http://www.w3.org/XML/1998/namespace".to_string())
                            } else {
                                let attr_name = format!("xmlns:{pfx}");
                                let mut found = None;
                                let mut cur = Some(n);
                                while let Some(el) = cur {
                                    if let Some(u) = dom.attr(&el, &attr_name) {
                                        found = Some(u);
                                        break;
                                    }
                                    cur = dom.parent(&el);
                                }
                                found
                            }
                        }
                        None => {
                            let mut found = None;
                            let mut cur = Some(n);
                            while let Some(el) = cur {
                                if let Some(u) = dom.attr(&el, "xmlns") {
                                    found = Some(u);
                                    break;
                                }
                                cur = dom.parent(&el);
                            }
                            found
                        }
                    };
                    V::Str(uri.unwrap_or_default())
                }
                _ => V::Str(String::new()),
            }
        }
        other => return Err(format!("unsupported XPath function {other}()")),
    })
}
