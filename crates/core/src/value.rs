use indexmap::IndexMap;
use serde::Serialize;

/// Output value of an extraction. Maps keep insertion order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
    /// Internal: placeholder for a not-yet-fetched `follow` result. Never present in a finished result.
    #[serde(skip_serializing)]
    Pending(u64),
}

impl Value {
    pub fn from_json(j: serde_json::Value) -> Value {
        use serde_json::Value as J;
        match j {
            J::Null => Value::Null,
            J::Bool(b) => Value::Bool(b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            J::String(s) => Value::Str(s),
            J::Array(a) => Value::List(a.into_iter().map(Value::from_json).collect()),
            J::Object(o) => Value::Map(o.into_iter().map(|(k, v)| (k, Value::from_json(v))).collect()),
        }
    }

    /// Same as [`Value::from_json`] but borrows (the source stays usable, e.g. a cached parsed document).
    pub fn from_json_ref(j: &serde_json::Value) -> Value {
        use serde_json::Value as J;
        match j {
            J::Null => Value::Null,
            J::Bool(b) => Value::Bool(*b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            J::String(s) => Value::Str(s.clone()),
            J::Array(a) => Value::List(a.iter().map(Value::from_json_ref).collect()),
            J::Object(o) => Value::Map(o.iter().map(|(k, v)| (k.clone(), Value::from_json_ref(v))).collect()),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Null | Value::Pending(_) => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(i) => J::from(*i),
            Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
            Value::Str(s) => J::String(s.clone()),
            Value::List(l) => J::Array(l.iter().map(Value::to_json).collect()),
            Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
            Value::Pending(_) => true,
        }
    }

    /// String form of a scalar (`None` for null / containers).
    pub fn scalar_string(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}
