fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    // Embed PerMonitorV2 + asInvoker manifest when GNU windres is available
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let res_obj = std::path::Path::new(&out_dir).join("app_manifest.o");
    let rc_path = std::path::Path::new(&out_dir).join("app.rc");
    let manifest_abs = std::env::current_dir()
        .unwrap()
        .join("app.manifest")
        .to_string_lossy()
        .replace('\\', "/");
    let _ = std::fs::write(&rc_path, format!("1 24 \"{manifest_abs}\"\n"));

    let candidates = ["windres", "x86_64-w64-mingw32-windres"];
    for tool in candidates {
        if let Ok(status) = std::process::Command::new(tool)
            .args(["-i", rc_path.to_str().unwrap(), "-o", res_obj.to_str().unwrap()])
            .status()
        {
            if status.success() {
                println!("cargo:rustc-link-arg={}", res_obj.display());
                return;
            }
        }
    }
}
