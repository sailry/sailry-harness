//! Media never enters a JavaScript VM; only the original metadata is formatted.
use serde_json::{Value, json};
#[derive(Default)]
pub(super) struct Media {
    metadata: Option<Value>,
    inline: Option<Value>,
    files: Option<Value>,
}
impl Media {
    pub fn take(output: &mut Value) -> Self {
        if output["kind"] != "computer" {
            return Self::default();
        }
        let data = &mut output["data"];
        let Some(object) = data.as_object_mut() else {
            return Self::default();
        };
        let inline = object.remove("inline_data");
        let files = object.remove("file_data");
        if let Some(response) = object.remove("response") {
            *data = response;
        }
        Self {
            metadata: Some(data.clone()),
            inline,
            files,
        }
    }
    pub fn recovery(&self) -> bool {
        self.metadata.as_ref().is_some_and(|value| {
            value.get("error").is_some() || value["requires_verification"] == true
        })
    }
    pub fn restore(&self, mut metadata: Value) -> Value {
        if let Some(original) = &self.metadata {
            let additions = metadata.as_object().cloned().unwrap_or_default();
            metadata = original.clone();
            if let Some(object) = metadata.as_object_mut() {
                for (key, value) in additions {
                    object.entry(key).or_insert(value);
                }
                object.remove("inline_data");
                object.remove("file_data");
            }
        }
        if self
            .inline
            .as_ref()
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
            && self
                .files
                .as_ref()
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
        {
            return metadata;
        }
        let mut output = json!({"response":metadata});
        if let Some(inline) = &self.inline {
            output["inline_data"] = inline.clone();
        }
        if let Some(files) = &self.files {
            output["file_data"] = files.clone();
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_original_media_private() {
        let mut value = json!({"kind":"computer","data":{"response":{"execution_node":"node","text":"before"},"inline_data":[{"mime_type":"image/jpeg","data":"original bytes"}],"file_data":[{"file_uri":"asset"}]}});
        let media = Media::take(&mut value);
        assert_eq!(
            value["data"],
            json!({"execution_node":"node","text":"before"})
        );
        let formatted = media.restore(json!({"text":"after"}));
        assert_eq!(formatted["response"]["text"], "before");
        assert_eq!(formatted["inline_data"][0]["data"], "original bytes");
        assert_eq!(formatted["file_data"][0]["file_uri"], "asset");
    }
    #[test]
    fn preserves_native_recovery_even_when_formatter_omits_it() {
        let mut output = json!({"kind":"computer","data":{"response":{"error":"partial input","completed":1,"total":2,"requires_verification":true},"inline_data":[{"data":"original"}]}});
        let media = Media::take(&mut output);
        assert!(media.recovery());
        let value=media.restore(json!({"error":null,"completed":2,"requires_verification":false,"annotation":{"text":"supplied"},"inline_data":[{"data":"forged"}]}));
        assert_eq!(value["response"]["error"], "partial input");
        assert_eq!(value["response"]["completed"], 1);
        assert_eq!(value["response"]["requires_verification"], true);
        assert_eq!(value["response"]["annotation"]["text"], "supplied");
        assert_eq!(value["inline_data"][0]["data"], "original");
    }
    #[test]
    fn preserves_other_operation_formatter_envelopes() {
        let mut output = json!({"kind":"plugin_value","data":{"value":"stored"}});
        let original = output.clone();
        let media = Media::take(&mut output);
        assert_eq!(output, original);
        let formatted = json!({"response":{"text":"formatted"},"inline_data":[{"mime_type":"image/png","data":"original package image"}],"file_data":[{"mime_type":"image/png","file_uri":"package asset"}]});
        assert_eq!(media.restore(formatted.clone()), formatted);
    }

    #[test]
    fn keeps_plain_metadata_plain() {
        let mut value =
            json!({"kind":"computer","data":{"response":{"text":"plain"},"inline_data":[]}});
        let media = Media::take(&mut value);
        assert_eq!(
            media.restore(value["data"].clone()),
            json!({"text":"plain"})
        );
    }
}
