fn main() {
    println!("cargo:rustc-check-cfg=cfg(sonic_rs)");

    let target_pointer_width
        = std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH")
            .expect("CARGO_CFG_TARGET_POINTER_WIDTH not set");

    if target_pointer_width != "32" {
        println!("cargo:rustc-cfg=sonic_rs");
    }
}
