# CSS Selectors and XPath 1.0 Support

Engine support matrix for `v0.9.0`. Source of truth: `crates/core/src/markup/css.rs` and `crates/core/src/xpath/`.

## CSS Selectors

> **Engines:** `stream` and `lite` share `markup/css.rs`; `scraper` uses the `selectors` / `scraper` crates.

| Construct | `stream` | `lite` | `scraper` | Notes |
|---|---|---|---|---|
| Tag (`div`), `*`, `#id`, `.class` | yes | yes | yes | |
| Selector lists `a, b` | yes | yes | yes | |
| Descendant `a b` and child `a > b` | yes | yes | yes | |
| Sibling combinators `a + b`, `a ~ b` | no | yes | yes | `auto` switches to DOM |
| Attributes `[a]`, `[a=v]`, `~=`, `\|=`, `^=`, `$=`, `*=`, flags `i` & `s` | yes | yes | yes | Duplicate attributes follow first-wins rule |
| `:first-child`, `:nth-child(an+b \| odd \| even)` | yes | yes | yes | |
| `:first-of-type`, `:nth-of-type(an+b \| odd \| even)` | yes | yes | yes | |
| `:root` | yes | yes | yes | |
| `:not(...)`, `:is(...)`, `:where(...)` | yes | yes | yes | Complex arguments supported; nesting depth limit 32 in `lite`/`stream` |
| `:any-link`, `:checked`, `:disabled`, `:enabled` | yes | yes | no | `auto` picks `stream` or `lite` |
| `:last-child`, `:only-child`, `:nth-last-child(...)` | no | yes | yes | `auto` switches to DOM |
| `:last-of-type`, `:only-of-type`, `:nth-last-of-type(...)`, `:empty` | no | yes | yes | `auto` switches to DOM |
| `:has(...)` (including relative `> a`, `+ b`, `~ c`) | no | yes | yes | `auto` switches to DOM |
| `:contains("text")` / `:-soup-contains("text")` | no | yes | no | Case-sensitive substring. Unsupported in `scraper`; pass `backend="lite"` |
| `:scope` (e.g., `:scope > .title`) | yes | yes | no | Unsupported in `scraper`; `auto` and `html` select `stream` or `lite` |
| Pseudo-elements `::text`, `::attr(x)` | no | no | no | Chain `.text()` and `.attr("x")` on `T` instead |

Performance note for `:contains(...)`: candidate element text is collected per match test, so avoid `*:contains("...")` and anchor by tag or class (`li:contains("Sale")`).

Streaming note: `stream` only tracks top-level single captures and `css(".card").map({...})`; nested `.map(...)` inside another `.map(...)` requires a DOM engine.

---

## XPath 1.0

> **Engines:** `lite` · `scraper` | **Fallback on `auto`:** switches to DOM (`scraper` or `lite`)

Implemented in `crates/core/src/xpath/` over the shared `Dom` trait, so `lite` and `scraper` behave identically. Parser expression nesting limit is 64 levels.

### Axes

| Axis | Support |
|---|---|
| `child`, `descendant`, `descendant-or-self` (`//`) | yes |
| `parent` (`..`), `ancestor`, `ancestor-or-self`, `self` (`.`) | yes |
| `following-sibling`, `preceding-sibling` | yes |
| `attribute` (`@attr`) | yes |
| `following`, `preceding`, `namespace` | no (compile error) |

### Node Tests, Operators, and Functions

- **Node tests:** tag name, `*`, `text()`, `node()`, `comment()`, `processing-instruction()`.
- **Operators:** `|`, `and`, `or`, `=`, `!=`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `div`, `mod`, unary `-`.
- **Functions (29):**
  - nodes & positions: `position()`, `last()`, `count()`, `name()`, `local-name()`, `lang()`;
  - logic & math: `true()`, `false()`, `not()`, `boolean()`, `number()`, `sum()`, `floor()`, `ceiling()`, `round()`;
  - strings: `string()`, `string-length()`, `normalize-space()`, `lower-case()`, `upper-case()`, `contains()`, `starts-with()`, `ends-with()`, `substring()`, `substring-before()`, `substring-after()`, `concat()`, `translate()`, `matches()`.

### Return Types and Limits

Semantics follow Parsel conventions:
- Element paths (`//div[@class="card"]`) return a node set for `.text()`, `.attr()`, `.map(...)`.
- Paths ending in `@attr` or `text()` (`//h1/text()`, `.//a/@href`) return a list of strings (pick one with `.first()` or `.at(0)`).
- Top-level function calls (`count(//li)`, `string(//h1)`) return a number, string, or `bool`.

Constraints:
1. `@attr` and `text()` steps are only valid as the **final** path step.
2. XML namespaces are not supported (qualified attribute names like `@xml:lang` work by attribute name lookup).
3. `//text()` preserves element-by-element order rather than interleaving mixed child text nodes across sibling elements.
