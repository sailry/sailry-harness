//! File input conversion for the OpenAI Responses API.
//!
//! Converts ADK `Part::FileData` items to Responses API `InputItem` values,
//! supporting both inline base64 content and file ID references.
//!
//! # Supported MIME Types
//!
//! The Responses API accepts PDF, DOCX, PPTX, and various text/code file types.
//! See [`SUPPORTED_FILE_MIME_TYPES`] for the full list.
//!
//! # Example
//!
//! ```rust,ignore
//! use adk_model::openai::file_input::{convert_file_input, SUPPORTED_FILE_MIME_TYPES};
//!
//! // Inline base64 content
//! let item = convert_file_input("application/pdf", "data:application/pdf;base64,JVBERi0...")?;
//!
//! // File ID reference
//! let item = convert_file_input("text/plain", "file-abc123")?;
//! ```

use adk_core::{AdkError, ErrorCategory, ErrorComponent};
use async_openai::types::responses::{
    EasyInputContent, EasyInputMessage, InputContent, InputFileArgs, InputItem, Role,
};

/// Supported MIME types for file inputs in the OpenAI Responses API.
///
/// Includes document formats (PDF, DOCX, PPTX), plain text variants,
/// and common programming language MIME types.
pub const SUPPORTED_FILE_MIME_TYPES: &[&str] = &[
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/csv",
    "text/html",
    "text/markdown",
    "text/x-rust",
    "text/x-python",
    "text/javascript",
    "text/x-typescript",
    "text/x-java",
    "text/x-c",
    "text/x-cpp",
    "text/x-go",
];

/// Convert a file input to a Responses API `InputItem`.
///
/// Supports two modes based on the `file_uri` content:
/// - **Inline base64**: When `file_uri` starts with `"data:"`, the content is treated
///   as a base64-encoded data URI and sent inline via `file_data`.
/// - **File ID reference**: When `file_uri` starts with `"file-"`, it is treated as
///   an OpenAI Files API file ID reference.
/// - **URL reference**: Otherwise, the value is treated as a file URL.
///
/// # Errors
///
/// Returns `AdkError` with category `InvalidInput` and code
/// `model.openai_responses.unsupported_file_type` if `mime_type` is not in
/// [`SUPPORTED_FILE_MIME_TYPES`].
///
/// # Example
///
/// ```rust,ignore
/// use adk_model::openai::file_input::convert_file_input;
///
/// // File ID reference
/// let item = convert_file_input("application/pdf", "file-abc123")?;
///
/// // Inline base64 data URI
/// let item = convert_file_input("text/plain", "data:text/plain;base64,SGVsbG8=")?;
/// ```
pub fn convert_file_input(mime_type: &str, file_uri: &str) -> Result<InputItem, AdkError> {
    if !is_supported_mime_type(mime_type) {
        return Err(AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::InvalidInput,
            "model.openai_responses.unsupported_file_type",
            format!(
                "Unsupported file type '{mime_type}'. Supported types: {}",
                SUPPORTED_FILE_MIME_TYPES.join(", ")
            ),
        ));
    }

    let file_content = if file_uri.starts_with("file-") {
        // File ID reference mode
        InputFileArgs::default()
            .file_id(file_uri)
            .build()
            .expect("InputFileArgs build with file_id should not fail")
    } else if file_uri.starts_with("data:") {
        // Inline base64 data URI mode — pass the full data URI as file_data
        InputFileArgs::default()
            .file_data(file_uri)
            .build()
            .expect("InputFileArgs build with file_data should not fail")
    } else {
        // URL reference mode
        InputFileArgs::default()
            .file_url(file_uri)
            .build()
            .expect("InputFileArgs build with file_url should not fail")
    };

    let content = InputContent::InputFile(file_content);
    Ok(InputItem::EasyMessage(EasyInputMessage {
        role: Role::User,
        content: EasyInputContent::ContentList(vec![content]),
        ..Default::default()
    }))
}

/// Check whether a MIME type is in the supported list.
fn is_supported_mime_type(mime_type: &str) -> bool {
    SUPPORTED_FILE_MIME_TYPES.contains(&mime_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_pdf_with_file_id() {
        let result = convert_file_input("application/pdf", "file-abc123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_supported_text_with_data_uri() {
        let result = convert_file_input("text/plain", "data:text/plain;base64,SGVsbG8=");
        assert!(result.is_ok());
    }

    #[test]
    fn test_supported_rust_with_url() {
        let result = convert_file_input("text/x-rust", "https://example.com/main.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn test_unsupported_mime_type_returns_error() {
        let result = convert_file_input("application/zip", "file-abc123");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.category, ErrorCategory::InvalidInput);
        assert_eq!(err.code, "model.openai_responses.unsupported_file_type");
    }

    #[test]
    fn test_all_supported_types_accepted() {
        for mime_type in SUPPORTED_FILE_MIME_TYPES {
            let result = convert_file_input(mime_type, "file-test123");
            assert!(result.is_ok(), "Expected {mime_type} to be supported");
        }
    }

    #[test]
    fn test_file_id_mode_produces_file_id_content() {
        let item = convert_file_input("application/pdf", "file-xyz789").unwrap();
        let json = serde_json::to_value(&item).unwrap();
        let content = &json["content"][0];
        assert_eq!(content["type"], "input_file");
        assert_eq!(content["file_id"], "file-xyz789");
    }

    #[test]
    fn test_data_uri_mode_produces_file_data_content() {
        let data_uri = "data:text/plain;base64,SGVsbG8gV29ybGQ=";
        let item = convert_file_input("text/plain", data_uri).unwrap();
        let json = serde_json::to_value(&item).unwrap();
        let content = &json["content"][0];
        assert_eq!(content["type"], "input_file");
        assert_eq!(content["file_data"], data_uri);
    }

    #[test]
    fn test_url_mode_produces_file_url_content() {
        let url = "https://example.com/report.pdf";
        let item = convert_file_input("application/pdf", url).unwrap();
        let json = serde_json::to_value(&item).unwrap();
        let content = &json["content"][0];
        assert_eq!(content["type"], "input_file");
        assert_eq!(content["file_url"], url);
    }
}
