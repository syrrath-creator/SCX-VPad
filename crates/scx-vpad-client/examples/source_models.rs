//! Read-only exact HID-model inventory for Source Guard package preparation.
fn main() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.is_empty() {
            for model in scx_vpad_client::source::discover_source_models()? {
                println!("{model}");
            }
        } else if args.len() == 1 && args[0] == "--status" {
            use scx_vpad_client::source::{discover_source_preparations, SourceGuardAvailability};
            for preparation in discover_source_preparations()? {
                let status = match preparation.availability {
                    SourceGuardAvailability::FilterInstalled => "filter-installed",
                    SourceGuardAvailability::NeedsInstallation => "needs-installation",
                    SourceGuardAvailability::ModelNotPackageable => "unsupported-model",
                };
                println!("{status}\t{}", preparation.model.as_deref().unwrap_or("-"));
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Usage: source_models [--status] (read-only; no installation or isolation)",
            ));
        }
    }
    Ok(())
}
