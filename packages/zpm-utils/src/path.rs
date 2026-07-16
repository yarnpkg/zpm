use std::{collections::BTreeMap, io::{Read, Write}, str::{FromStr, Split}, sync::atomic::{AtomicU64, Ordering}, time::SystemTime};

use rkyv::Archive;

use crate::{diff_data, impl_file_string_from_str, impl_file_string_serialization, path_resolve::resolve_path, DataType, FromFileString, IoResultExt, PathError, PathIterator, ToFileString, ToHumanString};

static ATOMIC_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

#[cfg(any(windows, test))]
fn to_portable_path(value: &str) -> String {
    let value = value.replace('\\', "/");

    if let Some(value) = value.strip_prefix("//?/UNC/") {
        format!("/unc/?/UNC/{}", value)
    } else if value.starts_with("//?/")
        && value.as_bytes().get(5) == Some(&b':')
        && value.as_bytes().get(4).map_or(false, u8::is_ascii_alphabetic)
    {
        value[3..].to_string()
    } else if value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes().first().map_or(false, u8::is_ascii_alphabetic)
    {
        format!("/{}", value)
    } else if let Some(value) = value.strip_prefix("//./") {
        format!("/unc/.dot/{}", value)
    } else if let Some(value) = value.strip_prefix("//") {
        format!("/unc/{}", value)
    } else {
        value
    }
}

#[cfg(any(windows, test))]
fn from_portable_path(value: &str) -> String {
    if value.as_bytes().get(2) == Some(&b':')
        && value.as_bytes().get(1).map_or(false, u8::is_ascii_alphabetic)
    {
        value[1..].replace('/', "\\")
    } else if let Some(value) = value.strip_prefix("/unc/.dot/") {
        format!("\\\\.\\{}", value.replace('/', "\\"))
    } else if let Some(value) = value.strip_prefix("/unc/?/UNC/") {
        format!("\\\\?\\UNC\\{}", value.replace('/', "\\"))
    } else if let Some(value) = value.strip_prefix("/unc/?/") {
        format!("\\\\?\\{}", value.replace('/', "\\"))
    } else if let Some(value) = value.strip_prefix("/unc/") {
        format!("\\\\{}", value.replace('/', "\\"))
    } else {
        value.to_string()
    }
}

