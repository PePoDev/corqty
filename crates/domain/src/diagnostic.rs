use serde::{Deserialize, Serialize};

/// The most diagnostics one validation reports, so a hostile document cannot make the
/// response huge. Validations stop collecting soon after they pass it.
pub const MAX_DIAGNOSTICS: usize = 200;

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &str, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// The first `MAX_DIAGNOSTICS` of `diagnostics`, followed by one `too_many_diagnostics`
/// when there were more.
#[must_use]
pub fn capped(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    if diagnostics.len() > MAX_DIAGNOSTICS {
        diagnostics.truncate(MAX_DIAGNOSTICS);
        diagnostics.push(Diagnostic::new(
            "too_many_diagnostics",
            "",
            format!("Only the first {MAX_DIAGNOSTICS} problems are listed."),
        ));
    }
    diagnostics
}

/// One RFC 6901 reference token: `~` becomes `~0` and `/` becomes `~1`.
#[must_use]
pub fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
