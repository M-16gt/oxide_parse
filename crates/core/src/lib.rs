//! # oxide_parse-core
//! Declarative HTML and JSON extraction: `Expr` (serializable IR) -> [`Plan`] (compiled once) -> [`Value`].
//!
//! * Generic execution ([`Dom`]) and XPath 1.0 evaluation shared across engines, plus a single-pass streaming engine.
//! * Cargo feature flags per engine (`engine-scraper`, `engine-lite`, `engine-stream`, `engine-json`, `yaml`).
//! * Zero IO in the core: `follow` and `paginate` emit [`Request`]s and `adaptive` emits [`RepairRequest`]s via [`Session`].
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "engine-lite")]
//! # {
//! use oxide_parse_core::{compile, Expr, Value};
//!
//! let ir = Expr::from_json(r#"{"object":{"title":{"pipe":[{"css":"h1"},"text","strip"]}}}"#).unwrap();
//! let plan = compile(&ir, "auto").unwrap();
//! let out = plan.extract("<h1>  Laptops  </h1>", Value::Null).unwrap();
//! assert_eq!(out.to_json(), serde_json::json!({"title": "Laptops"}));
//! # }
//! ```
pub mod adaptive;
pub mod dom;
pub mod engine;
pub mod engines;
pub mod error;
pub mod ir;
pub mod markdown;
pub mod value;
pub mod url;
pub mod xpath;

mod exec;
mod jsjson;
mod plan;
mod structured;

#[cfg(feature = "engine-lite")]
pub mod markup;
#[cfg(feature = "engine-lite")]
pub use markup::distill::{distill, distill_container, DistillOptions};

pub use adaptive::{AdaptiveMiss, FileStore, MemoryStore, Store};
pub use dom::{Dom, Walk};
pub use engine::{available, compile, compile_with, Detailed, Engine, EngineInfo};
pub use error::{Error, ErrorInfo, Result};
pub use indexmap::IndexMap;
pub use exec::{RepairRequest, Request, Session};
pub use ir::{Expr, OnError, Op};
pub use plan::Plan;
pub use value::Value;

// short, backwards-compatible module paths
#[cfg(feature = "engine-json")]
pub use engines::json;
#[cfg(feature = "engine-lite")]
pub use engines::lite;
#[cfg(feature = "engine-scraper")]
pub use engines::scraper as html;
#[cfg(feature = "engine-stream")]
pub use engines::stream;
