use thiserror::Error;

/// All errors produced by the engine. Execution errors carry a path such as `$.items[3].price`.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    #[error("IR error: {0}")]
    Ir(String),
    #[error("compile error at `{path}`: {msg}")]
    Compile { path: String, msg: String },
    #[error("extraction error at `{path}`: {msg}")]
    Exec { path: String, msg: String },
    #[error("session error: {0}")]
    Session(String),
    #[error("document error: {0}")]
    Doc(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn fmt_path(segs: &[String]) -> String {
    let mut s = String::from("$");
    for g in segs {
        if g.starts_with('[') {
            s.push_str(g);
        } else {
            s.push('.');
            s.push_str(g);
        }
    }
    s
}

impl Error {
    /// Put `prefix` (e.g. `["items", "[3]", "price"]`) in front of the path of an execution error.
    pub(crate) fn prefixed(self, prefix: &[String]) -> Error {
        match self {
            Error::Exec { path, msg } => {
                let rest = path.strip_prefix('$').unwrap_or(&path).to_string();
                Error::Exec { path: format!("{}{}", fmt_path(prefix), rest), msg }
            }
            other => other,
        }
    }
}

/// A failure that did not abort the extraction (quarantined by `map(..., on_error=...)`).
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorInfo {
    /// e.g. `$.items[41].price`
    pub path: String,
    pub message: String,
}