#[cfg(windows)]
fn fs_set_mode_0600(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::{ffi::OsStr, iter, mem, os::windows::ffi::OsStrExt, ptr::{null, null_mut}};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS, GENERIC_ALL, HANDLE, LocalFree},
        Security::{
            Authorization::{
                EXPLICIT_ACCESS_W, GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
                SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
                TRUSTEE_W,
            },
            DACL_SECURITY_INFORMATION, GetTokenInformation, NO_INHERITANCE,
            PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct OwnedAcl(*mut windows_sys::Win32::Security::ACL);

    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }

    fn win32_error(code: u32) -> std::io::Error {
        std::io::Error::from_raw_os_error(code as i32)
    }

    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let token = OwnedHandle(token);

        let mut token_user_len = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut token_user_len);
        if token_user_len == 0 {
            return Err(std::io::Error::last_os_error());
        }

        let token_user_unit_size
            = mem::size_of::<usize>();
        let token_user_units
            = (token_user_len as usize + token_user_unit_size - 1) / token_user_unit_size;
        let mut token_user_data = vec![0usize; token_user_units];
        if GetTokenInformation(token.0, TokenUser, token_user_data.as_mut_ptr().cast(), token_user_len, &mut token_user_len) == 0 {
            return Err(std::io::Error::last_os_error());
        }

        let token_user
            = &*(token_user_data.as_ptr().cast::<TOKEN_USER>());
        let user_sid
            = token_user.User.Sid;

        let explicit_access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: GENERIC_ALL,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: user_sid.cast(),
            },
        };

        let mut acl = null_mut();
        let result
            = SetEntriesInAclW(1, &explicit_access, null(), &mut acl);
        if result != ERROR_SUCCESS {
            return Err(win32_error(result));
        }
        let acl = OwnedAcl(acl);

        let wide_path = OsStr::new(path)
            .encode_wide()
            .chain(iter::once(0))
            .collect::<Vec<_>>();

        let result = SetNamedSecurityInfoW(
            wide_path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl.0,
            null_mut(),
        );
        if result != ERROR_SUCCESS {
            return Err(win32_error(result));
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct SyncEntry {
    pub rel_path: Path,
    pub kind: SyncEntryKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncEntryKind {
    Folder,
    Symlink(Path),
    File(String, bool),
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum SyncError {
    #[error("Path error: {0}")]
    PathError(#[from] PathError),

    #[error("Forward path required: {}", .0.to_print_string())]
    ForwardPathRequired(Path),

    #[error("Conflicting path types: {}", .0.to_print_string())]
    ConflictingPathTypes(Path),
}

#[derive(Debug)]
pub struct ExplicitPath {
    pub raw_path: RawPath,
}

fn is_explicit_path_parameter(s: &str) -> bool {
    is_explicit_path_parameter_for_platform(s, cfg!(windows))
}

fn is_explicit_path_parameter_for_platform(s: &str, windows: bool) -> bool {
    s.contains('/') || (windows && s.contains('\\'))
}

impl FromFileString for ExplicitPath {
    type Error = PathError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        if !is_explicit_path_parameter(s) {
            return Err(PathError::InvalidExplicitPathParameter(s.to_string()));
        }

        let raw_path
            = RawPath::try_from(s)?;

        Ok(ExplicitPath { raw_path })
    }
}

impl_file_string_from_str!(ExplicitPath);

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct RawPath {
    pub raw: String,
    pub path: Path,
}

impl FromFileString for RawPath {
    type Error = PathError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        let path = Path::try_from(s)?;
        Ok(RawPath {raw: s.to_string(), path})
    }
}

impl ToFileString for RawPath {
    fn to_file_string(&self) -> String {
        self.raw.clone()
    }
}

impl ToHumanString for RawPath {
    fn to_print_string(&self) -> String {
        self.raw.clone()
    }
}

#[macro_export]
macro_rules! p {
    ($str:expr) => {
        $str.parse::<$crate::Path>().unwrap()
    };
}

impl_file_string_from_str!(RawPath);
impl_file_string_serialization!(RawPath);
#[derive(Clone, Debug, Archive, rkyv::Serialize, rkyv::Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[rkyv(compare(PartialEq, PartialOrd), derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct Path {
    path: String,
}

impl Path {
    pub fn temp_dir_pattern(str: &str) -> Result<Path, PathError> {
        let name = str.find("<>").map_or_else(|| str.to_string(), |index| {
            let before = &str[..index];
            let after = &str[index + 2..];

            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            format!("{}{:032x}{}", before, nonce, after)
        });

        let mut iteration: usize = 0;

        loop {
            let mut dir
                = Path::try_from(std::env::temp_dir())?;

            dir.join_str(format!("{}-{}", name, iteration));

            match dir.fs_create_dir() {
                Ok(_) => {
                    return Ok(dir);
                },

                Err(e) if e.io_kind() == Some(std::io::ErrorKind::AlreadyExists) => {
                    iteration += 1;
                },

                Err(e) => {
                    return Err(e);
                },
            }
        }
    }

    pub fn temp_root_dir() -> Result<Path, PathError> {
        Path::try_from(std::env::temp_dir())
    }

    pub fn temp_dir() -> Result<Path, PathError> {
        Self::temp_dir_pattern("temp-<>")
    }

    pub fn current_exe() -> Result<Path, PathError> {
        Ok(Path::try_from(std::env::current_exe()?)?)
    }

    pub fn current_dir() -> Result<Path, PathError> {
        Ok(Path::try_from(std::env::current_dir()?)?)
    }

    pub fn home_dir() -> Result<Option<Path>, PathError> {
        #[cfg(windows)]
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"));

        #[cfg(not(windows))]
        let home = std::env::var("HOME");

        Ok(home
            .ok()
            .map(|s| Path::try_from(s))
            .transpose()?)
    }

    /** @deprecated Prefer Path::empty() */
    pub fn new() -> Self {
        Path {path: "".to_string()}
    }

    pub fn empty() -> Self {
        Path {path: "".to_string()}
    }

    pub fn root() -> Self {
        Path {path: "/".to_string()}
    }

    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    pub fn segments(&self) -> Split<'_, char> {
        self.path.split('/')
    }

    /// ```
    /// use zpm_utils::p;
    ///
    /// let path = p!("/a/b/c");
    /// let mut iterator = path.iter_path();
    ///
    /// assert_eq!(iterator.next(), Some(p!("/")));
    /// assert_eq!(iterator.next(), Some(p!("/a")));
    /// assert_eq!(iterator.next(), Some(p!("/a/b")));
    /// assert_eq!(iterator.next(), Some(p!("/a/b/c")));
    /// assert_eq!(iterator.next(), None);
    /// ```
    ///
    /// The iterator can also be used in reverse:
    ///
    /// ```
    /// use zpm_utils::p;
    ///
    /// let path = p!("/a/b/c");
    /// let mut iterator = path.iter_path().rev();
    ///
    /// assert_eq!(iterator.next(), Some(p!("/a/b/c")));
    /// assert_eq!(iterator.next(), Some(p!("/a/b")));
    /// assert_eq!(iterator.next(), Some(p!("/a")));
    /// assert_eq!(iterator.next(), Some(p!("/")));
    /// assert_eq!(iterator.next(), None);
    /// ```
    ///
    /// The iterator will not include the trailing slash:
    ///
    /// ```
    /// use zpm_utils::p;
    ///
    /// let path = p!("/a/b/c/");
    /// let mut iterator = path.iter_path();
    ///
    /// assert_eq!(iterator.next(), Some(p!("/")));
    /// assert_eq!(iterator.next(), Some(p!("/a")));
    /// assert_eq!(iterator.next(), Some(p!("/a/b")));
    /// assert_eq!(iterator.next(), Some(p!("/a/b/c")));
    /// assert_eq!(iterator.next(), None);
    /// ```
    ///
    /// It also works with relative paths:
    ///
    /// ```
    /// use zpm_utils::p;
    ///
    /// let path = p!("a/b/c/");
    /// let mut iterator = path.iter_path();
    ///
    /// assert_eq!(iterator.next(), Some(p!("")));
    /// assert_eq!(iterator.next(), Some(p!("a")));
    /// assert_eq!(iterator.next(), Some(p!("a/b")));
    /// assert_eq!(iterator.next(), Some(p!("a/b/c")));
    /// assert_eq!(iterator.next(), None);
    /// ```
    ///
    /// And in reverse:
    ///
    /// ```
    /// use zpm_utils::p;
    ///
    /// let path = p!("a/b/c");
    /// let mut iterator = path.iter_path().rev();
    ///
    /// assert_eq!(iterator.next(), Some(p!("a/b/c")));
    /// assert_eq!(iterator.next(), Some(p!("a/b")));
    /// assert_eq!(iterator.next(), Some(p!("a")));
    /// assert_eq!(iterator.next(), Some(p!("")));
    /// assert_eq!(iterator.next(), None);
    /// ```
    pub fn iter_path(&self) -> PathIterator<'_> {
        PathIterator::new(self)
    }

    pub fn strip_first_segment(&self) -> Option<Path> {
        if !self.is_relative() {
            return None;
        }

        let Some((_, rest)) = self.path.split_once('/') else {
            return None;
        };

        Some(Path {
            path: rest.to_string(),
        })
    }

    pub fn strip_prefix(&self, prefix: &Path) -> Option<Path> {
        if prefix.is_empty() {
            return Some(self.clone());
        }

        if !self.path.starts_with(prefix.as_str()) {
            return None;
        }

        if self.path.len() == prefix.as_str().len() {
            return Some(Path::new());
        }

        if prefix.path.ends_with('/') {
            return Some(Path {path: self.path[prefix.as_str().len()..].to_string()})
        } else if self.path.chars().nth(prefix.as_str().len()) == Some('/') {
            return Some(Path {path: self.path[prefix.as_str().len() + 1..].to_string()})
        }

        None
    }

    pub fn dirname<'a>(&'a self) -> Option<Path> {
        if self.is_root() {
            return None;
        }

        let mut slice_len
            = self.path.len();

        if self.path.ends_with('/') {
            if self.path.len() > 1 {
                slice_len -= 1;
            } else {
                return None;
            }
        }

        let slice
            = &self.path[..slice_len];

        if let Some(last_slash) = slice.rfind('/') {
            if cfg!(windows) && last_slash == 3 && slice.as_bytes().get(2) == Some(&b':') {
                return Some(Path::from_str(&slice[..=last_slash]).unwrap());
            }
            if last_slash > 0 {
                return Some(Path::from_str(&slice[..last_slash]).unwrap());
            } else {
                return Some(Path::root());
            }
        }

        if slice_len > 0 {
            return Some(Path::new());
        }

        None
    }

    pub fn basename<'a>(&'a self) -> Option<&'a str> {
        let has_trailing_slash = self.path.ends_with('/');

        let initial_slice = if has_trailing_slash {
            &self.path[..self.path.len() - 1]
        } else {
            &self.path
        };

        let first_basename_char = initial_slice
            .rfind('/')
            .map(|i| i + 1)
            .unwrap_or(0);

        if first_basename_char < initial_slice.len() {
            Some(&initial_slice[first_basename_char..])
        } else {
            None
        }
    }

    pub fn without_trailing_separators(&self) -> Path {
        if self.is_root() {
            return self.clone();
        }

        let trimmed = self.path.trim_end_matches('/');

        if trimmed.len() == self.path.len() {
            self.clone()
        } else {
            Path {
                path: trimmed.to_string(),
            }
        }
    }

    pub fn extname<'a>(&'a self) -> Option<&'a str> {
        self.basename().and_then(|basename| {
            if let Some(mut last_dot) = basename.rfind('.') {
                if last_dot > 2 && &basename[last_dot - 2..] == ".d.ts" {
                    last_dot -= 2;
                }

                if last_dot != 0 {
                    Some(&basename[last_dot..])
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    pub fn components<'a>(&'a self) -> Split<'a, char> {
        self.path.split('/')
    }

    pub fn as_str<'a>(&'a self) -> &'a str {
        self.path.as_str()
    }

    pub fn to_path_buf(&self) -> std::path::PathBuf {
        #[cfg(windows)]
        return std::path::PathBuf::from(from_portable_path(&self.path));

        #[cfg(not(windows))]
        std::path::PathBuf::from(&self.path)
    }

    pub fn to_native_string(&self) -> String {
        self.to_path_buf().to_string_lossy().into_owned()
    }

    pub fn is_root(&self) -> bool {
        self.path == "/"
            || (cfg!(windows) && self.path.len() == 4 && self.path.as_bytes().get(2) == Some(&b':') && self.path.ends_with('/'))
            || (cfg!(windows) && self.path.strip_prefix("/unc/")
                .map(|path| path.trim_end_matches('/').split('/').count() == 2)
                .unwrap_or(false))
    }

    pub fn is_absolute(&self) -> bool {
        self.path.starts_with('/')
    }

    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    pub fn is_forward(&self) -> bool {
        self.is_relative() && !self.is_extern()
    }

    pub fn is_extern(&self) -> bool {
        self.path.starts_with("../") || self.path == ".."
    }

    pub fn to_home_string(&self) -> String {
        let home
            = Path::home_dir()
                .unwrap_or_default();

        if let Some(home) = home {
            if let Some(relative_path) = self.forward_relative_to(&home) {
                let pretty_path
                    = relative_path.to_file_string();

                return format!("~/{}", pretty_path);
            }
        }

        self.to_file_string()
    }

    pub fn sys_set_current_dir(&self) -> Result<(), PathError> {
        std::env::set_current_dir(self.to_path_buf())?;
        Ok(())
    }

    /// Sets cwd *and* `PWD` to this path's symlinked form. Without
    /// updating `PWD`, child processes (`pwd` etc.) report the
    /// canonical path instead of what the user typed.
    ///
    /// # Safety
    /// Must be called single-threaded during startup; `set_var` is
    /// unsafe in a multi-threaded process.
    pub unsafe fn sys_set_current_dir_with_pwd(&self) -> Result<(), PathError> {
        self.sys_set_current_dir()?;
        // SAFETY: caller contract guarantees single-threaded startup.
        unsafe { std::env::set_var("PWD", self.to_path_buf()); }
        Ok(())
    }

    pub fn fs_canonicalize(&self) -> Result<Path, PathError> {
        Ok(Path::try_from(std::fs::canonicalize(self.to_path_buf())?)?)
    }

    pub fn fs_create_parent(&self) -> Result<&Self, PathError> {
        if let Some(parent) = self.dirname() {
            parent.fs_create_dir_all()?;
        }

        Ok(self)
    }

    pub fn fs_create_dir_all(&self) -> Result<&Self, PathError> {
        std::fs::create_dir_all(self.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_create_dir(&self) -> Result<&Self, PathError> {
        std::fs::create_dir(self.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_set_permissions(&self, permissions: std::fs::Permissions) -> Result<&Self, PathError> {
        std::fs::set_permissions(self.to_path_buf(), permissions)?;
        Ok(self)
    }

    pub fn fs_set_mode(&self, mode: u32) -> Result<&Self, PathError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            self.fs_set_permissions(std::fs::Permissions::from_mode(mode))?;
        }

        #[cfg(windows)]
        {
            if mode == 0o600 {
                fs_set_mode_0600(&self.to_path_buf())?;
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = mode;
        }

        Ok(self)
    }

    pub fn fs_is_executable(&self) -> Result<bool, PathError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            return Ok(self.fs_metadata()?.permissions().mode() & 0o111 != 0);
        }

        #[cfg(windows)]
        {
            Ok(false)
        }

        #[cfg(not(any(unix, windows)))]
        {
            Ok(false)
        }
    }

    pub fn fs_symlink_metadata(&self) -> Result<std::fs::Metadata, PathError> {
        Ok(std::fs::symlink_metadata(self.to_path_buf())?)
    }

    pub fn fs_metadata(&self) -> Result<std::fs::Metadata, PathError> {
        Ok(std::fs::metadata(self.to_path_buf())?)
    }

    pub fn fs_exists(&self) -> bool {
        self.fs_metadata().is_ok()
    }

    pub fn fs_is_symlink(&self) -> bool {
        self.fs_symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false)
    }

    pub fn fs_is_file(&self) -> bool {
        self.fs_metadata().map(|m| m.is_file()).unwrap_or(false)
    }

    pub fn fs_is_dir(&self) -> bool {
        self.fs_metadata().map(|m| m.is_dir()).unwrap_or(false)
    }

    pub fn fs_is_real_dir(&self) -> bool {
        self.fs_symlink_metadata().map(|m| m.is_dir()).unwrap_or(false)
    }

    pub fn if_exists(&self) -> Option<Path> {
        if self.fs_exists() {
            Some(self.clone())
        } else {
            None
        }
    }

    pub fn if_file(&self) -> Option<Path> {
        if self.fs_is_file() {
            Some(self.clone())
        } else {
            None
        }
    }

    pub fn if_dir(&self) -> Option<Path> {
        if self.fs_is_dir() {
            Some(self.clone())
        } else {
            None
        }
    }

    pub fn fs_read(&self) -> Result<Vec<u8>, PathError> {
        Ok(std::fs::read(&self.to_path_buf())?)
    }

    pub fn fs_read_prealloc(&self) -> Result<Vec<u8>, PathError> {
        let metadata = self.fs_metadata()?;

        Ok(self.fs_read_with_size(metadata.len())?)
    }

    pub fn fs_read_with_size(&self, size: u64) -> Result<Vec<u8>, PathError> {
        let mut data = Vec::with_capacity(size as usize);

        let mut file = std::fs::File::open(&self.to_path_buf())?;
        file.read_to_end(&mut data)?;

        Ok(data)
    }

    pub fn fs_read_text(&self) -> Result<String, PathError> {
        Ok(std::fs::read_to_string(self.to_path_buf())?)
    }

    pub fn fs_read_text_prealloc(&self) -> Result<String, PathError> {
        let metadata = self.fs_metadata()?;

        Ok(self.fs_read_text_with_size(metadata.len())?)
    }

    pub fn fs_read_text_with_size(&self, size: u64) -> Result<String, PathError> {
        let mut data = String::with_capacity(size as usize);

        let mut file = std::fs::File::open(&self.to_path_buf())?;
        file.read_to_string(&mut data)?;

        Ok(data)
    }

    pub async fn fs_read_text_async(&self) -> Result<String, PathError> {
        Ok(tokio::fs::read_to_string(self.to_path_buf()).await?)
    }

    pub fn fs_read_dir(&self) -> Result<std::fs::ReadDir, PathError> {
        Ok(std::fs::read_dir(&self.to_path_buf())?)
    }

    pub fn fs_write<T: AsRef<[u8]>>(&self, data: T) -> Result<&Self, PathError> {
        std::fs::write(self.to_path_buf(), data)?;
        Ok(self)
    }

    pub fn fs_write_atomic<E, F>(&self, f: F) -> Result<&Self, PathError> where
        E: Into<PathError>,
        F: FnOnce(Path) -> Result<(), E>,
    {
        let parent
            = self.dirname().unwrap_or_default();

        let basename
            = self.basename().unwrap_or("tmp");

        let timestamp
            = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();

        let nonce
            = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);

        let tmp_name
            = format!(".{}-{}-{}-{}.tmp", basename, std::process::id(), timestamp, nonce);

        let tmp_path
            = parent.with_join_str(tmp_name);

        if let Err(error) = f(tmp_path.clone()).map_err(Into::into) {
            let _ = tmp_path.fs_rm_file().ok_missing();
            return Err(error);
        }

        match tmp_path.fs_rename(self) {
            Ok(_) => {
                Ok(self)
            },

            Err(error) => {
                let _ = tmp_path
                    .fs_rm_file()
                    .ok_missing();

                if error.io_kind() == Some(std::io::ErrorKind::AlreadyExists) {
                    return Err(PathError::AtomicRenameConflict {
                        from: tmp_path,
                        to: self.clone(),
                        inner: std::sync::Arc::new(error),
                    });
                }

                Err(error)
            },
        }
    }

    pub fn fs_write_text<T: AsRef<str>>(&self, text: T) -> Result<&Self, PathError> {
        std::fs::write(self.to_path_buf(), text.as_ref())?;
        Ok(self)
    }

    pub fn fs_set_modified(&self, modified: std::time::SystemTime) -> Result<&Self, PathError> {
        let file
            = std::fs::File::open(self.to_path_buf())?;

        file.set_modified(modified)?;

        Ok(self)
    }

    pub fn fs_append<T: AsRef<[u8]>>(&self, data: T) -> Result<&Self, PathError> {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.to_path_buf())?;

        file.write_all(data.as_ref())?;

        Ok(self)
    }

    pub fn fs_append_text<T: AsRef<str>>(&self, text: T) -> Result<&Self, PathError> {
        self.fs_append(text.as_ref())
    }

    pub fn fs_sync_dir(&self, mut entries: BTreeMap<Path, SyncEntryKind>) -> Result<&Self, SyncError> {
        let first_non_forward_path = entries.keys()
            .find(|path| !path.is_forward());

        if let Some(first_non_forward_path) = first_non_forward_path {
            return Err(SyncError::ForwardPathRequired(first_non_forward_path.clone()));
        }

        let entry_keys = entries.keys()
            .cloned()
            .collect::<Vec<_>>();

        for path in entry_keys {
            let dirname = path.dirname()
                .expect("Expected a parent directory for every path");

            for dir in dirname.iter_path() {
                let existing_entry
                    = entries.get(&dir);

                if let Some(existing_entry) = existing_entry {
                    if existing_entry != &SyncEntryKind::Folder {
                        return Err(SyncError::ConflictingPathTypes(dir.clone()));
                    } else {
                        continue;
                    }
                }

                entries.insert(dir, SyncEntryKind::Folder);
            }
        }

        let mut traverse_queue = vec![
            Path::new(),
        ];

        while let Some(path_rel) = traverse_queue.pop() {
            let path_abs
                = self.with_join(&path_rel);

            for entry in path_abs.fs_read_dir()? {
                let entry = entry
                    .map_err(PathError::from)?;

                let file_name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| PathError::InvalidUtf8Path)?;

                let entry_rel_path = path_rel
                    .with_join_str(&file_name);
                let entry_abs_path = path_abs
                    .with_join_str(&file_name);

                if entries.remove(&entry_rel_path).is_none() {
                    entry_abs_path.fs_rm()?;
                    continue;
                };

                if entry_abs_path.fs_is_real_dir() {
                    traverse_queue.push(entry_rel_path);
                }
            }
        }

        for (path, kind) in entries {
            let path_abs
                = self.with_join(&path);

            path_abs.fs_sync_file(kind)?;
        }

        Ok(self)
    }

    pub fn fs_sync_file(&self, kind: SyncEntryKind) -> Result<&Self, PathError> {
        match kind {
            SyncEntryKind::Symlink(target)
                => self.fs_symlink(&target),

            SyncEntryKind::File(data, is_exec)
                => self.fs_change(&data, is_exec),

            SyncEntryKind::Folder
                => self.fs_create_dir_all(),
        }
    }

    /// Like `fs_expect` but with a caller-supplied error on
    /// missing/mismatch. Compares content only (no permission bits).
    pub fn fs_expect_with<T, E, F>(&self, expected_data: T, build_err: F) -> Result<&Self, E>
    where
        T: AsRef<[u8]>,
        F: FnOnce() -> E,
        E: From<PathError>,
    {
        let current_content = self.fs_read()
            .ok_missing()?;

        let matches = current_content.as_ref()
            .map(|current| current.as_slice() == expected_data.as_ref())
            .unwrap_or(false);

        if !matches {
            return Err(build_err());
        }

        Ok(self)
    }

    pub fn fs_expect<T: AsRef<[u8]>>(&self, expected_data: T, is_exec: bool) -> Result<&Self, PathError> {
        #[cfg(windows)]
        let _ = is_exec;

        let current_content
            = self.fs_read()
                .ok_missing()?;

        let update_content = current_content.as_ref()
            .map(|current| current.ne(expected_data.as_ref()))
            .unwrap_or(true);

        if update_content {
            let diff = current_content.as_ref()
                .map(|current| diff_data(current, expected_data.as_ref()));

            return Err(PathError::ImmutableData {
                path: self.clone(),
                diff,
            });
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let current_mode
                = self.fs_metadata()?
                    .permissions()
                    .mode() & 0o666;

            let expected_mode
                = current_mode | (if is_exec {0o111} else {0});

            if current_mode != expected_mode {
                return Err(PathError::ImmutablePermissions {
                    path: self.clone(),
                    current_mode,
                    expected_mode,
                });
            }
        }

        Ok(self)
    }

    pub fn fs_change<T: AsRef<[u8]>>(&self, data: T, is_exec: bool) -> Result<&Self, PathError> {
        #[cfg(windows)]
        let _ = is_exec;

        let path_buf = self.to_path_buf();

        let update_content = self.fs_read()
            .ok_missing()
            .map(|current| current.map(|current| current.ne(data.as_ref())).unwrap_or(true))?;

        if update_content {
            std::fs::write(&path_buf, data)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let current_mode
                = self.fs_metadata()?
                    .permissions()
                    .mode() & 0o666;

            let expected_mode
                = current_mode | (if is_exec {0o111} else {0});

            if current_mode != expected_mode {
                let expected_permissions
                    = std::fs::Permissions::from_mode(expected_mode);

                std::fs::set_permissions(&path_buf, expected_permissions)?;
            }
        }

        Ok(self)
    }

    /**
     * Rename a file or directory to a new location.
     *
     * The source and destination must be on the same device or the function
     * will return an error. Use `fs_move` or `fs_concurrent_move` when this
     * behavior isn't desired.
     */
    pub fn fs_rename(&self, new_path: &Path) -> Result<&Self, PathError> {
        std::fs::rename(self.to_path_buf(), new_path.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_copy_file(&self, new_path: &Path) -> Result<&Self, PathError> {
        std::fs::copy(self.to_path_buf(), new_path.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_copy(&self, new_path: &Path) -> Result<&Self, PathError> {
        match self.fs_is_dir() {
            true => {
                new_path.fs_create_dir_all()?;
                for entry in self.fs_read_dir()? {
                    let entry = entry?;
                    let entry_path = Path::try_from(entry.path())?;

                    let destination_path = new_path.with_join(&Path::try_from(entry.file_name())?);

                    entry_path.fs_copy(&destination_path)?;
                }
            },
            false => {
                std::fs::copy(self.to_path_buf(), new_path.to_path_buf())?;
            },
        };

        Ok(self)
    }

    /**
     * Move a file or directory to a new location, copying it if the source and
     * destination are on different devices.
     *
     * The function will return an error if the destination already exists; prefer
     * using `fs_concurrent_move` when multiple processes may try to write into
     * the same location and you don't care which one succeeds first.
     */
    pub fn fs_move(&self, new_path: &Path) -> Result<&Self, PathError> {
        match std::fs::rename(self.to_path_buf(), new_path.to_path_buf()) {
            Ok(_) => Ok(self),
            Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
                self.fs_copy(new_path)?;
                self.fs_rm()
            },
            Err(err) => Err(err.into()),
        }
    }

    /**
     * Move a file or directory to a new location, copying it if the source and
     * destination are on different devices.
     *
     * This function will discard errors about the destination already existing,
     * making it safe to use when multiple processes could write into the same
     * location but you don't care which one succeeds first.
     */
    pub fn fs_concurrent_move(&self, new_path: &Path) -> Result<&Self, PathError> {
        self.fs_move(new_path)
            .discard_io_error(|kind| kind == std::io::ErrorKind::DirectoryNotEmpty || kind == std::io::ErrorKind::AlreadyExists)
            .map(|_| self)
    }

    pub fn fs_rm_file(&self) -> Result<&Self, PathError> {
        std::fs::remove_file(self.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_rm(&self) -> Result<&Self, PathError> {
        match self.fs_is_real_dir() {
            true => std::fs::remove_dir_all(self.to_path_buf()),
            false => std::fs::remove_file(self.to_path_buf()),
        }?;

        Ok(self)
    }

    pub fn fs_symlink(&self, target: &Path) -> Result<&Self, PathError> {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target.path, &self.path)?;

        #[cfg(windows)]
        {
            let resolved_target = if target.is_absolute() {
                target.clone()
            } else {
                self.dirname().unwrap_or_default().with_join(target)
            };

            if resolved_target.fs_is_dir() {
                std::os::windows::fs::symlink_dir(target.to_path_buf(), self.to_path_buf())?;
            } else {
                std::os::windows::fs::symlink_file(target.to_path_buf(), self.to_path_buf())?;
            }
        }

        Ok(self)
    }

    pub fn fs_junction(&self, target: &Path) -> Result<&Self, PathError> {
        #[cfg(windows)]
        {
            let resolved_target = if target.is_absolute() {
                target.clone()
            } else {
                self.dirname().unwrap_or_default().with_join(target)
            };
            if resolved_target.fs_is_dir() {
                junction::create(resolved_target.to_path_buf(), self.to_path_buf())?;
            } else {
                self.fs_symlink(target)?;
            }
        }

        #[cfg(not(windows))]
        self.fs_symlink(target)?;

        Ok(self)
    }

    pub fn fs_read_link(&self) -> Result<Path, PathError> {
        Ok(Path::try_from(std::fs::read_link(&self.to_path_buf())?)?)
    }

    pub fn without_ext(&self) -> Path {
        self.with_ext("")
    }

    pub fn with_ext(&self, ext: &str) -> Path {
        let mut copy = self.clone();
        copy.set_ext(ext);
        copy
    }

    pub fn set_ext(&mut self, ext: &str) -> &mut Self {
        let has_trailing_slash = self.path.ends_with('/');

        let initial_slice = if has_trailing_slash {
            &self.path[..self.path.len() - 1]
        } else {
            &self.path
        };

        let first_basename_char = initial_slice
            .rfind('/')
            .map(|i| i + 1)
            .unwrap_or(0);

        let mut ext_char = self.path[first_basename_char..]
            .rfind('.')
            .map(|i| i + first_basename_char)
            .unwrap_or(initial_slice.len());

        if ext_char == first_basename_char {
            ext_char = self.path.len();
        }

        if ext_char > 2 && &self.path[ext_char - 2..] == ".d.ts" {
            ext_char -= 2;
        }

        let mut copy = self.path[..ext_char].to_string();
        copy.push_str(ext);

        if has_trailing_slash {
            copy.push('/');
        }

        self.path = copy;
        self
    }

    pub fn with_join(&self, other: &Path) -> Path {
        let mut copy = self.clone();
        copy.join(other);
        copy
    }

    pub fn with_join_str<T>(&self, other: T) -> Path
    where
        T: AsRef<str>,
    {
        let mut copy = self.clone();
        copy.join_str(other);
        copy
    }

    pub fn join(&mut self, other: &Path) -> &mut Self {
        if !other.path.is_empty() {
            if self.path.is_empty() || other.is_absolute() {
                self.path = other.path.clone();
            } else {
                if !self.path.ends_with('/') {
                    self.path.push('/');
                }
                self.path.push_str(&other.path);
                self.normalize();
            }
        }

        self
    }

    pub fn join_str<T>(&mut self, other: T) -> &mut Self where T: AsRef<str> {
        self.join(&Path::from_str(other.as_ref()).unwrap())
    }

    /// ```
    /// use zpm_utils::p;

    /// assert_eq!(p!("/a/b").contains(&p!("/a/b")), true);
    /// assert_eq!(p!("/a/b").contains(&p!("/a/b/")), true);
    /// assert_eq!(p!("/a/b").contains(&p!("/a/b/c")), true);

    /// assert_eq!(p!("a/b").contains(&p!("a/b")), true);
    /// assert_eq!(p!("a/b").contains(&p!("a/b/")), true);
    /// assert_eq!(p!("a/b").contains(&p!("a/b/c")), true);

    /// assert_eq!(p!("/a/b/").contains(&p!("a/b")), false);
    /// assert_eq!(p!("/a/b").contains(&p!("a/bc")), false);

    /// assert_eq!(p!("a/b/").contains(&p!("a/b")), false);
    /// assert_eq!(p!("a/b").contains(&p!("a/bc")), false);
    /// ```
    pub fn contains(&self, other: &Path) -> bool {
        let self_as_str
            = self.as_str();
        let other_as_str
            = other.as_str();

        if !other_as_str.starts_with(self_as_str) {
            return false;
        }

        if other_as_str.len() == self_as_str.len() {
            return true;
        }

        let self_as_bytes
            = self_as_str.as_bytes();
        let other_as_bytes
            = other_as_str.as_bytes();

        if other_as_bytes[self_as_bytes.len()] != b'/' {
            return false;
        }

        true
    }

    pub fn forward_relative_to(&self, other: &Path) -> Option<Path> {
        if other.contains(self) {
            Some(self.relative_to(other))
        } else {
            None
        }
    }

    pub fn relative_to(&self, other: &Path) -> Path {
        assert!(self.is_absolute());
        assert!(other.is_absolute());

        let ends_with_slash = self.path.ends_with('/');

        let self_components: Vec<&str> = self.path.trim_end_matches('/').split('/').collect();
        let other_components: Vec<&str> = other.path.trim_end_matches('/').split('/').collect();

        let common_prefix_length = self_components.iter()
            .zip(other_components.iter())
            .take_while(|(a, b)| a == b)
            .count();

        let mut relative_path = vec![];

        for _ in common_prefix_length..other_components.len() {
            if other_components[common_prefix_length..].len() > 0 {
                relative_path.push("..");
            }
        }

        for component in self_components[common_prefix_length..].iter() {
            relative_path.push(*component);
        }

        if ends_with_slash {
            relative_path.push("");
        }

        if relative_path.is_empty() {
            Path::new()
        } else {
            Path::from_str(&relative_path.join("/")).unwrap()
        }
    }

    fn normalize(&mut self) {
        self.path = resolve_path(&self.path);
    }
}

impl Default for Path {
    fn default() -> Self {
        Path::new()
    }
}

impl TryFrom<std::ffi::OsString> for Path {
    type Error = PathError;

    fn try_from(value: std::ffi::OsString) -> Result<Self, Self::Error> {
        Path::try_from(value.as_os_str())
    }
}

impl TryFrom<&std::ffi::OsStr> for Path {
    type Error = PathError;

    fn try_from(value: &std::ffi::OsStr) -> Result<Self, Self::Error> {
        let value
            = value.to_str()
                .ok_or(PathError::InvalidUtf8Path)?;

        #[cfg(windows)]
        let value = to_portable_path(value);

        Ok(Path::from_str(&value)?)
    }
}

impl TryFrom<std::path::PathBuf> for Path {
    type Error = PathError;

    fn try_from(value: std::path::PathBuf) -> Result<Self, Self::Error> {
        Path::try_from(value.as_os_str())
    }
}

impl TryFrom<&std::path::Path> for Path {
    type Error = PathError;

    fn try_from(value: &std::path::Path) -> Result<Self, Self::Error> {
        Path::try_from(value.as_os_str())
    }
}

impl FromFileString for Path {
    type Error = PathError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        #[cfg(windows)]
        let s = to_portable_path(s);

        Ok(Path {path: resolve_path(&s)})
    }
}

impl ToFileString for Path {
    fn to_file_string(&self) -> String {
        self.path.clone()
    }
}

impl ToHumanString for Path {
    fn to_print_string(&self) -> String {
        DataType::Path.colorize(&self.to_home_string())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Path, from_portable_path, is_explicit_path_parameter_for_platform, to_portable_path};

    #[test]
    fn converts_windows_paths() {
        assert_eq!(to_portable_path(r"C:\work\project"), "/C:/work/project");
        assert_eq!(to_portable_path(r"\\server\share\project"), "/unc/server/share/project");
        assert_eq!(to_portable_path(r"\\.\pipe\yarn"), "/unc/.dot/pipe/yarn");
        assert_eq!(to_portable_path(r"\\?\C:\work\project"), "/C:/work/project");
        assert_eq!(to_portable_path(r"\\?\UNC\server\share\project"), "/unc/?/UNC/server/share/project");

        assert_eq!(from_portable_path("/C:/work/project"), r"C:\work\project");
        assert_eq!(from_portable_path("/unc/server/share/project"), r"\\server\share\project");
        assert_eq!(from_portable_path("/unc/.dot/pipe/yarn"), r"\\.\pipe\yarn");
        assert_eq!(from_portable_path("/unc/?/C:/work/project"), r"\\?\C:\work\project");
        assert_eq!(from_portable_path("/unc/?/UNC/server/share/project"), r"\\?\UNC\server\share\project");
    }

    #[test]
    fn classifies_backslash_paths_as_explicit_only_on_windows() {
        assert!(is_explicit_path_parameter_for_platform("./workspace", false));
        assert!(is_explicit_path_parameter_for_platform(r"D:\a\zpm\zpm\tests\acceptance-tests", true));
        assert!(!is_explicit_path_parameter_for_platform(r"foo\bar", false));
    }

    #[test]
    fn normalizes_repeated_trailing_separators() {
        assert_eq!(Path::from_str("foo//").unwrap().as_str(), "foo/");
        assert_eq!(Path::from_str("/foo///").unwrap().as_str(), "/foo/");
        assert_eq!(Path::from_str("///").unwrap().as_str(), "/");
    }

    #[test]
    fn removes_trailing_separators() {
        assert_eq!(Path::from_str("foo/").unwrap().without_trailing_separators().as_str(), "foo");
        assert_eq!(Path::from_str("/foo/").unwrap().without_trailing_separators().as_str(), "/foo");
        assert_eq!(Path::from_str("/").unwrap().without_trailing_separators().as_str(), "/");
        assert_eq!(Path::new().without_trailing_separators().as_str(), "");
    }
}

impl_file_string_from_str!(Path);
impl_file_string_serialization!(Path);
