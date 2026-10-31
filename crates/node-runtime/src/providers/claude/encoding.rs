use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;

pub(super) struct Body<'a>(pub &'a Value);

impl Serialize for Body<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(object) = self.0.as_object() else {
            return self.0.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(object.len()))?;
        let order = [
            "model",
            "messages",
            "system",
            "tools",
            "metadata",
            "max_tokens",
            "thinking",
            "context_management",
            "compaction",
            "output_config",
            "fallbacks",
            "anthropic_beta",
            "stream",
            "temperature",
            "top_p",
            "top_k",
            "stop_sequences",
            "speed",
            "tool_choice",
        ];
        for name in order {
            if let Some(value) = object.get(name) {
                if name == "system" {
                    map.serialize_entry(name, &Blocks(value))?;
                } else {
                    map.serialize_entry(name, value)?;
                }
            }
        }
        for (name, value) in object {
            if !order.contains(&name.as_str()) {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}

struct Blocks<'a>(&'a Value);

impl Serialize for Blocks<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(blocks) = self.0.as_array() else {
            return self.0.serialize(serializer);
        };
        let mut seq = serializer.serialize_seq(Some(blocks.len()))?;
        for block in blocks {
            seq.serialize_element(&Block(block))?;
        }
        seq.end()
    }
}

struct Block<'a>(&'a Value);

impl Serialize for Block<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(object) = self.0.as_object() else {
            return self.0.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for name in ["type", "text"] {
            if let Some(value) = object.get(name) {
                map.serialize_entry(name, value)?;
            }
        }
        for (name, value) in object {
            if !matches!(name.as_str(), "type" | "text") {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}
