//! Structured extraction helpers: HTML table expansion, WHATWG `srcset`, locale prices, and deduplication keys.
use crate::dom::Dom;
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

fn cell_text<D: Dom>(dom: &D, cell: &D::Node) -> String {
    let mut skip: HashSet<usize> = HashSet::new();
    let mut stack: Vec<D::Node> = Vec::new();
    dom.children(cell, &mut stack);
    while let Some(el) = stack.pop() {
        if dom.name(&el).map_or(false, |t| t.eq_ignore_ascii_case("table")) {
            skip.insert(dom.node_key(&el));
        } else {
            dom.children(&el, &mut stack);
        }
    }
    if skip.is_empty() {
        return dom.text(cell);
    }
    let mut parts = Vec::new();
    dom.text_parts(cell, &skip, &mut parts);
    parts.concat()
}

fn push_table_rows<D: Dom>(
    dom: &D,
    container: &D::Node,
    section_id: usize,
    is_thead: bool,
    out: &mut Vec<(usize, bool, D::Node)>,
) {
    let mut kids: Vec<D::Node> = Vec::new();
    dom.children(container, &mut kids);
    for child in kids {
        let tag = match dom.name(&child) {
            Some(n) => n,
            None => continue,
        };
        if tag.eq_ignore_ascii_case("tr") {
            out.push((section_id, is_thead, child));
        }
    }
}

fn collect_table_rows<D: Dom>(dom: &D, table: &D::Node) -> Vec<(usize, bool, D::Node)> {
    let mut out: Vec<(usize, bool, D::Node)> = Vec::new();
    let mut kids: Vec<D::Node> = Vec::new();
    dom.children(table, &mut kids);
    let mut next_section = 1usize;
    for child in kids {
        let tag = match dom.name(&child) {
            Some(n) => n,
            None => continue,
        };
        if tag.eq_ignore_ascii_case("tr") {
            out.push((0, false, child));
        } else if tag.eq_ignore_ascii_case("thead") {
            let sid = next_section;
            next_section += 1;
            push_table_rows(dom, &child, sid, true, &mut out);
        } else if tag.eq_ignore_ascii_case("tbody") || tag.eq_ignore_ascii_case("tfoot") {
            let sid = next_section;
            next_section += 1;
            push_table_rows(dom, &child, sid, false, &mut out);
        }
    }
    out
}

fn parse_span(raw: Option<String>, allow_zero: bool) -> usize {
    match raw.and_then(|s| s.trim().parse::<usize>().ok()) {
        Some(0) if allow_zero => 0,
        Some(0) | None => 1,
        Some(n) => n.min(1000),
    }
}

