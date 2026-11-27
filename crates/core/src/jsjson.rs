//! Pull a JSON value out of JavaScript source: `window.__STATE__ = {...};`, `var d = [...]`,
//! `x = JSON.parse("...")`. Only JSON-compatible literals are supported (no unquoted keys, no `undefined`).

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Index just after the `=` of `var = ...` (not `==`, `===`, `=>`).
fn find_assignment(text: &str, var: &str) -> Option<usize> {
    for (pos, _) in text.match_indices(var) {
        if text[..pos].chars().next_back().map_or(false, |c| ident_char(c) || c == '.') {
            continue;
        }
        let after = &text[pos + var.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            if !rest.starts_with('=') && !rest.starts_with('>') {
                return Some(text.len() - rest.len());
            }
        }
    }
    None
}

fn skip_line_comment(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(b: &[u8], mut i: usize) -> usize {
    i += 2;
    while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
        i += 1;
    }
    i + 2
}

/// The `{...}` / `[...]` at the start of `s`. Brackets inside strings and `//`, `/* */` comments do not count.
pub(crate) fn balanced(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i = skip_line_comment(b, i);
            continue;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(b, i);
            continue;
        } else {
            match c {
                b'"' | b'\'' => in_str = Some(c),
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[..=i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// JS object literals may carry comments and trailing commas, JSON may not: remove both (outside strings).
fn to_json_text(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = in_str {
            out.push(c);
            if c == b'\\' {
                if let Some(&n) = b.get(i + 1) {
                    out.push(n);
                }
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i = skip_line_comment(b, i);
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(b, i);
        } else if c == b'"' || c == b'\'' {
            in_str = Some(c);
            out.push(c);
            i += 1;
        } else if c == b',' {
            // a trailing comma: the next significant byte (skipping blanks and comments) closes the container
            let mut j = i + 1;
            loop {
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'/') {
                    j = skip_line_comment(b, j);
                } else if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'*') {
                    j = skip_block_comment(b, j);
                } else {
                    break;
                }
            }
            if !matches!(b.get(j).copied(), Some(b'}') | Some(b']')) {
                out.push(c);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn hex(it: &mut std::str::Chars<'_>, n: usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..n {
        v = v * 16 + it.next()?.to_digit(16)?;
    }
    Some(v)
}

/// Decode a JS string literal at the start of `s`.
fn js_string(s: &str) -> Result<String, String> {
    let mut it = s.chars();
    let q = it.next().ok_or("empty input")?;
    if q != '"' && q != '\'' {
        return Err("JSON.parse(...) needs a string literal".into());
    }
    let mut out = String::new();
    while let Some(c) = it.next() {
        if c == q {
            return Ok(out);
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next().ok_or("unterminated escape")? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '0' => out.push('\0'),
            'x' => out.push(char::from_u32(hex(&mut it, 2).ok_or("bad \\x escape")?).unwrap_or('\u{fffd}')),
            'u' => {
                let hi = hex(&mut it, 4).ok_or("bad \\u escape")?;
                if (0xD800..0xDC00).contains(&hi) {
                    let mut look = it.clone();
                    if look.next() == Some('\\') && look.next() == Some('u') {
                        if let Some(lo) = hex(&mut look, 4) {
                            if (0xDC00..0xE000).contains(&lo) {
                                it = look;
                                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                                continue;
                            }
                        }
                    }
                    out.push('\u{fffd}');
                } else {
                    out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
                }
            }
            other => out.push(other),
        }
    }
    Err("unterminated string literal".into())
}

/// `var`: the assignment target (`window.__STATE__`); `None` = the first `{...}` / `[...]` in the text.
pub fn extract(text: &str, var: Option<&str>) -> Result<serde_json::Value, String> {
    let rest = match var {
        Some(v) => {
            let at = find_assignment(text, v).ok_or_else(|| format!("no assignment to {v:?} found"))?;
            text[at..].trim_start()
        }
        None => match text.find(|c| c == '{' || c == '[') {
            Some(i) => &text[i..],
            None => return Err("no JSON object or array found".into()),
        },
    };
    if let Some(call) = rest.strip_prefix("JSON.parse") {
        let inner = call.trim_start().strip_prefix('(').ok_or("malformed JSON.parse call")?.trim_start();
        let s = js_string(inner)?;
        return serde_json::from_str(&s).map_err(|e| format!("invalid JSON in JSON.parse: {e}"));
    }
    if !(rest.starts_with('{') || rest.starts_with('[')) {
        return Err("expected an object or array literal after `=`".into());
    }
    let obj = balanced(rest).ok_or("unbalanced brackets")?;
    serde_json::from_str(&to_json_text(obj))
        .map_err(|e| format!("not valid JSON (comments and trailing commas are accepted, other JS-only syntax is not): {e}"))
}

#[cfg(test)]
mod tests {
    use super::extract;
    use serde_json::json;

    #[test]
    fn assignments() {
        let js = r#"var a = 1; window.__S = {"a":[1,2,{"b":"x}"}], "c": 'no'}; foo()"#;
        // single-quoted JS strings are not JSON: reported, not guessed
        assert!(extract(js, Some("window.__S")).is_err());
        let js = r#"x.window.__S = 1; window.__S = {"a":[1,2,{"b":"x}"}]}; foo()"#;
        assert_eq!(extract(js, Some("window.__S")).unwrap(), json!({"a": [1, 2, {"b": "x}"}]}));
        assert!(extract("window.__S == {}", Some("window.__S")).is_err());
        assert_eq!(extract("let q = [1,2];", None).unwrap(), json!([1, 2]));
    }

    #[test]
    fn comments_and_trailing_commas() {
        let js = "window.__S = { // don't touch\n \"a\": [1, 2, /* } ] */ 3,], \"b\": \"x // not a comment\", };";
        assert_eq!(extract(js, Some("window.__S")).unwrap(), json!({"a": [1, 2, 3], "b": "x // not a comment"}));
        // a comment after the object does not matter, and a brace inside a comment does not end it early
        let js = "var d = {\"k\": 1 /* } */}; // don't";
        assert_eq!(extract(js, Some("d")).unwrap(), json!({"k": 1}));
    }

    #[test]
    fn json_parse_call() {
        let js = r#"window.__D = JSON.parse("{\"a\":\"\\u00e9\\n\",\"b\":[1]}");"#;
        assert_eq!(extract(js, Some("window.__D")).unwrap(), json!({"a": "é\n", "b": [1]}));
    }
}
