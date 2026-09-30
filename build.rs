#[cfg(windows)]
fn main() {
    use std::env;
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let resource =
        PathBuf::from(env::var_os("OUT_DIR").expect("missing output dir")).join("flux.res");
    let status = Command::new(resource_compiler())
        .current_dir(manifest_dir.join("assets"))
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg("flux.rc")
        .status()
        .expect("Unable to run the Windows resource compiler");
    assert!(status.success(), "Windows resource compiler failed");
    println!("cargo:rustc-link-arg={}", resource.display());
    println!("cargo:rerun-if-changed=assets/flux.ico");
    println!("cargo:rerun-if-changed=assets/flux.rc");
}

#[cfg(windows)]
fn resource_compiler() -> std::path::PathBuf {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    if let Some(path) = env::var_os("RC") {
        return PathBuf::from(path);
    }

    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)")
                .map(PathBuf::from)
                .map(|path| path.join("Windows Kits").join("10"))
        })
        .expect("Unable to locate the Windows SDK. Set RC to the path of rc.exe.");

    let bin = sdk.join("bin");
    let direct = bin.join("x64").join("rc.exe");
    if direct.is_file() {
        return direct;
    }

    let versioned = fs::read_dir(&bin)
        .expect("Unable to inspect the Windows SDK bin directory")
        .flatten()
        .map(|entry| entry.path().join("x64").join("rc.exe"))
        .find(|path| path.is_file());

    versioned.expect("Unable to locate rc.exe. Install the Windows SDK or set RC to its path.")
}

#[cfg(not(windows))]
fn main() {}
