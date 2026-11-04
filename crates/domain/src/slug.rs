use uuid::Uuid;

/// Lowercase ASCII letter and digit runs of `name` joined by single hyphens (or
/// `fallback` when there are none), then `-` and the first 8 hex digits of `suffix_source`.
#[must_use]
pub fn slugify(name: &str, fallback: &str, suffix_source: Uuid) -> String {
    let base = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    let base = if base.is_empty() { fallback } else { &base };
    format!("{base}-{}", &suffix_source.simple().to_string()[..8])
}
