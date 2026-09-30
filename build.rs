#[cfg(windows)]
fn main() {
    use std::env;
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let resource = PathBuf::from(env::var_os("OUT_DIR").expect("missing output dir")).join("flux.res");
    let status = Command::new("rc.exe")
        .current_dir(manifest_dir.join("assets"))
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg("flux.rc")
        .status()
        .expect("Unable to run rc.exe. Install the Windows SDK resource compiler.");
    assert!(status.success(), "Windows resource compiler failed");
    println!("cargo:rustc-link-arg={}", resource.display());
    println!("cargo:rerun-if-changed=assets/flux.ico");
    println!("cargo:rerun-if-changed=assets/flux.rc");
}

#[cfg(not(windows))]
fn main() {}
