#[cfg(windows)]
mod service;
fn main() {
    #[cfg(windows)]
    if let Err(e) = service::dispatch() {
        eprintln!("SCX VirtualPad service: {e}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("SCX VirtualPad service requires Windows");
        std::process::exit(1);
    }
}
