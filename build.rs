use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=resources/app.res");
    println!("cargo:rerun-if-changed=resources/app.o");
    println!("cargo:rerun-if-changed=resources/app.manifest");
    println!("cargo:rerun-if-changed=pic/icon.png");
    println!("cargo:rerun-if-changed=pic/ico.png");
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let input = root.join("resources/app.res");
    assert!(
        input.is_file(),
        "先运行 python tools/make_resources.py 生成图标资源，或直接运行 build.bat"
    );
    let target = std::env::var("TARGET").unwrap();
    if target.ends_with("gnu") {
        let output = root.join("resources/app.o");
        assert!(output.is_file(), "缺少资源对象，请使用 build.bat 生成资源");
        println!("cargo:rustc-link-arg={}", output.display());
    } else {
        println!("cargo:rustc-link-arg={}", input.display());
    }
}
