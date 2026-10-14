use std::borrow::Cow;

/// Converts a native Windows path into its portable representation, where
/// drive letters are exposed as a top-level directory (`C:\foo` becomes
/// `/C:/foo`) and UNC shares live under `/unc` (`\\server\share` becomes
/// `/unc/server/share`). Portable paths are absolute iff they start with a
/// `/`, which lets `Path` reuse the same logic on every platform.
///
/// This function is always compiled so that it can be tested on every
/// platform, but it's only applied to paths on Windows.
///
/// ```
/// use zpm_utils::windows_to_portable;
///
/// assert_eq!(windows_to_portable(r"C:\foo\bar"), "/C:/foo/bar");
/// assert_eq!(windows_to_portable(r"c:\foo"), "/C:/foo");
/// assert_eq!(windows_to_portable(r"C:"), "/C:/");
/// assert_eq!(windows_to_portable(r"\\?\C:\foo"), "/C:/foo");
/// assert_eq!(windows_to_portable(r"\\server\share\foo"), "/unc/server/share/foo");
/// assert_eq!(windows_to_portable(r"\\?\UNC\server\share\foo"), "/unc/server/share/foo");
/// assert_eq!(windows_to_portable(r"foo\bar"), "foo/bar");
/// assert_eq!(windows_to_portable("/C:/foo"), "/C:/foo");
/// assert_eq!(windows_to_portable("/foo"), "/foo");
/// ```
pub fn windows_to_portable(path: &str) -> Cow<'_, str> {
    if !path.contains('\\') && !has_drive_prefix(path) && !has_unc_prefix(path) {
        return Cow::Borrowed(path);
    }

    let path
        = path.replace('\\', "/");

    let path = match path.strip_prefix("//?/").or_else(|| path.strip_prefix("//./")) {
        Some(verbatim) if verbatim.len() >= 4 && verbatim[..4].eq_ignore_ascii_case("UNC/") => {
            return Cow::Owned(format!("/unc/{}", &verbatim[4..]));
        },

        Some(verbatim) if has_drive_prefix(verbatim) => {
            verbatim.to_string()
        },

        _ => {
            path
        },
    };

    if has_unc_prefix(&path) {
        return Cow::Owned(format!("/unc/{}", &path[2..]));
    }

    if has_drive_prefix(&path) {
        let drive
            = path[..1].to_ascii_uppercase();

        let rest
            = path[2..].trim_start_matches('/');

        return Cow::Owned(format!("/{}:/{}", drive, rest));
    }

    Cow::Owned(path)
}

/// Converts a portable path back into a native Windows path, using the
/// given character as separator. Both `/` and `\` are accepted by the
/// Win32 APIs; the former is used when paths are displayed or passed to
/// subprocesses, the latter when they're given to the filesystem APIs.
///
/// ```
/// use zpm_utils::windows_from_portable;
///
/// assert_eq!(windows_from_portable("/C:/foo/bar", '\\'), r"C:\foo\bar");
/// assert_eq!(windows_from_portable("/C:/foo/bar", '/'), "C:/foo/bar");
/// assert_eq!(windows_from_portable("/C:", '\\'), r"C:\");
/// assert_eq!(windows_from_portable("/C:/", '/'), "C:/");
/// assert_eq!(windows_from_portable("/unc/server/share/foo", '\\'), r"\\server\share\foo");
/// assert_eq!(windows_from_portable("foo/bar", '\\'), r"foo\bar");
/// assert_eq!(windows_from_portable("foo/bar", '/'), "foo/bar");
/// ```
pub fn windows_from_portable(path: &str, separator: char) -> Cow<'_, str> {
    let native = if let Some(rest) = path.strip_prefix('/').filter(|rest| has_drive_prefix(rest)) {
        let (drive, rest)
            = rest.split_at(2);

        let rest = match rest {
            "" => "/",
            rest => rest,
        };

        Cow::Owned(format!("{}{}", drive, rest))
    } else if let Some(unc) = path.strip_prefix("/unc/") {
        Cow::Owned(format!("//{}", unc))
    } else {
        Cow::Borrowed(path)
    };

    if separator != '/' && native.contains('/') {
        Cow::Owned(native.replace('/', &separator.to_string()))
    } else {
        native
    }
}

/// Whether the path starts with `//server` (a UNC path once backslashes have
/// been converted; `///` isn't one, it's just a redundant root).
fn has_unc_prefix(path: &str) -> bool {
    let bytes
        = path.as_bytes();

    bytes.len() > 2
        && bytes[0] == b'/'
        && bytes[1] == b'/'
        && bytes[2] != b'/'
}

fn has_drive_prefix(path: &str) -> bool {
    let bytes
        = path.as_bytes();

    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Converts a native path into its portable representation (identity
/// outside of Windows).
pub fn to_portable_path(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        windows_to_portable(path)
    } else {
        Cow::Borrowed(path)
    }
}

/// Converts a portable path into a native path using forward slashes
/// (identity outside of Windows). This is the representation used when
/// paths are printed or passed to other processes.
pub fn to_native_path(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        windows_from_portable(path, '/')
    } else {
        Cow::Borrowed(path)
    }
}

/// The separator used by the `PATH` environment variable.
pub const PATH_LIST_SEPARATOR: char = if cfg!(windows) {';'} else {':'};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_drive_paths() {
        for native in [r"C:\", r"C:\foo", r"C:\foo\bar\", r"\\server\share\foo"] {
            let portable = windows_to_portable(native);
            assert_eq!(windows_from_portable(&portable, '\\'), native);
        }
    }

    #[test]
    fn ignores_redundant_root_slashes() {
        assert_eq!(windows_to_portable("///"), "///");
        assert_eq!(windows_to_portable(r"\\\foo"), "///foo");
    }

    #[test]
    fn ignores_drive_like_relative_segments() {
        assert_eq!(windows_to_portable("C:foo"), "C:foo");
        assert_eq!(windows_to_portable("foo:bar"), "foo:bar");
        assert_eq!(windows_from_portable("/foo:/bar", '/'), "/foo:/bar");
    }
}
