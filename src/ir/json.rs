//! The IR as JSON, with the version of its shape.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// The version of the IR's JSON shape, written as `"schema"` beside a
/// score's or music document's fields: 1 is lytk 0.6.0's, compact (values
/// at their default left out) and typed. JSON without one was written by
/// lytk 0.5 or earlier, in a shape this one doesn't read.
pub const SCHEMA: u64 = 1;

/// A score or music document as JSON, marked with [`SCHEMA`].
pub fn to_value<T: Serialize>(ir: &T) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(ir)?;
    if let Value::Object(map) = &mut value {
        map.insert("schema".to_string(), SCHEMA.into());
    }
    Ok(value)
}

/// A score or music document from its JSON, refused unless it is in
/// [`SCHEMA`].
pub fn from_value<T: DeserializeOwned>(mut value: Value) -> Result<T, String> {
    match value.as_object_mut().and_then(|map| map.remove("schema")) {
        Some(v) if v == SCHEMA => {}
        Some(v) => {
            return Err(format!(
                "IR JSON in schema {v}: this lytk reads schema {SCHEMA}"
            ))
        }
        None => {
            return Err(
                "IR JSON without a \"schema\": written by lytk 0.5 or earlier, whose shape \
                 this lytk doesn't read; read the source file again"
                    .to_string(),
            )
        }
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::score::Score;

    #[test]
    fn json_carries_its_schema_and_older_json_is_refused() {
        let value = to_value(&Score::new()).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(from_value::<Score>(value.clone()).unwrap(), Score::new());
        let mut old = value.clone();
        old.as_object_mut().unwrap().remove("schema");
        assert!(from_value::<Score>(old)
            .unwrap_err()
            .contains("0.5 or earlier"));
        let mut newer = value;
        newer["schema"] = (SCHEMA + 1).into();
        assert!(from_value::<Score>(newer).unwrap_err().contains("schema 2"));
    }
}
