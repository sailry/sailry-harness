//! Finite table recipes format original scalar values without a JavaScript round trip.
use super::Reference;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub columns: Reference,
    pub rows: Reference,
    pub truncated: Option<Reference>,
    pub cell: Cell,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    /// JSON Pointer within each cell, or the empty pointer for scalar cells.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub missing: String,
    pub bytes: Option<Bytes>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Bytes {
    Hex,
}

impl Table {
    pub(super) fn valid(&self) -> bool {
        self.columns.valid(false)
            && self.rows.valid(false)
            && self
                .truncated
                .as_ref()
                .is_none_or(|value| value.valid(false))
            && (self.cell.value.is_empty() || super::super::valid_pointer(&self.cell.value))
            && self.cell.missing.len() <= 1024
    }

    pub fn render(&self, arguments: &Value, result: &Value) -> Option<crate::tool::Table> {
        let columns = self
            .columns
            .read(arguments, result)?
            .as_array()?
            .iter()
            .map(|column| column.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        let rows = self
            .rows
            .read(arguments, result)?
            .as_array()?
            .iter()
            .map(|row| {
                row.as_array()?
                    .iter()
                    .map(|cell| self.cell.render(cell))
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        let table = crate::tool::Table {
            columns,
            rows,
            truncated: self
                .truncated
                .as_ref()
                .and_then(|value| value.read(arguments, result))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        crate::tool::Content {
            version: 1,
            blocks: vec![crate::tool::Block::Table(table.clone())],
        }
        .valid()
        .then_some(table)
    }
}

impl Cell {
    fn render(&self, cell: &Value) -> Option<String> {
        match cell.pointer(&self.value) {
            None | Some(Value::Null) => Some(self.missing.clone()),
            Some(value @ (Value::Bool(_) | Value::Number(_) | Value::String(_))) => {
                super::literal(value)
            }
            Some(Value::Array(bytes)) if self.bytes == Some(Bytes::Hex) => {
                let bytes = bytes
                    .iter()
                    .map(|byte| u8::try_from(byte.as_u64()?).ok())
                    .collect::<Option<Vec<_>>>()?;
                Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Source;
    use super::*;
    use serde_json::json;

    fn projection() -> Table {
        let reference = |path: &str| Reference {
            source: Source::Result,
            path: path.into(),
        };
        Table {
            columns: reference("/columns"),
            rows: reference("/rows"),
            truncated: Some(reference("/truncated")),
            cell: Cell {
                value: "/value".into(),
                missing: "NULL".into(),
                bytes: Some(Bytes::Hex),
            },
        }
    }

    #[test]
    fn preserves_integer_decimal_unicode_and_binary_cells() {
        let result = json!({"columns":["large","small","decimal","text","null","bytes"],"rows":[[
            {"value":i64::MAX},{"value":i64::MIN},{"value":"1234567890.12345678901234567890"},{"value":"资料 🙂"},{},{"value":[0,127,255]}
        ]],"truncated":true});
        let table = projection().render(&Value::Null, &result).unwrap();
        assert_eq!(
            table.rows[0],
            [
                "9223372036854775807",
                "-9223372036854775808",
                "1234567890.12345678901234567890",
                "资料 🙂",
                "NULL",
                "007fff"
            ]
        );
        assert!(table.truncated);
        assert_eq!(result["rows"][0][0]["value"].as_i64(), Some(i64::MAX));
    }

    #[test]
    fn malformed_cells_preserve_raw_values() {
        let result = json!({"columns":["binary"],"rows":[[{"value":[256]}]]});
        assert!(projection().render(&Value::Null, &result).is_none());
        let mut projection = projection();
        projection.cell.value = "/invalid~2".into();
        assert!(!projection.valid());
    }
}
