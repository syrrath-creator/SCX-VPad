//! Linked to the release no_std policy rlib by Test-Panic.ps1. No local panic
//! handler: panic! must use the exact handler shipped in the policy library.
#![no_std]
#![no_main]

extern crate scx_source_policy;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetErrorMode(mode: u32) -> u32;
    fn ExitProcess(code: u32) -> !;
}
#[link(name = "wer")]
unsafe extern "system" {
    fn WerSetFlags(flags: u32) -> i32;
}

#[no_mangle]
pub extern "C" fn mainCRTStartup() -> ! {
    // These are child-process settings only. Suppress Windows error dialogs and
    // heap capture without changing machine-wide WER or registry settings.
    unsafe {
        SetErrorMode(0x0001 | 0x0002); // FAILCRITICALERRORS | NOGPFAULTERRORBOX
        if WerSetFlags(1 | 32) < 0 {
            // NOHEAP | NO_UI; a setup failure must not count as fail-fast.
            ExitProcess(2);
        }
    }
    panic!("intentional Source Guard policy fail-fast verification");
}
