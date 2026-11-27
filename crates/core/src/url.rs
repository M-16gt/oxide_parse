//! RFC 3986 reference resolution (section 5.2) without dependencies: `/catalog/x`, `../img.png`, `//cdn.x/a.js`,
//! `?page=2`, `#top` against a base URL. No percent-encoding normalisation, no IDNA, no `<base href>`.

struct Parts<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

fn split(u: &str) -> Parts<'_> {
    let (u, fragment) = match u.find('#') {
        Some(i) => (&u[..i], Some(&u[i + 1..])),
        None => (u, None),
    };
    let (u, query) = match u.find('?') {
        Some(i) => (&u[..i], Some(&u[i + 1..])),
        None => (u, None),
    };
    let mut scheme = None;
    let mut rest = u;
    if let Some(i) = u.find(':') {
        let s = &u[..i];
        let valid = s.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
        if valid {
            scheme = Some(s);
            rest = &u[i + 1..];
        }
    }
    let (authority, path) = match rest.strip_prefix("//") {
        Some(r) => {
            let end = r.find('/').unwrap_or(r.len());
            (Some(&r[..end]), &r[end..])
        }
        None => (None, rest),
    };
    Parts { scheme, authority, path, query, fragment }
}

fn remove_dots(path: &str) -> String {
    let abs = path.starts_with('/');
    let segs: Vec<&str> = path.split('/').collect();
    let n = segs.len();
    let mut out: Vec<&str> = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        if i == 0 && abs {
            continue;
        }
        match *seg {
            "." => {
                if i == n - 1 {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if i == n - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if abs {
        format!("/{joined}")
    } else {
        joined
    }
}

fn merge(base: &Parts<'_>, rel_path: &str) -> String {
    if base.authority.is_some() && base.path.is_empty() {
        return format!("/{rel_path}");
    }
    match base.path.rfind('/') {
        Some(i) => format!("{}{}", &base.path[..=i], rel_path),
        None => rel_path.to_string(),
    }
}

/// Resolve `rel` against `base`. An absolute `rel` (has a scheme) is returned as it is.
pub fn resolve(base: Option<&str>, rel: &str) -> Result<String, String> {
    let r = split(rel);
    if r.scheme.is_some() {
        return Ok(rel.to_string());
    }
    let base = base.ok_or_else(|| "no base URL: pass meta={\"base_url\": ...} or url(base=...) (the page has no <base href> either)".to_string())?;
    let b = split(base);
    let scheme = b.scheme.ok_or_else(|| format!("the base URL {base:?} is not absolute"))?;
    let (authority, path, query): (Option<&str>, String, Option<&str>) = if r.authority.is_some() {
        (r.authority, remove_dots(r.path), r.query)
    } else if r.path.is_empty() {
        (b.authority, b.path.to_string(), r.query.or(b.query))
    } else if r.path.starts_with('/') {
        (b.authority, remove_dots(r.path), r.query)
    } else {
        (b.authority, remove_dots(&merge(&b, r.path)), r.query)
    };
    let mut out = format!("{scheme}:");
    if let Some(a) = authority {
        out.push_str("//");
        out.push_str(a);
    }
    out.push_str(&path);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = r.fragment {
        out.push('#');
        out.push_str(f);
    }
    Ok(out)
}

/// Parse an HTTP `Link` header (RFC 8288) into `{rel: url}` (first occurrence wins per relation token).
pub(crate) fn parse_link_header(header: &str) -> indexmap::IndexMap<String, String> {
    let mut out = indexmap::IndexMap::new();
    let mut entries: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut in_angles = false;
    let mut in_quotes = false;
    let mut escape = false;
    for (i, ch) in header.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_quotes {
            if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_quotes = false;
            }
            continue;
        }
        match ch {
            '"' => in_quotes = true,
            '<' => in_angles = true,
            '>' => in_angles = false,
            ',' if !in_angles => {
                entries.push(&header[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    entries.push(&header[start..]);

    for raw_entry in entries {
        let entry = raw_entry.trim();
        let Some(open) = entry.find('<') else { continue };
        let Some(rel_close) = entry[open + 1..].find('>') else { continue };
        let close = open + 1 + rel_close;
        let uri = entry[open + 1..close].trim();
        if uri.is_empty() {
            continue;
        }
        let rest = &entry[close + 1..];
        let mut params: Vec<&str> = Vec::new();
        let mut p_start = 0usize;
        let mut p_quotes = false;
        let mut p_escape = false;
        for (j, ch) in rest.char_indices() {
            if p_escape {
                p_escape = false;
                continue;
            }
            if p_quotes {
                if ch == '\\' {
                    p_escape = true;
                } else if ch == '"' {
                    p_quotes = false;
                }
                continue;
            }
            match ch {
                '"' => p_quotes = true,
                ';' => {
                    params.push(&rest[p_start..j]);
                    p_start = j + 1;
                }
                _ => {}
            }
        }
        params.push(&rest[p_start..]);

        for param in params {
            let param = param.trim();
            let Some((k, v)) = param.split_once('=') else { continue };
            if !k.trim().eq_ignore_ascii_case("rel") {
                continue;
            }
            let val = v.trim();
            let unquoted = if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
                &val[1..val.len() - 1]
            } else {
                val
            };
            for tok in unquoted.split_ascii_whitespace() {
                let rel_key = tok.to_ascii_lowercase();
                out.entry(rel_key).or_insert_with(|| uri.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn rfc3986_examples() {
        let base = "http://a/b/c/d;p?q";
        let cases = [
            ("g:h", "g:h"),
            ("g", "http://a/b/c/g"),
            ("./g", "http://a/b/c/g"),
            ("g/", "http://a/b/c/g/"),
            ("/g", "http://a/g"),
            ("//g", "http://g"),
            ("?y", "http://a/b/c/d;p?y"),
            ("g?y", "http://a/b/c/g?y"),
            ("#s", "http://a/b/c/d;p?q#s"),
            ("g#s", "http://a/b/c/g#s"),
            ("", "http://a/b/c/d;p?q"),
            (".", "http://a/b/c/"),
            ("./", "http://a/b/c/"),
            ("..", "http://a/b/"),
            ("../", "http://a/b/"),
            ("../g", "http://a/b/g"),
            ("../..", "http://a/"),
            ("../../g", "http://a/g"),
            ("../../../g", "http://a/g"),
            ("/./g", "http://a/g"),
            ("g.", "http://a/b/c/g."),
            ("g/./h", "http://a/b/c/g/h"),
            ("g/../h", "http://a/b/c/h"),
        ];
        for (rel, want) in cases {
            assert_eq!(resolve(Some(base), rel).unwrap(), want, "rel = {rel:?}");
        }
    }

    #[test]
    fn errors_and_passthrough() {
        assert!(resolve(None, "/x").is_err());
        assert!(resolve(Some("/relative/base"), "x").is_err());
        assert_eq!(resolve(None, "https://x.y/z").unwrap(), "https://x.y/z");
        assert_eq!(resolve(Some("https://shop.example"), "/cat/1").unwrap(), "https://shop.example/cat/1");
        assert_eq!(resolve(Some("https://shop.example/a/b.html"), "c.png").unwrap(), "https://shop.example/a/c.png");
    }
}
