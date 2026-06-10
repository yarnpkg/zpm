fn main() {
    println!("cargo::rustc-check-cfg=cfg(target_vendor, values(\"browserpod\"))");
}
