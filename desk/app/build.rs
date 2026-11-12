fn main() {
    println!("cargo:rerun-if-env-changed=PRODUCTIVITYMON_VERSION");
    let version=std::env::var("PRODUCTIVITYMON_VERSION").unwrap_or_else(|_|std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=PRODUCTIVITYMON_VERSION={version}");
    println!("cargo:rerun-if-changed=src/desk/macos.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref()==Ok("macos") {
        if std::env::var("HOST").unwrap_or_default().contains("apple-darwin") {
            cc::Build::new().file("src/desk/macos.m").flag("-fobjc-arc").warnings(true).compile("productivitymon_platform");
        } else {
            println!("cargo:warning=Cross-checking Rust only; the Objective-C bridge must be compiled and linked on macOS.");
        }
        for framework in ["AppKit","Foundation","ApplicationServices","CoreGraphics"] {println!("cargo:rustc-link-lib=framework={framework}");}
    }

    println!("cargo:rerun-if-changed=assets/productivitymon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/productivitymon.ico")
            .set("ProductName", "ProductivityMon")
            .set("FileDescription", "ProductivityMon desktop companion")
            .set("InternalName", "ProductivityMon")
            .set("OriginalFilename", "desk-mons.exe")
            .compile()
            .expect("compile the ProductivityMon Windows icon resource");
    }
}
