//! XPath 1.0 lexer + parser (the subset that makes sense for HTML scraping).
//!
//! Supported: absolute/relative paths, `//`, `.`/`..`, `@attr`, axes `child descendant
//! descendant-or-self parent ancestor ancestor-or-self self following-sibling preceding-sibling attribute`,
//! node tests `name * text() node()`, predicates, `| and or = != < <= > >= + - * div mod`, function calls.
//! `@attr` and `text()` steps are only allowed as the *last* step of a path (they yield strings).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Parent,
    Ancestor,
    AncestorOrSelf,
    SelfAxis,
    FollowingSibling,
    PrecedingSibling,
    Attribute,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    Name(String),
    Any,
    Text,
    Node,
    Never,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub axis: Axis,
    pub test: Test,
    pub preds: Vec<Expr>,
}

impl Step {
    /// Steps that produce strings instead of elements.
    pub fn is_terminal(&self) -> bool {
        self.axis == Axis::Attribute || self.test == Test::Text
    }
}

#[derive(Debug, Clone)]
pub enum Start {
    Root,
    Context,
    Expr(Box<Expr>, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Union,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Path { start: Start, steps: Vec<Step> },
    Literal(String),
    Number(f64),
    Func(String, Vec<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
}

/// A parsed XPath expression.
#[derive(Debug, Clone)]
pub struct XPathExpr {
    pub(crate) expr: Expr,
    pub(crate) src: String,
}

impl XPathExpr {
    pub fn source(&self) -> &str {
        &self.src
    }
}

// ---------------------------------------------------------------------------------------------
// lexer
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Slash,
    DSlash,
    Dot,
    DotDot,
    At,
    DColon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Pipe,
    Star,
    Plus,
    Minus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Literal(String),
    Number(f64),
    Name(String),
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '/' => {
                if next == Some('/') {
                    out.push(Tok::DSlash);
                    i += 2;
                } else {
                    out.push(Tok::Slash);
                    i += 1;
                }
            }
            '.' => {
                if next == Some('.') {
                    out.push(Tok::DotDot);
                    i += 2;
                } else if next.map_or(false, |n| n.is_ascii_digit()) {
                    let (n, len) = lex_number(&chars[i..])?;
                    out.push(Tok::Number(n));
                    i += len;
                } else {
                    out.push(Tok::Dot);
                    i += 1;
                }
            }
            '@' => {
                out.push(Tok::At);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '|' => {
                out.push(Tok::Pipe);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '=' => {
                out.push(Tok::Eq);
                i += 1;
            }
            '!' => {
                if next == Some('=') {
                    out.push(Tok::Ne);
                    i += 2;
                } else {
                    return Err("unexpected '!'".into());
                }
            }
            '<' => {
                if next == Some('=') {
                    out.push(Tok::Le);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            '>' => {
                if next == Some('=') {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            ':' => {
                if next == Some(':') {
                    out.push(Tok::DColon);
                    i += 2;
                } else {
                    return Err("unexpected ':'".into());
                }
            }
            '"' | '\'' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != c {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err("unterminated string literal".into());
                }
                out.push(Tok::Literal(chars[i + 1..j].iter().collect()));
                i = j + 1;
            }
            d if d.is_ascii_digit() => {
                let (n, len) = lex_number(&chars[i..])?;
                out.push(Tok::Number(n));
                i += len;
            }
            n if is_name_start(n) => {
                let mut j = i;
                while j < chars.len() {
                    let cj = chars[j];
                    if is_name_char(cj) {
                        j += 1;
                    } else if cj == ':' && chars.get(j + 1) == Some(&'*') {
                        j += 2; // QName wildcard, e.g. atom:*
                        break;
                    } else if cj == ':' && chars.get(j + 1).map_or(false, |&x| x != ':' && is_name_start(x)) {
                        j += 1; // QName prefix, e.g. xml:lang or atom:link
                    } else {
                        break;
                    }
                }
                out.push(Tok::Name(chars[i..j].iter().collect()));
                i = j;
            }
            other => return Err(format!("unexpected character {other:?}")),
        }
    }
    Ok(out)
}

fn lex_number(chars: &[char]) -> Result<(f64, usize), String> {
    let mut j = 0;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j < chars.len() && chars[j] == '.' {
        j += 1;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
    }
    let s: String = chars[..j].iter().collect();
    s.parse::<f64>().map(|n| (n, j)).map_err(|_| format!("bad number {s:?}"))
}

// ---------------------------------------------------------------------------------------------
// parser
// ---------------------------------------------------------------------------------------------

/// Nesting limit: deeply nested `((((...))))` or `----1` must be an error, not a stack overflow
/// (with `panic = "abort"` that would take the whole Python process down).
const MAX_DEPTH: usize = 64;

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

const NODE_TYPES: [&str; 4] = ["text", "node", "comment", "processing-instruction"];

fn axis_from(name: &str) -> Result<Axis, String> {
    Ok(match name {
        "child" => Axis::Child,
        "descendant" => Axis::Descendant,
        "descendant-or-self" => Axis::DescendantOrSelf,
        "parent" => Axis::Parent,
        "ancestor" => Axis::Ancestor,
        "ancestor-or-self" => Axis::AncestorOrSelf,
        "self" => Axis::SelfAxis,
        "following-sibling" => Axis::FollowingSibling,
        "preceding-sibling" => Axis::PrecedingSibling,
        "attribute" => Axis::Attribute,
        "following" | "preceding" | "namespace" => return Err(format!("axis `{name}` is not supported")),
        other => return Err(format!("unknown axis `{other}`")),
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn peek_at(&self, off: usize) -> Option<&Tok> {
        self.toks.get(self.pos + off)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: Tok) -> Result<(), String> {
        if self.eat(&t) {
            Ok(())
        } else {
            Err(format!("expected {t:?}, found {:?}", self.peek()))
        }
    }
    fn peek_name(&self, n: &str) -> bool {
        matches!(self.peek(), Some(Tok::Name(s)) if s == n)
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("expression nesting is too deep".into());
        }
        Ok(())
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        self.enter()?;
        let r = self.parse_or_inner();
        self.depth -= 1;
        r
    }

    fn parse_or_inner(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_and()?;
        while self.peek_name("or") {
            self.pos += 1;
            let r = self.parse_and()?;
            l = Expr::Binary(BinOp::Or, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_eq()?;
        while self.peek_name("and") {
            self.pos += 1;
            let r = self.parse_eq()?;
            l = Expr::Binary(BinOp::And, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_eq(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_rel()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Eq) => BinOp::Eq,
                Some(Tok::Ne) => BinOp::Ne,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_rel()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_rel(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => BinOp::Lt,
                Some(Tok::Le) => BinOp::Le,
                Some(Tok::Gt) => BinOp::Gt,
                Some(Tok::Ge) => BinOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_add()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_mul()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_unary()?;
        loop {
            let op = if self.peek() == Some(&Tok::Star) {
                BinOp::Mul
            } else if self.peek_name("div") {
                BinOp::Div
            } else if self.peek_name("mod") {
                BinOp::Mod
            } else {
                break;
            };
            self.pos += 1;
            let r = self.parse_unary()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.eat(&Tok::Minus) {
            self.enter()?;
            let e = self.parse_unary();
            self.depth -= 1;
            return Ok(Expr::Neg(Box::new(e?)));
        }
        self.parse_union()
    }

    fn parse_union(&mut self) -> Result<Expr, String> {
        let mut l = self.parse_path()?;
        while self.eat(&Tok::Pipe) {
            let r = self.parse_path()?;
            l = Expr::Binary(BinOp::Union, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Name(_)) | Some(Tok::Star) | Some(Tok::At) | Some(Tok::Dot) | Some(Tok::DotDot)
        )
    }

    fn is_function_call(&self) -> bool {
        match (self.peek(), self.peek_at(1)) {
            (Some(Tok::Name(n)), Some(Tok::LParen)) => !NODE_TYPES.contains(&n.as_str()),
            _ => false,
        }
    }

    fn parse_path(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Slash) => {
                self.pos += 1;
                let mut steps = Vec::new();
                if self.starts_step() {
                    self.parse_steps(&mut steps)?;
                }
                make_path(Start::Root, steps)
            }
            Some(Tok::DSlash) => {
                self.pos += 1;
                let mut steps = vec![dos_step()];
                self.parse_steps(&mut steps)?;
                make_path(Start::Root, steps)
            }
            Some(Tok::Literal(_)) | Some(Tok::Number(_)) | Some(Tok::LParen) => self.parse_filter(),
            _ if self.is_function_call() => self.parse_filter(),
            _ => {
                let mut steps = Vec::new();
                self.parse_steps(&mut steps)?;
                make_path(Start::Context, steps)
            }
        }
    }

    /// `PrimaryExpr Predicate* (('/' | '//') RelativeLocationPath)?`
    fn parse_filter(&mut self) -> Result<Expr, String> {
        let primary = self.parse_primary()?;
        let mut preds = Vec::new();
        while self.eat(&Tok::LBracket) {
            preds.push(self.parse_or()?);
            self.expect(Tok::RBracket)?;
        }
        let mut steps = Vec::new();
        if self.eat(&Tok::Slash) {
            self.parse_steps(&mut steps)?;
        } else if self.eat(&Tok::DSlash) {
            steps.push(dos_step());
            self.parse_steps(&mut steps)?;
        }
        if preds.is_empty() && steps.is_empty() {
            return Ok(primary);
        }
        make_path(Start::Expr(Box::new(primary), preds), steps)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Some(Tok::LParen) => {
                let e = self.parse_or()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Literal(s)) => Ok(Expr::Literal(s)),
            Some(Tok::Number(n)) => Ok(Expr::Number(n)),
            Some(Tok::Name(name)) => {
                self.expect(Tok::LParen)?;
                let mut args = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        args.push(self.parse_or()?);
                        if self.eat(&Tok::Comma) {
                            continue;
                        }
                        self.expect(Tok::RParen)?;
                        break;
                    }
                }
                Ok(Expr::Func(name, args))
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }

    fn parse_steps(&mut self, steps: &mut Vec<Step>) -> Result<(), String> {
        steps.push(self.parse_step()?);
        loop {
            if self.eat(&Tok::Slash) {
                steps.push(self.parse_step()?);
            } else if self.eat(&Tok::DSlash) {
                steps.push(dos_step());
                steps.push(self.parse_step()?);
            } else {
                return Ok(());
            }
        }
    }

    fn parse_step(&mut self) -> Result<Step, String> {
        if self.eat(&Tok::Dot) {
            return Ok(Step { axis: Axis::SelfAxis, test: Test::Node, preds: self.parse_preds()? });
        }
        if self.eat(&Tok::DotDot) {
            return Ok(Step { axis: Axis::Parent, test: Test::Node, preds: self.parse_preds()? });
        }
        let mut axis = Axis::Child;
        if self.eat(&Tok::At) {
            axis = Axis::Attribute;
        } else if let (Some(Tok::Name(n)), Some(Tok::DColon)) = (self.peek(), self.peek_at(1)) {
            axis = axis_from(n)?;
            self.pos += 2;
        }
        let test = match self.bump() {
            Some(Tok::Star) => Test::Any,
            Some(Tok::Name(n)) => {
                if self.peek() == Some(&Tok::LParen) && NODE_TYPES.contains(&n.as_str()) {
                    self.pos += 1;
                    self.expect(Tok::RParen)?;
                    match n.as_str() {
                        "text" => Test::Text,
                        "node" => Test::Node,
                        _ => Test::Never,
                    }
                } else {
                    Test::Name(n)
                }
            }
            other => return Err(format!("expected a node test, found {other:?}")),
        };
        Ok(Step { axis, test, preds: self.parse_preds()? })
    }

    fn parse_preds(&mut self) -> Result<Vec<Expr>, String> {
        let mut preds = Vec::new();
        while self.eat(&Tok::LBracket) {
            preds.push(self.parse_or()?);
            self.expect(Tok::RBracket)?;
        }
        Ok(preds)
    }
}

fn dos_step() -> Step {
    Step { axis: Axis::DescendantOrSelf, test: Test::Node, preds: Vec::new() }
}

fn make_path(start: Start, steps: Vec<Step>) -> Result<Expr, String> {
    let last = steps.len().saturating_sub(1);
    for (i, s) in steps.iter().enumerate() {
        if s.is_terminal() {
            if i != last {
                return Err("`@attr` and `text()` steps are only supported as the last step of a path".into());
            }
            if !s.preds.is_empty() {
                return Err("predicates on `@attr` / `text()` steps are not supported".into());
            }
            if s.axis == Axis::Attribute && !matches!(s.test, Test::Name(_) | Test::Any) {
                return Err("attribute axis needs a name or `*`".into());
            }
            if s.test == Test::Text && !matches!(s.axis, Axis::Child | Axis::Descendant | Axis::DescendantOrSelf) {
                return Err("text() is only supported on the child/descendant axes".into());
            }
        }
    }
    Ok(Expr::Path { start, steps })
}

pub fn parse(src: &str) -> Result<XPathExpr, String> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("empty XPath".into());
    }
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let expr = p.parse_or()?;
    if p.pos != p.toks.len() {
        return Err(format!("unexpected trailing token {:?}", p.toks[p.pos]));
    }
    Ok(XPathExpr { expr, src: src.to_string() })
}