/// Expand an HTML `<table>` into a rectangular 2D grid (resolving `rowspan` and `colspan`).
///
/// `mode`: `0` = records (`List[Dict[str, str]]`), `1` = grid (`List[List[str]]`),
/// `2` = split (`{"headers": List[List[str]], "rows": List[List[str]]}`).
pub(crate) fn extract_table<D: Dom>(
    dom: &D,
    table: &D::Node,
    mode: u8,
    strip: bool,
    header_sep: &str,
) -> Value {
    let rows = collect_table_rows(dom, table);
    if rows.is_empty() {
        return match mode {
            2 => {
                let mut m = IndexMap::new();
                m.insert("headers".to_string(), Value::List(Vec::new()));
                m.insert("rows".to_string(), Value::List(Vec::new()));
                Value::Map(m)
            }
            _ => Value::List(Vec::new()),
        };
    }

    let total_rows = rows.len();
    let mut section_end: HashMap<usize, usize> = HashMap::new();
    for (r_idx, (sid, _, _)) in rows.iter().enumerate() {
        section_end.insert(*sid, r_idx + 1);
    }

    let any_thead = rows.iter().any(|(_, in_thead, _)| *in_thead);
    let mut header_end_row = 0usize;
    let mut still_leading_headers = true;

    let mut cells: HashMap<(usize, usize), String> = HashMap::new();
    let mut max_cols = 0usize;

    for (r_idx, (sid, in_thead, tr)) in rows.iter().enumerate() {
        let mut kids: Vec<D::Node> = Vec::new();
        dom.children(tr, &mut kids);

        let mut row_has_th = false;
        let mut row_has_td = false;
        let mut col = 0usize;

        for cell in kids {
            let Some(tag) = dom.name(&cell) else { continue };
            let is_th = tag.eq_ignore_ascii_case("th");
            let is_td = tag.eq_ignore_ascii_case("td");
            if !is_th && !is_td {
                continue;
            }
            row_has_th |= is_th;
            row_has_td |= is_td;

            while cells.contains_key(&(r_idx, col)) {
                col += 1;
            }
            let colspan = parse_span(dom.attr(&cell, "colspan"), false);
            let raw_rowspan = parse_span(dom.attr(&cell, "rowspan"), true);
            let span_end = if raw_rowspan == 0 {
                *section_end.get(sid).unwrap_or(&total_rows)
            } else {
                (r_idx + raw_rowspan).min(total_rows)
            };

            let raw_txt = cell_text(dom, &cell);
            let txt = if strip {
                raw_txt.trim().to_string()
            } else {
                raw_txt
            };
            for rr in r_idx..span_end {
                for cc in col..(col + colspan) {
                    cells.entry((rr, cc)).or_insert_with(|| txt.clone());
                }
            }
            col += colspan;
            if col > max_cols {
                max_cols = col;
            }
        }

        if any_thead {
            if *in_thead {
                header_end_row = r_idx + 1;
            }
        } else if still_leading_headers {
            if row_has_th && !row_has_td {
                header_end_row = r_idx + 1;
            } else {
                still_leading_headers = false;
            }
        }
    }

    let mut grid: Vec<Vec<String>> = Vec::with_capacity(total_rows);
    for r_idx in 0..total_rows {
        let mut row = Vec::with_capacity(max_cols);
        for c_idx in 0..max_cols {
            row.push(cells.remove(&(r_idx, c_idx)).unwrap_or_default());
        }
        grid.push(row);
    }

    let to_value_rows = |slice: &[Vec<String>]| -> Value {
        Value::List(
            slice
                .iter()
                .map(|row| Value::List(row.iter().map(|s| Value::Str(s.clone())).collect()))
                .collect(),
        )
    };

    match mode {
        1 => to_value_rows(&grid),
        2 => {
            let split_at = header_end_row.min(grid.len());
            let mut m = IndexMap::new();
            m.insert("headers".to_string(), to_value_rows(&grid[..split_at]));
            m.insert("rows".to_string(), to_value_rows(&grid[split_at..]));
            Value::Map(m)
        }
        _ => {
            let hdr_rows = if header_end_row > 0 {
                header_end_row.min(grid.len())
            } else if grid.len() >= 2 {
                1
            } else {
                return Value::List(Vec::new());
            };
            let hdr_slice = &grid[..hdr_rows];
            let data_slice = &grid[hdr_rows..];

            let mut used: HashMap<String, usize> = HashMap::new();
            let mut col_names: Vec<String> = Vec::with_capacity(max_cols);
            for c_idx in 0..max_cols {
                let mut parts: Vec<String> = Vec::new();
                for h_row in hdr_slice {
                    let label = h_row[c_idx].trim();
                    if !label.is_empty() && parts.last().map_or(true, |prev| prev != label) {
                        parts.push(label.to_string());
                    }
                }
                let base = if parts.is_empty() {
                    format!("col_{c_idx}")
                } else {
                    parts.join(header_sep)
                };
                let count = used.entry(base.clone()).or_insert(0);
                *count += 1;
                let final_name = if *count == 1 {
                    base
                } else {
                    format!("{base}_{count}")
                };
                col_names.push(final_name);
            }

            let out_rows = data_slice
                .iter()
                .map(|row| {
                    let mut m = IndexMap::with_capacity(max_cols);
                    for (c_idx, val) in row.iter().enumerate() {
                        m.insert(col_names[c_idx].clone(), Value::Str(val.clone()));
                    }
                    Value::Map(m)
                })
                .collect();
            Value::List(out_rows)
        }
    }
}

#[derive(Clone, Debug)]
struct SrcsetCandidate {
    url: String,
    width: Option<i64>,
    density: Option<f64>,
}

impl SrcsetCandidate {
    fn to_value(&self) -> Value {
        let mut m = IndexMap::new();
        m.insert("url".to_string(), Value::Str(self.url.clone()));
        m.insert(
            "width".to_string(),
            self.width.map(Value::Int).unwrap_or(Value::Null),
        );
        m.insert(
            "density".to_string(),
            self.density.map(Value::Float).unwrap_or(Value::Null),
        );
        Value::Map(m)
    }
}

