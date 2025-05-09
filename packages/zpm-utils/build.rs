fn main() {
    if let Some(env) = std::env::var_os("CARGO_CFG_TARGET_ARCH") {
        println!("cargo:rustc-env=TARGET_ARCH={}", env.to_string_lossy());
    }
    if let Some(env) = std::env::var_os("CARGO_CFG_TARGET_OS") {
        println!("cargo:rustc-env=TARGET_OS={}", env.to_string_lossy());
    }

    if let Some(env) = std::env::var_os("CARGO_CFG_TARGET_ENV") {
        println!("cargo:rustc-env=TARGET_ENV={}", env.to_string_lossy());
    }
}