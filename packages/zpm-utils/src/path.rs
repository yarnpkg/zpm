use std::{io::{Read, Write}, os::unix::ffi::OsStrExt, str::FromStr};

use bincode::{Decode, Encode};

use crate::{diff_data, impl_serialization_traits, path_resolve::resolve_path, FromFileString, OkMissing, PathError, PathIterator, ToFileString, ToHumanString};

#[derive(Debug)]
pub struct ExplicitPath {
    pub raw_path: RawPath,
}

impl FromStr for ExplicitPath {
    type Err = PathError;

    fn from_str(val: &str) -> Result<ExplicitPath, PathError> {
        if !val.contains('/') {
            return Err(PathError::InvalidExplicitPathParameter(val.to_string()));
        }

        let raw_path
            = RawPath::try_from(val)?;

        Ok(ExplicitPath {
            raw_path,
        })
    }
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

impl_serialization_traits!(RawPath);

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        Ok(std::env::var("HOME")
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

    pub fn iter_path(&self) -> PathIterator {
        PathIterator::new(self)
    }

    pub fn dirname<'a>(&'a self) -> Option<Path> {
        let mut slice_len = self.path.len();
        if self.path.ends_with('/') {
            if self.path.len() > 1 {
                slice_len -= 1;
            } else {
                return None;
            }
        }

        let slice = &self.path[..slice_len];
        if let Some(last_slash) = slice.rfind('/') {
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

    pub fn as_str<'a>(&'a self) -> &'a str {
        self.path.as_str()
    }

    pub fn to_path_buf(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.path)
    }

    pub fn is_root(&self) -> bool {
        self.path == "/"
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

    pub fn sys_set_current_dir(&self) -> Result<(), PathError> {
        std::env::set_current_dir(&self.path)?;
        Ok(())
    }

    pub fn fs_create_parent(&self) -> Result<&Self, PathError> {
        if let Some(parent) = self.dirname() {
            parent.fs_create_dir_all()?;
        }

        Ok(self)
    }

    pub fn fs_create_dir_all(&self) -> Result<&Self, PathError> {
        std::fs::create_dir_all(&self.path)?;
        Ok(self)
    }

    pub fn fs_create_dir(&self) -> Result<&Self, PathError> {
        std::fs::create_dir(&self.path)?;
        Ok(self)
    }

    pub fn fs_set_permissions(&self, permissions: std::fs::Permissions) -> Result<&Self, PathError> {
        std::fs::set_permissions(&self.path, permissions)?;
        Ok(self)
    }

    pub fn fs_metadata(&self) -> Result<std::fs::Metadata, PathError> {
        Ok(std::fs::metadata(&self.path)?)
    }

    pub fn fs_exists(&self) -> bool {
        self.fs_metadata().is_ok()
    }

    pub fn fs_is_file(&self) -> bool {
        self.fs_metadata().map(|m| m.is_file()).unwrap_or(false)
    }

    pub fn fs_is_dir(&self) -> bool {
        self.fs_metadata().map(|m| m.is_dir()).unwrap_or(false)
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

    pub fn fs_write_text<T: AsRef<str>>(&self, text: T) -> Result<&Self, PathError> {
        std::fs::write(self.to_path_buf(), text.as_ref())?;
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

    pub fn fs_expect<T: AsRef<[u8]>>(&self, expected_data: T, is_exec: bool) -> Result<&Self, PathError> {
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
                    .mode();

            let expected_mode
                = current_mode & 0o666 | if is_exec {0o111} else {0};

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
                    .mode();

            let expected_mode
                = current_mode & 0o666 | if is_exec {0o111} else {0};

            if current_mode != expected_mode {
                let expected_permissions
                    = std::fs::Permissions::from_mode(expected_mode);

                std::fs::set_permissions(&path_buf, expected_permissions)?;
            }
        }

        Ok(self)
    }

    pub fn fs_rename(&self, new_path: &Path) -> Result<&Self, PathError> {
        std::fs::rename(self.to_path_buf(), new_path.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_rm_file(&self) -> Result<&Self, PathError> {
        std::fs::remove_file(self.to_path_buf())?;
        Ok(self)
    }

    pub fn fs_rm(&self) -> Result<&Self, PathError> {
        match self.fs_is_dir() {
            true => std::fs::remove_dir_all(self.to_path_buf()),
            false => std::fs::remove_file(self.to_path_buf()),
        }?;

        Ok(self)
    }

    pub fn fs_symlink(&self, target: &Path) -> Result<&Self, PathError> {
        std::os::unix::fs::symlink(&target.path, &self.path)?;
        Ok(self)
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

    pub fn contains(&self, other: &Path) -> bool {
        other.as_str().starts_with(self.as_str()) || other == self
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

impl TryFrom<std::path::PathBuf> for Path {
    type Error = PathError;

    fn try_from(value: std::path::PathBuf) -> Result<Self, Self::Error> {
        Ok(Path::from_str(std::str::from_utf8(&value.as_os_str().as_bytes())?)?)
    }
}

impl TryFrom<&std::path::Path> for Path {
    type Error = PathError;

    fn try_from(value: &std::path::Path) -> Result<Self, Self::Error> {
        Ok(Path::from_str(std::str::from_utf8(value.as_os_str().as_bytes())?)?)
    }
}

impl FromFileString for Path {
    type Error = PathError;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(Path {path: resolve_path(s)})
    }
}

impl ToFileString for Path {
    fn to_file_string(&self) -> String {
        self.path.clone()
    }
}

impl ToHumanString for Path {
    fn to_print_string(&self) -> String {
        self.path.clone()
    }
}

impl_serialization_traits!(Path);
