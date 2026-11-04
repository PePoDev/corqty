//! Rebuild the committed precompiled sample without a cross-compilation toolchain.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/example-wasm");
    let bytes = wat::parse_file(root.join("hello.wat"))?;
    std::fs::write(root.join("hello.wasm"), bytes)?;
    Ok(())
}
