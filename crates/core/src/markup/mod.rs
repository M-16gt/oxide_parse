//! Shared markup runtime used by the `lite` (arena DOM) and `stream` (no DOM) engines:
//!
//! ```text
//! &str --tokenizer--> Token --events (tree-construction rules)--> Event --+--> arena::Doc   (lite)
//!                                                                          +--> stream matcher (stream)
//! ```
//! Everything is zero-copy: names, attribute text and text nodes are slices of the input.
pub mod arena;
pub mod css;
pub mod distill;
pub mod entities;
pub mod events;
mod html_entities;
pub mod tokenizer;
