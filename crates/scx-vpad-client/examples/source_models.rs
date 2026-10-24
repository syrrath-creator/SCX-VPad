//! Read-only exact HID-model inventory for Source Guard package preparation.
fn main() -> std::io::Result<()> {
    #[cfg(windows)]
    for model in scx_vpad_client::source::discover_source_models()? {
        println!("{model}");
    }
    Ok(())
}
