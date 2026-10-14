use std::env;

/// Windows only reserves 1 MiB of stack for the main thread (versus 8 MiB on
/// most Linux systems), which isn't enough for the deeply nested futures we
/// poll from it.
fn reserve_windows_main_stack() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo::rustc-link-arg-bins=/STACK:8388608"),
        Ok("gnu") => println!("cargo::rustc-link-arg-bins=-Wl,--stack,8388608"),
        _ => {},
    }
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(target_vendor, values(\"browserpod\"))");

    reserve_windows_main_stack();
}
