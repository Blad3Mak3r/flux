pub fn attach_parent() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

    // A GUI-subsystem executable has no console of its own. Attaching is useful
    // only when Flux was invoked from an existing terminal, and harmless when it
    // was launched from Explorer.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
}
