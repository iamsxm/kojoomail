use std::path::Path;

fn main() {
    // 清单交给链接器嵌入，作用于所有目标。tauri-build 默认只把它链进 bin，
    // 而 cargo test --lib 的测试二进制同样需要 Common Controls v6：没有清单就会加载
    // 5.82 版 comctl32，找不到 TaskDialogIndirect 入口点，测试进程直接起不来。
    let attrs = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(attrs)).expect("tauri-build failed");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}