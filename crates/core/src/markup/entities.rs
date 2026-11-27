//! HTML5 character references with distinct text and attribute contexts.
use std::borrow::Cow;

/// Таблица маппинга 0x80..=0x9F по стандарту WHATWG HTML5 (Windows-1252)
const WINDOWS_1252_MAP: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}',
    '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}',
    '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}',
    '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

fn map_html5_codepoint(code: u32) -> char {
    match code {
        0x00 => '\u{FFFD}',
        0x80..=0x9F => WINDOWS_1252_MAP[(code - 0x80) as usize],
        0xD800..=0xDFFF => '\u{FFFD}',
        c if c > 0x10FFFF => '\u{FFFD}',
        c => char::from_u32(c).unwrap_or('\u{FFFD}'),
    }
}

/// Декодирует числовую сущность начиная сразу после "&#".
/// Возвращает `(декодированный_символ, число_поглощённых_байт)`.
pub fn parse_numeric_entity(input: &str) -> Option<(char, usize)> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let (hex, mut idx) = if bytes[0] == b'x' || bytes[0] == b'X' {
        (true, 1)
    } else {
        (false, 0)
    };

    let digits_start = idx;
    let mut acc: u32 = 0;
    let mut overflow = false;

    while idx < bytes.len() {
        let b = bytes[idx];
        let digit = if hex {
            match b {
                b'0'..=b'9' => Some((b - b'0') as u32),
                b'a'..=b'f' => Some((b - b'a' + 10) as u32),
                b'A'..=b'F' => Some((b - b'A' + 10) as u32),
                _ => None,
            }
        } else {
            match b {
                b'0'..=b'9' => Some((b - b'0') as u32),
                _ => None,
            }
        };

        let Some(d) = digit else {
            break;
        };

        if !overflow {
            let base = if hex { 16 } else { 10 };
            if let Some(next) = acc.checked_mul(base).and_then(|v| v.checked_add(d)) {
                acc = next;
            } else {
                overflow = true;
            }
        }
        idx += 1;
    }

    // Если не было ни одной цифры (например, "&#;" или "&#xG;"), сущность невалидна
    if idx == digits_start {
        return None;
    }

    // Необязательная точка с запятой поглощается, если присутствует
    if idx < bytes.len() && bytes[idx] == b';' {
        idx += 1;
    }

    let ch = if overflow { '\u{FFFD}' } else { map_html5_codepoint(acc) };

    Some((ch, idx))
}

/// Longest HTML5 name match. Attribute values disallow ambiguous legacy references.
pub fn parse_named_entity(input: &str, in_attribute: bool) -> Option<(&'static str, usize)> {
    let bytes = input.as_bytes();
    let alpha = bytes.iter().take(32).take_while(|c| c.is_ascii_alphanumeric()).count();
    // every HTML5 name has at least two characters
    if alpha < 2 {
        return None;
    }
    let table = super::html_entities::NAMED_ENTITIES;
    // 1. the full name with its `;`: a single binary search
    if bytes.get(alpha) == Some(&b';') {
        let candidate = &input[..alpha + 1];
        if let Ok(index) = table.binary_search_by_key(&candidate, |(name, _)| *name) {
            return Some((table[index].1, alpha + 1));
        }
    }
    // 2. legacy names (no `;`): the table has 106 of them, 2..=6 characters long, longest match wins
    for length in (2..=alpha.min(6)).rev() {
        let candidate = &input[..length];
        if let Ok(index) = table.binary_search_by_key(&candidate, |(name, _)| *name) {
            if in_attribute
                && bytes
                    .get(length)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'=')
            {
                continue;
            }
            return Some((table[index].1, length));
        }
    }
    None
}

pub fn decode(s: &str) -> Cow<'_, str> {
    decode_context(s, false)
}

pub fn decode_attr(s: &str) -> Cow<'_, str> {
    decode_context(s, true)
}

fn decode_context(s: &str, in_attribute: bool) -> Cow<'_, str> {
    let mut pos = match memchr::memchr(b'&', s.as_bytes()) {
        Some(p) => p,
        None => return Cow::Borrowed(s),
    };
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        if let Some((ch, used)) = rest.strip_prefix('#').and_then(parse_numeric_entity) {
            out.push(ch);
            rest = &rest[used + 1..];
        } else if let Some((value, used)) = parse_named_entity(rest, in_attribute) {
            out.push_str(value);
            rest = &rest[used..];
        } else {
            out.push('&');
        }
        pos = match memchr::memchr(b'&', rest.as_bytes()) {
            Some(p) => p,
            None => break,
        };
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_names_and_unicode_boundaries() {
        assert_eq!(decode("&notin; &notit; &NotEqualTilde;"), "∉ ¬it; ≂̸");
        assert_eq!(decode("&amp;текст &unknown;"), "&текст &unknown;");
        assert_eq!(decode(&format!("&{}", "я".repeat(40))), format!("&{}", "я".repeat(40)));
    }

    #[test]
    fn ambiguous_names_are_preserved_only_in_attributes() {
        assert_eq!(decode("&notit; &copy= &COPY"), "¬it; ©= ©");
        assert_eq!(decode_attr("&notit; &copy= &COPY"), "&notit; &copy= ©");
        assert_eq!(decode_attr("&notin; &amp;"), "∉ &");
    }

    #[test]
    fn false_ampersands_and_legacy_names() {
        assert_eq!(decode("?a=1&destination=2 &a; &x"), "?a=1&destination=2 &a; &x");
        assert_eq!(decode("&frac12 &frac12; &plusmn5 &amp"), "½ ½ ±5 &");
        assert_eq!(decode_attr("&ampx &amp=1 &amp;"), "&ampx &amp=1 &");
    }

    #[test]
    fn numeric_references_handle_overflow_and_missing_semicolons() {
        assert_eq!(decode("&#128; &#xD800; &#0; &#11111111111; &#97a"), "€ � � � aa");
        assert_eq!(decode_attr("&#x80 &#xG; &#;"), "€ &#xG; &#;");
    }
}
