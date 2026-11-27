//! Engines. Each one lives behind a cargo feature so you only compile what you use.
//!
//! | feature          | engine  | kind   | notes                                                        |
//! |------------------|---------|--------|--------------------------------------------------------------|
//! | `engine-scraper` | scraper | DOM    | html5ever: spec-correct tree, full CSS, XPath (generic)      |
//! | `engine-lite`    | lite    | DOM    | own zero-copy parser + arena DOM, CSS, XPath (generic)       |
//! | `engine-stream`  | stream  | stream | no DOM, one pass; falls back to `lite` for complex schemas   |
//! | `engine-json`    | json    | data   | same schemas over JSON documents                             |
#[cfg(feature = "engine-json")]
pub mod json;
#[cfg(feature = "engine-lite")]
pub mod lite;
#[cfg(feature = "engine-scraper")]
pub mod scraper;
#[cfg(feature = "engine-stream")]
pub mod stream;
