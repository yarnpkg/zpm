fn main() {
    println!("cargo:rustc-check-cfg=cfg(sonic_rs)");

    if cfg!(not(target_pointer_width = "32")) {
        println!("cargo:rustc-cfg=sonic_rs");
    }
}
