pub fn resolve_path(input: &str) -> String {
    if input.is_empty() {
        return "".to_string();
    }

    let preserve_unc_prefix = input.starts_with("//") && !input.starts_with("///");
    let mut path = Vec::new();
    for component in input.split('/') {
        match component {
            ".." => {
                let last = path.last();
                if last == Some(&"") {
                    // Do nothing
                } else if last != None && last != Some(&"..") {
                    path.pop();
                } else {
                    path.push("..");
                }
            },
            "." => {},
            "" => {
                if path.is_empty() {
                    path.push("");
                }
            },
            _ => {
                path.push(component);
            },
        }
    }

    if input.ends_with("/") {
        path.push("");
    }

    if path == vec![""] {
        return "/".to_string();
    } else {
        let resolved = path.join("/");
        if preserve_unc_prefix && !resolved.starts_with("//") {
            format!("/{}", resolved)
        } else {
            resolved
        }
    }
}
