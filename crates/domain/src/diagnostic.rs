use serde::{Deserialize, Serialize};

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}