/// Parse an HTML `srcset` attribute string following the WHATWG candidate splitting rules.
pub(crate) fn parse_srcset(
    src: &str,
    pick: Option<&str>,
    min_width: Option<usize>,
    min_density: Option<f64>,
) -> Result<Value, String> {
    let b = src.as_bytes();
    let len = b.len();
    let mut i = 0usize;
    let mut candidates: Vec<SrcsetCandidate> = Vec::new();

    while i < len {
        while i < len && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= len {
            break;
        }
        let u_start = i;
        while i < len && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let raw_url = &src[u_start..i];
        let trimmed_url = raw_url.trim_end_matches(',');
        let had_trailing_comma = trimmed_url.len() != raw_url.len();
        let url = trimmed_url.to_string();

        let mut width: Option<i64> = None;
        let mut density: Option<f64> = None;

        if !had_trailing_comma {
            let mut desc_tokens: Vec<&str> = Vec::new();
            let mut paren_depth = 0usize;
            while i < len {
                while i < len && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= len {
                    break;
                }
                if b[i] == b',' && paren_depth == 0 {
                    i += 1;
                    break;
                }
                let t_start = i;
                while i < len && !b[i].is_ascii_whitespace() {
                    if b[i] == b'(' {
                        paren_depth += 1;
                    } else if b[i] == b')' {
                        paren_depth = paren_depth.saturating_sub(1);
                    } else if b[i] == b',' && paren_depth == 0 {
                        break;
                    }
                    i += 1;
                }
                if i > t_start {
                    desc_tokens.push(&src[t_start..i]);
                }
                if i < len && b[i] == b',' && paren_depth == 0 {
                    i += 1;
                    break;
                }
            }
            for tok in desc_tokens {
                if let Some(num_s) = tok.strip_suffix('w').or_else(|| tok.strip_suffix('W')) {
                    if let Ok(w) = num_s.parse::<i64>() {
                        if w > 0 {
                            width = Some(w);
                        }
                    }
                } else if let Some(num_s) = tok.strip_suffix('x').or_else(|| tok.strip_suffix('X')) {
                    if let Ok(d) = num_s.parse::<f64>() {
                        if d.is_finite() && d > 0.0 {
                            density = Some(d);
                        }
                    }
                }
            }
        }
        if !url.is_empty() {
            candidates.push(SrcsetCandidate { url, width, density });
        }
    }

    if pick.is_none() && min_width.is_none() && min_density.is_none() {
        return Ok(Value::List(
            candidates.iter().map(SrcsetCandidate::to_value).collect(),
        ));
    }

    if candidates.is_empty() {
        return Ok(Value::Null);
    }

    let chosen = if let Some(mw) = min_width {
        let mw_i = mw as i64;
        candidates
            .iter()
            .filter(|c| c.width.map_or(false, |w| w >= mw_i))
            .min_by_key(|c| c.width.unwrap_or(i64::MAX))
            .or_else(|| {
                candidates
                    .iter()
                    .max_by_key(|c| c.width.unwrap_or(-1))
            })
    } else if let Some(md) = min_density {
        candidates
            .iter()
            .filter(|c| c.density.unwrap_or(1.0) >= md)
            .min_by(|a, b| {
                a.density
                    .unwrap_or(1.0)
                    .partial_cmp(&b.density.unwrap_or(1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .or_else(|| {
                candidates.iter().max_by(|a, b| {
                    a.density
                        .unwrap_or(1.0)
                        .partial_cmp(&b.density.unwrap_or(1.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            })
    } else if pick == Some("max") {
        let has_width = candidates.iter().any(|c| c.width.is_some());
        if has_width {
            candidates.iter().max_by_key(|c| c.width.unwrap_or(-1))
        } else {
            candidates.iter().max_by(|a, b| {
                a.density
                    .unwrap_or(1.0)
                    .partial_cmp(&b.density.unwrap_or(1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
    } else if pick == Some("min") {
        let has_width = candidates.iter().any(|c| c.width.is_some());
        if has_width {
            candidates
                .iter()
                .min_by_key(|c| c.width.unwrap_or(i64::MAX))
        } else {
            candidates.iter().min_by(|a, b| {
                a.density
                    .unwrap_or(1.0)
                    .partial_cmp(&b.density.unwrap_or(1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
    } else {
        return Err(format!(
            "parse_srcset: pick must be \"max\" or \"min\", got {pick:?}"
        ));
    };

    Ok(chosen.map(SrcsetCandidate::to_value).unwrap_or(Value::Null))
}

const ISO_CODES: &[&str] = &[
    "USD", "EUR", "GBP", "RUB", "UAH", "KZT", "BYN", "PLN", "CZK", "CHF", "JPY", "CNY", "RMB",
    "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "TRY", "INR", "BRL", "KRW", "AED", "SAR", "ILS",
    "HKD", "SGD", "MXN", "ZAR", "RON", "HUF", "BGN", "THB", "IDR", "MYR", "PHP", "VND",
];

fn detect_currency(src: &str, hint: Option<&str>) -> Option<String> {
    for (pat, code) in [
        ("US$", "USD"),
        ("CA$", "CAD"),
        ("AU$", "AUD"),
        ("NZ$", "NZD"),
        ("HK$", "HKD"),
        ("S$", "SGD"),
        ("R$", "BRL"),
        ("C$", "CAD"),
        ("A$", "AUD"),
    ] {
        if src.contains(pat) {
            return Some(code.to_string());
        }
    }
    let bytes = src.as_bytes();
    for i in 0..bytes.len() {
        if i + 3 <= bytes.len() {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphabetic();
            let next_ok = i + 3 == bytes.len() || !bytes[i + 3].is_ascii_alphabetic();
            if prev_ok && next_ok && bytes[i..i + 3].iter().all(|b| b.is_ascii_alphabetic()) {
                let cand = src[i..i + 3].to_ascii_uppercase();
                if ISO_CODES.contains(&cand.as_str()) {
                    return Some(cand);
                }
            }
        }
    }
    for ch in src.chars() {
        match ch {
            '€' => return Some("EUR".to_string()),
            '£' => return Some("GBP".to_string()),
            '₽' => return Some("RUB".to_string()),
            '₴' => return Some("UAH".to_string()),
            '₸' => return Some("KZT".to_string()),
            '₹' => return Some("INR".to_string()),
            '₩' => return Some("KRW".to_string()),
            '₺' => return Some("TRY".to_string()),
            '₪' => return Some("ILS".to_string()),
            '$' => return Some(hint.unwrap_or("USD").to_string()),
            '¥' => return Some(hint.unwrap_or("JPY").to_string()),
            _ => {}
        }
    }
    hint.map(str::to_string)
}

fn parse_number_with_decimal(src: &str, decimal: char) -> Option<f64> {
    let mut start = None;
    let chars: Vec<char> = src.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_digit()
            || (c == '-'
                && (i == 0 || !chars[i - 1].is_alphanumeric())
                && chars.get(i + 1).map_or(false, |n| n.is_ascii_digit()))
        {
            start = Some(i);
            break;
        }
    }
    let mut i = start?;
    let mut out = String::new();
    if chars[i] == '-' {
        out.push('-');
        i += 1;
    }
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            out.push(c);
            i += 1;
        } else if (c == '.' || c == ',') && chars.get(i + 1).map_or(false, |n| n.is_ascii_digit()) {
            if c == decimal {
                out.push('.');
            }
            i += 1;
        } else if matches!(c, ' ' | '\u{a0}' | '\u{202f}')
            && chars.get(i + 1).map_or(false, |n| n.is_ascii_digit())
        {
            i += 1;
        } else {
            break;
        }
    }
    out.parse::<f64>().ok()
}

/// Extract a structured `{"amount": float, "currency": str | null}` price, or `None` when absent.
pub(crate) fn parse_price(src: &str, decimal: Option<&str>, currency_hint: Option<&str>) -> Option<Value> {
    let amount = match decimal {
        Some(",") => parse_number_with_decimal(src, ','),
        Some(".") => parse_number_with_decimal(src, '.'),
        _ => crate::exec::parse_number(src),
    }?;
    let cur = detect_currency(src, currency_hint);
    let mut m = IndexMap::new();
    m.insert("amount".to_string(), Value::Float(amount));
    m.insert(
        "currency".to_string(),
        cur.map(Value::Str).unwrap_or(Value::Null),
    );
    Some(Value::Map(m))
}

/// Canonical deduplication key comparing `Int(n)` and equal `Float(n)` identically.
pub(crate) fn canonical_key(v: &Value) -> String {
    match v {
        Value::Null | Value::Pending(_) => "n".to_string(),
        Value::Bool(b) => format!("b:{b}"),
        Value::Int(i) => format!("num:{i}"),
        Value::Float(f) => {
            if f.is_finite() && f.fract() == 0.0 && f.abs() < 9e15 {
                format!("num:{}", *f as i64)
            } else {
                format!("f:{f}")
            }
        }
        Value::Str(s) => format!("s:{s}"),
        Value::List(_) | Value::Map(_) => {
            format!("j:{}", serde_json::to_string(&v.to_json()).unwrap_or_default())
        }
    }
}

pub(crate) fn dedup_with_keys(items: Vec<Value>, keys: Vec<String>, keep_last: bool) -> Vec<Value> {
    let mut map: IndexMap<String, Value> = IndexMap::with_capacity(items.len());
    for (item, key) in items.into_iter().zip(keys.into_iter()) {
        if keep_last {
            map.insert(key, item);
        } else {
            map.entry(key).or_insert(item);
        }
    }
    map.into_values().collect()
}

pub(crate) fn dedup_plain(items: Vec<Value>, keep_last: bool) -> Vec<Value> {
    let mut map: IndexMap<String, Value> = IndexMap::with_capacity(items.len());
    for item in items {
        let key = canonical_key(&item);
        if keep_last {
            map.insert(key, item);
        } else {
            map.entry(key).or_insert(item);
        }
    }
    map.into_values().collect()
}

// -- Case #11: JSON-LD @id reference expansion ----------------------------------------------------------------------

fn collect_ld_defs(v: &serde_json::Value, defs: &mut HashMap<String, serde_json::Value>) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_ld_defs(item, defs);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(id) = map.get("@id").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                if map.len() > 1 {
                    let entry = defs
                        .entry(id.to_string())
                        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                    if let serde_json::Value::Object(target) = entry {
                        for (k, val) in map {
                            target.entry(k.clone()).or_insert_with(|| val.clone());
                        }
                    }
                }
            }
            for val in map.values() {
                collect_ld_defs(val, defs);
            }
        }
        _ => {}
    }
}

fn expand_ld_refs(
    v: &mut serde_json::Value,
    defs: &HashMap<String, serde_json::Value>,
    visited: &mut HashSet<String>,
    depth: usize,
) {
    if depth >= 16 {
        return;
    }
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr {
                expand_ld_refs(item, defs, visited, depth + 1);
            }
        }
        serde_json::Value::Object(map) => {
            let ref_id = map
                .get("@id")
                .and_then(|x| x.as_str())
                .filter(|id| defs.contains_key(*id) && !visited.contains(*id))
                .map(str::to_string);
            if let Some(id) = ref_id {
                visited.insert(id.clone());
                let mut resolved = defs[&id].clone();
                expand_ld_refs(&mut resolved, defs, visited, depth + 1);
                if map.len() == 1 {
                    *v = resolved;
                    visited.remove(&id);
                    return;
                } else if let serde_json::Value::Object(res_map) = resolved {
                    for (k, val) in res_map {
                        map.entry(k).or_insert(val);
                    }
                }
                for (k, val) in map.iter_mut() {
                    if k != "@id" {
                        expand_ld_refs(val, defs, visited, depth + 1);
                    }
                }
                visited.remove(&id);
                return;
            }
            for val in map.values_mut() {
                expand_ld_refs(val, defs, visited, depth + 1);
            }
        }
        _ => {}
    }
}

/// Resolve `{"@id": "..."}` object references inside unfolded JSON-LD nodes (cycle-safe).
pub(crate) fn resolve_json_ld_refs(items: &mut [serde_json::Value]) {
    let mut defs: HashMap<String, serde_json::Value> = HashMap::new();
    for item in items.iter() {
        collect_ld_defs(item, &mut defs);
    }
    if defs.is_empty() {
        return;
    }
    let mut visited = HashSet::new();
    for item in items.iter_mut() {
        expand_ld_refs(item, &defs, &mut visited, 0);
    }
}

// -- Case #12: WHATWG HTML Microdata extractor ----------------------------------------------------------------------

fn type_matches(attr: Option<&str>, want: &str) -> bool {
    let Some(s) = attr else { return false };
    s.split_whitespace()
        .any(|t| t == want || t.rsplit(|c| c == '/' || c == '#').next() == Some(want))
}

fn collect_id_map<D: Dom>(dom: &D, root: &D::Node, out: &mut HashMap<String, D::Node>) {
    let mut stack = vec![root.clone()];
    while let Some(el) = stack.pop() {
        if let Some(id) = dom.attr(&el, "id") {
            let id_t = id.trim();
            if !id_t.is_empty() {
                out.entry(id_t.to_string()).or_insert_with(|| el.clone());
            }
        }
        let mut kids = Vec::new();
        dom.children(&el, &mut kids);
        for k in kids.into_iter().rev() {
            stack.push(k);
        }
    }
}

fn microdata_prop_value<D: Dom>(
    dom: &D,
    el: &D::Node,
    id_map: &HashMap<String, D::Node>,
    active: &mut HashSet<usize>,
) -> Value {
    if dom.attr(el, "itemscope").is_some() {
        let key = dom.node_key(el);
        if active.contains(&key) {
            return Value::Null;
        }
        return extract_microdata_item(dom, el, id_map, active);
    }
    let tag = dom.name(el).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let s = match tag.as_str() {
        "meta" => dom.attr(el, "content").unwrap_or_default(),
        "audio" | "embed" | "iframe" | "img" | "source" | "track" | "video" => {
            dom.attr(el, "src").unwrap_or_default()
        }
        "a" | "area" | "link" => dom.attr(el, "href").unwrap_or_default(),
        "object" => dom.attr(el, "data").unwrap_or_default(),
        "data" | "meter" => dom
            .attr(el, "value")
            .unwrap_or_else(|| dom.text(el).trim().to_string()),
        "time" => dom
            .attr(el, "datetime")
            .unwrap_or_else(|| dom.text(el).trim().to_string()),
        _ => dom.text(el).trim().to_string(),
    };
    Value::Str(s)
}

fn extract_microdata_item<D: Dom>(
    dom: &D,
    item: &D::Node,
    id_map: &HashMap<String, D::Node>,
    active: &mut HashSet<usize>,
) -> Value {
    let item_key = dom.node_key(item);
    active.insert(item_key);
    let mut map: IndexMap<String, Value> = IndexMap::new();
    if let Some(t) = dom.attr(item, "itemtype") {
        let types: Vec<Value> = t.split_whitespace().map(|s| Value::Str(s.to_string())).collect();
        if types.len() == 1 {
            map.insert("@type".to_string(), types.into_iter().next().unwrap());
        } else if types.len() > 1 {
            map.insert("@type".to_string(), Value::List(types));
        }
    }
    if let Some(id) = dom.attr(item, "itemid") {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            map.insert("@id".to_string(), Value::Str(trimmed.to_string()));
        }
    }

    let mut stack: Vec<D::Node> = Vec::new();
    let mut initial: Vec<D::Node> = Vec::new();
    dom.children(item, &mut initial);
    if let Some(refs) = dom.attr(item, "itemref") {
        for r in refs.split_whitespace() {
            if let Some(target) = id_map.get(r) {
                initial.push(target.clone());
            }
        }
    }
    for el in initial.into_iter().rev() {
        stack.push(el);
    }

    let mut seen: HashSet<usize> = HashSet::new();
    seen.insert(item_key);
    while let Some(el) = stack.pop() {
        let key = dom.node_key(&el);
        if !seen.insert(key) {
            continue;
        }
        let has_scope = dom.attr(&el, "itemscope").is_some();
        if let Some(props) = dom.attr(&el, "itemprop") {
            let names: Vec<&str> = props.split_whitespace().collect();
            if !names.is_empty() {
                let val = microdata_prop_value(dom, &el, id_map, active);
                for name in names {
                    match map.get_mut(name) {
                        None => {
                            map.insert(name.to_string(), val.clone());
                        }
                        Some(Value::List(list)) => {
                            list.push(val.clone());
                        }
                        Some(prev) => {
                            let first = std::mem::replace(prev, Value::Null);
                            *prev = Value::List(vec![first, val.clone()]);
                        }
                    }
                }
            }
        }
        if !has_scope {
            let mut kids = Vec::new();
            dom.children(&el, &mut kids);
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        }
    }

    active.remove(&item_key);
    Value::Map(map)
}

/// Extract HTML Microdata objects from `[itemscope]` nodes matching optional `schema_type`.
pub(crate) fn extract_microdata<D: Dom>(
    dom: &D,
    scopes: &[D::Node],
    doc_root: &D::Node,
    schema_type: Option<&str>,
) -> Value {
    let mut id_map: HashMap<String, D::Node> = HashMap::new();
    if scopes.iter().any(|n| dom.attr(n, "itemref").is_some()) {
        let mut top = Vec::new();
        dom.document_children(&mut top);
        if top.is_empty() {
            top.push(doc_root.clone());
        }
        for root in top {
            collect_id_map(dom, &root, &mut id_map);
        }
    }

    let mut out = Vec::new();
    let mut active = HashSet::new();
    for node in scopes {
        let keep = match schema_type {
            Some(want) => type_matches(dom.attr(node, "itemtype").as_deref(), want),
            None => dom.attr(node, "itemprop").is_none(),
        };
        if keep {
            out.push(extract_microdata_item(dom, node, &id_map, &mut active));
        }
    }
    Value::List(out)
}

// -- Case #14: Next.js App Router RSC Flight decoder ----------------------------------------------------------------

fn parse_flight_stream(buf: &str, rows: &mut IndexMap<String, serde_json::Value>) {
    let bytes = buf.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b'\r' || bytes[i] == b'\n') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let line_end = bytes[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|o| i + o)
            .unwrap_or(bytes.len());
        let Some(colon_off) = bytes[i..line_end].iter().position(|&b| b == b':') else {
            i = line_end + 1;
            continue;
        };
        let row_id = buf[i..i + colon_off].trim();
        let after_colon = i + colon_off + 1;
        if row_id.is_empty() || after_colon >= bytes.len() {
            i = line_end + 1;
            continue;
        }
        let first = bytes[after_colon];
        if first == b'T' {
            if let Some(comma_off) = bytes[after_colon + 1..line_end].iter().position(|&b| b == b',') {
                let len_str = &buf[after_colon + 1..after_colon + 1 + comma_off];
                if let Ok(text_len) = usize::from_str_radix(len_str.trim(), 16) {
                    let payload_start = after_colon + 1 + comma_off + 1;
                    let payload_end = (payload_start + text_len).min(bytes.len());
                    if let Ok(text) = std::str::from_utf8(&bytes[payload_start..payload_end]) {
                        rows.insert(row_id.to_string(), serde_json::Value::String(text.to_string()));
                    }
                    i = payload_end;
                    if i < bytes.len() && bytes[i] == b'\r' {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == b'\n' {
                        i += 1;
                    }
                    continue;
                }
            }
        }

        let rest = &buf[after_colon..line_end];
        let rest_trim = rest.trim();
        // Module/hint lines like `I[...]` or `HL[...]` are skipped unless plain JSON follows a single uppercase tag.
        let json_slice = if rest_trim.starts_with('{') || rest_trim.starts_with('[') || rest_trim.starts_with('"') {
            Some(rest_trim)
        } else if rest_trim.starts_with("I[") || rest_trim.starts_with("HL[") || rest_trim.starts_with("S\"") {
            None
        } else if let Some(first_c) = rest_trim.chars().next() {
            if first_c.is_ascii_uppercase() && rest_trim.len() > 1 {
                let tail = rest_trim[first_c.len_utf8()..].trim_start();
                if tail.starts_with('{') || tail.starts_with('[') || tail.starts_with('"') {
                    Some(tail)
                } else {
                    None
                }
            } else {
                Some(rest_trim)
            }
        } else {
            None
        };
        if let Some(js) = json_slice {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(js) {
                rows.insert(row_id.to_string(), parsed);
            }
        }
        i = line_end + 1;
    }
}

fn resolve_rsc_value(
    v: &serde_json::Value,
    rows: &IndexMap<String, serde_json::Value>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> serde_json::Value {
    if depth >= 16 {
        return v.clone();
    }
    match v {
        serde_json::Value::String(s) => {
            if s == "$undefined" {
                return serde_json::Value::Null;
            }
            if let Some(escaped) = s.strip_prefix("$$") {
                return serde_json::Value::String(format!("${escaped}"));
            }
            let target = s
                .strip_prefix("$L")
                .or_else(|| s.strip_prefix("$@"))
                .or_else(|| s.strip_prefix('$'));
            if let Some(id) = target {
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) && rows.contains_key(id) {
                    if visited.insert(id.to_string()) {
                        let res = resolve_rsc_value(&rows[id], rows, visited, depth + 1);
                        visited.remove(id);
                        return res;
                    }
                }
            }
            v.clone()
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|x| resolve_rsc_value(x, rows, visited, depth + 1))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, val) in map {
                out.insert(k.clone(), resolve_rsc_value(val, rows, visited, depth + 1));
            }
            serde_json::Value::Object(out)
        }
        _ => v.clone(),
    }
}

fn collect_key_matches(v: &serde_json::Value, want: &str, out: &mut Vec<Value>) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_key_matches(item, want, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(hit) = map.get(want) {
                out.push(Value::from_json_ref(hit));
            }
            for (k, val) in map {
                if k != want {
                    collect_key_matches(val, want, out);
                }
            }
        }
        _ => {}
    }
}

/// Extract decoded Next.js App Router `self.__next_f.push(...)` RSC Flight payloads from `<script>` contents.
pub(crate) fn parse_next_flight(scripts: &[String], want_key: Option<&str>) -> Value {
    let mut stream_buf = String::new();
    let mut direct: Vec<serde_json::Value> = Vec::new();
    for script in scripts {
        let mut offset = 0usize;
        while let Some(pos) = script[offset..].find("__next_f.push") {
            let after_fn = &script[offset + pos + "__next_f.push".len()..];
            let Some(paren_off) = after_fn.find('(') else { break };
            let inside = after_fn[paren_off + 1..].trim_start();
            if inside.starts_with('[') {
                if let Some(arr_src) = crate::jsjson::balanced(inside) {
                    if let Ok(serde_json::Value::Array(mut arr)) = serde_json::from_str::<serde_json::Value>(arr_src) {
                        if arr.len() >= 2 {
                            let item = arr.swap_remove(1);
                            match item {
                                serde_json::Value::String(chunk) => stream_buf.push_str(&chunk),
                                other => direct.push(other),
                            }
                        }
                    }
                    offset += pos + "__next_f.push".len() + paren_off + 1 + arr_src.len();
                    continue;
                }
            }
            offset += pos + "__next_f.push".len();
        }
    }

    let mut rows: IndexMap<String, serde_json::Value> = IndexMap::new();
    if !stream_buf.is_empty() {
        parse_flight_stream(&stream_buf, &mut rows);
    }
    let mut visited = HashSet::new();
    let mut resolved: Vec<serde_json::Value> = Vec::with_capacity(rows.len() + direct.len());
    for (_, val) in &rows {
        resolved.push(resolve_rsc_value(val, &rows, &mut visited, 0));
    }
    for val in &direct {
        resolved.push(resolve_rsc_value(val, &rows, &mut visited, 0));
    }

    if let Some(key) = want_key {
        let mut hits = Vec::new();
        for item in &resolved {
            collect_key_matches(item, key, &mut hits);
        }
        return Value::List(hits);
    }
    Value::List(resolved.into_iter().map(Value::from_json).collect())
}

// -- Case #31: Main content container finder (Readability-inspired scoring) -----------------------------------------

fn has_any_token(haystack: &str, tokens: &[&str]) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| {
            let low = part.to_ascii_lowercase();
            !low.is_empty() && tokens.contains(&low.as_str())
        })
}

fn has_substring_token(haystack: &str, substrs: &[&str]) -> bool {
    let low = haystack.to_ascii_lowercase();
    substrs.iter().any(|p| low.contains(p))
}

fn link_text_len<D: Dom>(dom: &D, el: &D::Node) -> usize {
    let mut links = Vec::new();
    let mut stack = vec![el.clone()];
    while let Some(node) = stack.pop() {
        if dom.name(&node).map_or(false, |t| t.eq_ignore_ascii_case("a")) {
            links.push(node);
        } else {
            let mut kids = Vec::new();
            dom.children(&node, &mut kids);
            stack.extend(kids);
        }
    }
    links
        .into_iter()
        .map(|a| dom.text(&a).chars().filter(|c| !c.is_whitespace()).count())
        .sum()
}

/// Score descendant containers under `root` to locate the primary article/content element.
pub(crate) fn find_main_content<D: Dom>(dom: &D, root: &D::Node) -> Option<D::Node> {
    const SKIP_TAGS: &[&str] = &[
        "script", "style", "noscript", "svg", "template", "head", "nav", "footer", "aside", "form", "dialog",
    ];
    const POS_SUBSTR: &[&str] = &[
        "article", "content", "main", "post", "entry", "story", "markdown", "prose", "body-text",
    ];
    const NEG_TOKENS: &[&str] = &[
        "sidebar", "footer", "header", "navbar", "nav", "menu", "breadcrumb", "comments", "comment",
        "cookie", "banner", "advert", "promo", "modal", "popup", "share", "social", "related", "ad", "ads",
    ];

    let mut parent_of: HashMap<usize, usize> = HashMap::new();
    let mut nodes: HashMap<usize, D::Node> = HashMap::new();
    let mut scores: HashMap<usize, f64> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();

    let mut roots = Vec::new();
    if dom.name(root).is_some() {
        roots.push(root.clone());
    } else {
        dom.document_children(&mut roots);
        if roots.is_empty() {
            dom.children(root, &mut roots);
        }
    }

    let mut stack: Vec<(Option<usize>, D::Node)> = roots.into_iter().rev().map(|n| (None, n)).collect();
    while let Some((pid, el)) = stack.pop() {
        let Some(tag_raw) = dom.name(&el) else { continue };
        let tag = tag_raw.to_ascii_lowercase();
        if SKIP_TAGS.contains(&tag.as_str()) {
            continue;
        }
        let key = dom.node_key(&el);
        if let Some(p) = pid {
            parent_of.insert(key, p);
        }
        nodes.insert(key, el.clone());
        order.push(key);

        let mut base = match tag.as_str() {
            "article" => 30.0,
            "main" => 25.0,
            "section" => 6.0,
            "div" => 2.0,
            "body" | "html" => -12.0,
            _ => 0.0,
        };
        let role = dom.attr(&el, "role").unwrap_or_default().to_ascii_lowercase();
        if role == "main" {
            base += 25.0;
        } else if matches!(role.as_str(), "navigation" | "banner" | "contentinfo" | "complementary" | "search") {
            base -= 40.0;
        }
        let sig = format!(
            "{} {}",
            dom.attr(&el, "id").unwrap_or_default(),
            dom.attr(&el, "class").unwrap_or_default()
        );
        if !sig.trim().is_empty() {
            if has_substring_token(&sig, POS_SUBSTR) {
                base += 22.0;
            }
            if has_any_token(&sig, NEG_TOKENS) {
                base -= 35.0;
            }
        }
        scores.insert(key, base);

        let is_text_block = matches!(tag.as_str(), "p" | "pre" | "blockquote");
        if is_text_block {
            let txt = dom.text(&el);
            let chars = txt.chars().filter(|c| !c.is_whitespace()).count();
            let min_chars = if tag == "p" { 20 } else { 8 };
            if chars >= min_chars {
                let commas = txt.matches(|c| c == ',' || c == '，' || c == ';').count();
                let lchars = link_text_len(dom, &el);
                let ldens = (lchars as f64) / (chars.max(1) as f64);
                let pts = (1.0 + (chars as f64 / 50.0).min(8.0) + commas as f64) * (1.0 - ldens.min(0.95));
                if let Some(p) = pid {
                    *scores.entry(p).or_insert(0.0) += pts;
                    if let Some(&gp) = parent_of.get(&p) {
                        *scores.entry(gp).or_insert(0.0) += pts * 0.5;
                    }
                }
            }
        } else if matches!(tag.as_str(), "div" | "article" | "section" | "main") {
            let direct_chars: usize = dom
                .direct_texts(&el)
                .iter()
                .map(|s| s.chars().filter(|c| !c.is_whitespace()).count())
                .sum();
            if direct_chars >= 40 {
                *scores.entry(key).or_insert(0.0) += (direct_chars as f64 / 60.0).min(8.0);
            }
        }

        let mut kids = Vec::new();
        dom.children(&el, &mut kids);
        for k in kids.into_iter().rev() {
            stack.push((Some(key), k));
        }
    }

    // Prefer dedicated content containers (`<article>`, `<main>`, `<div class=...>`) over top wrappers (`html`/`body`).
    let mut best_key: Option<usize> = None;
    let mut best_score = f64::NEG_INFINITY;
    for k in &order {
        let mut sc = *scores.get(k).unwrap_or(&0.0);
        let tag = nodes.get(k).and_then(|n| dom.name(n)).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        if matches!(tag.as_str(), "html" | "body") {
            sc = sc.min(0.5) - 20.0;
        }
        if matches!(tag.as_str(), "article" | "main" | "section" | "div") && sc > best_score {
            best_score = sc;
            best_key = Some(*k);
        }
    }
    if best_score > 0.0 {
        if let Some(k) = best_key {
            return nodes.remove(&k);
        }
    }
    for want in ["article", "main", "body"] {
        if let Some(k) = order.iter().find(|k| {
            nodes
                .get(k)
                .and_then(|n| dom.name(n))
                .map_or(false, |t| t.eq_ignore_ascii_case(want))
        }) {
            return nodes.remove(k);
        }
    }
    order.first().and_then(|k| nodes.remove(k))
}
