//! Operators may reduce request limits or raise them to the documented hard cap.
//! Malformed settings retain the safe default; out-of-range values are clamped.
pub fn bounded(name: &str, default: usize, maximum: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(1, maximum)
}

pub fn run_input_bytes() -> usize {
    corqty_domain::limits::Limits::deployment().run_input_bytes
}
pub fn webhook_bytes() -> usize {
    corqty_domain::limits::Limits::deployment().webhook_bytes
}
