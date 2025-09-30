pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace("'", "'\"'\"'"))
}
