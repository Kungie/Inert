//! Keeping freed memory from counting against the browser.
//!
//! macOS's allocator keeps freed large blocks (page frames, decoded images,
//! texture uploads) in a per-process cache. They stay dirty and count in the
//! physical footprint: about 40 MB in a typical session. The cache can only
//! be turned off before the allocator starts, so on first launch the browser
//! sets `MallocLargeCache=0` and re-executes itself (same PID, a few ms).
//! The Servo process inherits the setting.

pub fn tune_allocator() {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        if std::env::var_os("MallocLargeCache").is_some() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("MallocLargeCache", "0").exec();
        // exec only returns on failure: carry on with the default allocator.
        tracing::debug!("re-exec with MallocLargeCache=0 failed: {err}");
    }
}
