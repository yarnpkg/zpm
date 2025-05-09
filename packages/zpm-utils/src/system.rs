pub fn get_system_string() -> String {
    let mut parts = vec![];

    parts.push(env!("TARGET_ARCH"));
    parts.push(env!("TARGET_OS"));
    parts.push(env!("TARGET_ENV"));

    parts.join("-")
}
