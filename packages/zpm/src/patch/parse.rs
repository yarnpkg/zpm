use std::{borrow::Cow, str::FromStr, sync::LazyLock};

use zpm_utils::Path;
use regex::Regex;
use zpm_utils::FromFileString;

use crate::error::Error;

#[cfg(test)]
#[path = "./parse.test.rs"]
mod parse_tests;

static HEADER_REGEXP: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^@@ -(\d+)(,(\d+))? \+(\d+)(,(\d+))? @@.*").unwrap()
});

static DIFF_LINE_REGEXP: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^diff --git a/(.*?) b/(.*?)\s*$").unwrap()
});

static INDEX_REGEXP: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(\w+)\.\.(\w+)").unwrap()
});

/// Git quotes the paths that contain double quotes, backslashes, control
/// characters, or (unless `core.quotePath` is disabled) non-ASCII bytes, as
/// C-style strings: `"a/caf\303\251.js"`. Returns the unquoted path, and
/// what follows the closing quote.
fn parse_quoted_path(src: &str) -> Option<(String, &str)> {
    let mut bytes
        = src.strip_prefix('"')?.bytes().enumerate();

    let mut path
        = Vec::new();

    while let Some((index, byte)) = bytes.next() {
        match byte {
            b'"' => {
                let rest
                    = &src[index + 2..];

                return String::from_utf8(path).ok().map(|path| (path, rest));
            },

            b'\\' => {
                let (_, escape)
                    = bytes.next()?;

                path.push(match escape {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    b'"' => b'"',
                    b'\\' => b'\\',

                    // Git always writes three octal digits, each byte of a
                    // multi-byte character being escaped separately
                    b'0'..=b'3' => {
                        let (_, second) = bytes.next()?;
                        let (_, third) = bytes.next()?;

                        if !matches!(second, b'0'..=b'7') || !matches!(third, b'0'..=b'7') {
                            return None;
                        }

                        ((escape - b'0') << 6) | ((second - b'0') << 3) | (third - b'0')
                    },

                    _ => {
                        return None;
                    },
                });
            },

            _ => {
                path.push(byte);
            },
        }
    }

    None
}

/// Parses the path of a `rename from`/`rename to` line, which git quotes when
/// needed (cf `parse_quoted_path`).
fn parse_header_path(src: &str) -> Option<Cow<'_, str>> {
    if !src.starts_with('"') {
        return Some(Cow::Borrowed(src));
    }

    match parse_quoted_path(src)? {
        (path, "") => Some(Cow::Owned(path)),
        _ => None,
    }
}

/// Parses the path of a `---`/`+++` line, without its `a/`/`b/` prefix. Git
/// follows unquoted paths that contain spaces with a tab (other tools follow
/// the path with a tab and a timestamp), which isn't part of the path.
fn parse_file_line_path(src: &str) -> Option<Cow<'_, str>> {
    let path = if src.starts_with('"') {
        let (path, rest)
            = parse_quoted_path(src)?;

        if !rest.is_empty() && !rest.starts_with('\t') {
            return None;
        }

        Cow::Owned(path)
    } else {
        Cow::Borrowed(src.split('\t').next().unwrap_or_default())
    };

    // Same as Berry, we don't check what the prefix is
    match path {
        Cow::Borrowed(path) => path.get(2..).map(Cow::Borrowed),
        Cow::Owned(path) => path.get(2..).map(|path| Cow::Owned(path.to_string())),
    }
}

/// Parses the two paths of a `diff --git a/<from> b/<to>` line, without their
/// prefixes; each of them may be quoted (cf `parse_quoted_path`).
fn parse_diff_line_paths(line: &str) -> Option<(Cow<'_, str>, Cow<'_, str>)> {
    let rest
        = line.strip_prefix("diff --git ")?;

    if rest.starts_with('"') {
        let (from, rest)
            = parse_quoted_path(rest)?;
        let to
            = parse_header_path(rest.strip_prefix(' ')?.trim_end())?;

        return Some((Cow::Owned(from.strip_prefix("a/")?.to_string()), Cow::Owned(to.strip_prefix("b/")?.to_string())));
    }

    // Only the destination path is quoted (renames); we look for the opening
    // quote that makes it a valid quoted path running until the end of the line
    if rest.trim_end().ends_with('"') {
        for (index, _) in rest.match_indices(" \"b/") {
            if let Some(to) = parse_header_path(rest[index + 1..].trim_end()) {
                return Some((Cow::Borrowed(rest[..index].strip_prefix("a/")?), Cow::Owned(to.strip_prefix("b/")?.to_string())));
            }
        }
    }

    let m
        = DIFF_LINE_REGEXP.captures(line)?;

    Some((Cow::Borrowed(m.get(1)?.as_str()), Cow::Borrowed(m.get(2)?.as_str())))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkHeader {
    pub original: Range,
    pub modified: Range,
}

impl FromFileString for HunkHeader {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let m = HEADER_REGEXP.captures(src)
            .ok_or_else(|| Error::InvalidHunkHeader(src.to_string()))?;

        let original_start = m.get(1).unwrap().as_str().parse::<usize>()?;
        let original_length = m.get(3).map(|m| m.as_str().parse::<usize>()).unwrap_or(Ok(1))?;

        let patched_start = m.get(4).unwrap().as_str().parse::<usize>()?;
        let patched_length = m.get(6).map(|m| m.as_str().parse::<usize>()).unwrap_or(Ok(1))?;

        Ok(HunkHeader {
            original: Range {
                start: original_start,
                length: original_length,
            },
            modified: Range {
                start: patched_start,
                length: patched_length,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchMutationPartKind {
    Header,
    Pragma,
    Context,
    Insertion,
    Deletion,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchMutationPart {
    pub kind: PatchMutationPartKind,
    pub lines: Vec<String>,
    pub no_newline_at_eof: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub header: HunkHeader,
    pub parts: Vec<PatchMutationPart>,
}

impl Hunk {
    fn verify_integrity(&self) -> bool {
        let mut original_length = 0;
        let mut patched_length = 0;

        for part in &self.parts {
            match part.kind {
                PatchMutationPartKind::Context => {
                    original_length += part.lines.len();
                    patched_length += part.lines.len();
                },

                PatchMutationPartKind::Insertion => {
                    patched_length += part.lines.len();
                },

                PatchMutationPartKind::Deletion => {
                    original_length += part.lines.len();
                },

                _ => {},
            }
        }

        
        original_length == self.header.original.length && patched_length == self.header.modified.length
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchFilePart {
    FilePatch {
        semver_exclusivity: Option<zpm_semver::Range>,
        path: Path,
        hunks: Vec<Hunk>,
        before_hash: Option<String>,
        after_hash: Option<String>,
    },

    FileDeletion {
        semver_exclusivity: Option<zpm_semver::Range>,
        path: Path,
        mode: u32,
        hunk: Option<Hunk>,
        hash: Option<String>,
    },

    FileCreation {
        semver_exclusivity: Option<zpm_semver::Range>,
        path: Path,
        mode: u32,
        hunk: Option<Hunk>,
        hash: Option<String>,
    },

    FileRename {
        semver_exclusivity: Option<zpm_semver::Range>,
        from: Path,
        to: Path,
    },

    FileModeChange {
        semver_exclusivity: Option<zpm_semver::Range>,
        path: Path,
        old_mode: u32,
        new_mode: u32,
    },
}

impl PatchFilePart {
    pub fn semver_exclusivity(&self) -> Option<&zpm_semver::Range> {
        match self {
            PatchFilePart::FilePatch { semver_exclusivity, .. } => semver_exclusivity.as_ref(),
            PatchFilePart::FileDeletion { semver_exclusivity, .. } => semver_exclusivity.as_ref(),
            PatchFilePart::FileCreation { semver_exclusivity, .. } => semver_exclusivity.as_ref(),
            PatchFilePart::FileRename { semver_exclusivity, .. } => semver_exclusivity.as_ref(),
            PatchFilePart::FileModeChange { semver_exclusivity, .. } => semver_exclusivity.as_ref(),
        }
    }

    pub fn source_path(&self) -> &Path {
        match self {
            PatchFilePart::FilePatch { path, .. } => path,
            PatchFilePart::FileDeletion { path, .. } => path,
            PatchFilePart::FileCreation { path, .. } => path,
            PatchFilePart::FileRename { from, .. } => from,
            PatchFilePart::FileModeChange { path, .. } => path,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct FileDeets<'a> {
    semver_exclusivity: Option<&'a str>,

    diff_line_from_path: Option<Cow<'a, str>>,
    diff_line_to_path: Option<Cow<'a, str>>,

    old_mode: Option<&'a str>,
    new_mode: Option<&'a str>,

    deleted_file_mode: Option<&'a str>,
    new_file_mode: Option<&'a str>,

    rename_from: Option<Cow<'a, str>>,
    rename_to: Option<Cow<'a, str>>,

    before_hash: Option<&'a str>,
    after_hash: Option<&'a str>,

    from_path: Option<Cow<'a, str>>,
    to_path: Option<Cow<'a, str>>,

    hunks: Vec<Hunk>,
}

#[derive(PartialEq, Eq)]
enum ParseState {
    Header,
    Hunks,
}

pub struct PatchParser<'a> {
    result: Vec<PatchFilePart>,

    current_file_patch: FileDeets<'a>,
    current_state: ParseState,
    current_hunk: Option<Hunk>,
    current_hunk_mutation_part: Option<PatchMutationPart>,
}

fn parse_file_mode(mode: &str) -> Result<u32, Error> {
    let mode = u32::from_str_radix(mode, 8)? & 0o777;

    if mode != 0o644 && mode != 0o755 {
        return Err(Error::InvalidModeInPatchFile(mode));
    }

    Ok(mode)
}

impl Default for PatchParser<'_> {
    fn default() -> Self {
        PatchParser {
            result: Vec::new(),

            current_file_patch: FileDeets::default(),
            current_state: ParseState::Header,
            current_hunk: None,
            current_hunk_mutation_part: None,
        }
    }
}

impl<'a> PatchParser<'a> {
    pub fn parse(content: &'a str) -> Result<Vec<PatchFilePart>, Error> {
        let mut parser = PatchParser::new();
        parser.process(content)
    }

    fn new() -> Self {
        Self::default()
    }

    fn commit_hunk(&mut self) {
        if let Some(mut hunk) = self.current_hunk.take() {
            if let Some(hunk_mutation_part) = self.current_hunk_mutation_part.take() {
                hunk.parts.push(hunk_mutation_part);
            }

            self.current_file_patch.hunks.push(hunk);
        }
    }

    fn commit_file_patch(&mut self) -> Result<(), Error> {
        self.commit_hunk();

        let file_patch
            = std::mem::take(&mut self.current_file_patch);

        for hunk in &file_patch.hunks {
            if !hunk.verify_integrity() {
                return Err(Error::HunkIntegrityCheckFailed);
            }
        }

        let semver_exclusivity = file_patch.semver_exclusivity
            .map(zpm_semver::Range::from_str)
            .transpose()?;

        let mut current_destination_file_path = None;

        if let Some(rename_from) = file_patch.rename_from {
            let rename_to = file_patch.rename_to
                .ok_or(Error::MissingRenameTarget)?;

            self.result.push(PatchFilePart::FileRename {
                semver_exclusivity: semver_exclusivity.clone(),
                from: Path::try_from(rename_from.as_ref())?,
                to: Path::try_from(rename_to.as_ref())?,
            });

            current_destination_file_path = Some(Path::try_from(rename_to.as_ref())?);
        } else if let Some(deleted_file_mode) = file_patch.deleted_file_mode {
            let path = file_patch.diff_line_from_path
                .or(file_patch.from_path)
                .ok_or(Error::MissingFromPath)?;

            self.result.push(PatchFilePart::FileDeletion {
                semver_exclusivity: semver_exclusivity.clone(),
                path: Path::try_from(path.as_ref())?,
                mode: parse_file_mode(deleted_file_mode)?,
                hunk: file_patch.hunks.first().cloned(),
                hash: file_patch.before_hash.map(|s| s.to_string()),
            });
        } else if let Some(new_file_mode) = file_patch.new_file_mode {
            let path = file_patch.diff_line_to_path
                .or(file_patch.to_path)
                .ok_or(Error::MissingToPath)?;

            self.result.push(PatchFilePart::FileCreation {
                semver_exclusivity: semver_exclusivity.clone(),
                path: Path::try_from(path.as_ref())?,
                mode: parse_file_mode(new_file_mode)?,
                hunk: file_patch.hunks.first().cloned(),
                hash: file_patch.after_hash.map(|s| s.to_string()),
            });
        } else {
            current_destination_file_path = file_patch.to_path
                .or(file_patch.diff_line_to_path)
                .map(|path| Path::try_from(path.as_ref()))
                .transpose()?;
        }

        if let Some(current_destination_file_path) = &current_destination_file_path {
            if let (Some(old_mode), Some(new_mode)) = (file_patch.old_mode, file_patch.new_mode) {
                if old_mode != new_mode {
                    self.result.push(PatchFilePart::FileModeChange {
                        semver_exclusivity: semver_exclusivity.clone(),
                        path: current_destination_file_path.clone(),
                        old_mode: parse_file_mode(old_mode)?,
                        new_mode: parse_file_mode(new_mode)?,
                    });
                }
            }

            if !file_patch.hunks.is_empty() {
                self.result.push(PatchFilePart::FilePatch {
                    semver_exclusivity,
                    path: current_destination_file_path.clone(),
                    hunks: file_patch.hunks,
                    before_hash: file_patch.before_hash.map(|s| s.to_string()),
                    after_hash: file_patch.after_hash.map(|s| s.to_string()),
                });
            }
        }

        Ok(())
    }

    fn process(&mut self, content: &'a str) -> Result<Vec<PatchFilePart>, Error> {
        static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"\r\n|\r|\n").unwrap()
        });

        let mut lines = SEPARATOR
            .split(content)
            .collect::<Vec<_>>();

        if lines.last() == Some(&"") {
            lines.pop();
        }
    
        let mut idx = 0;

        while idx < lines.len() {
            let line = lines[idx];

            match self.current_state {
                ParseState::Header => {
                    if line.starts_with("@@") {
                        self.current_state = ParseState::Hunks;
                        continue;
                    } else if line.starts_with("diff --git ") {
                        if self.current_file_patch.diff_line_from_path.is_some() {
                            self.commit_file_patch()?;
                        }

                        let (from, to)
                            = parse_diff_line_paths(line)
                                .ok_or_else(|| Error::InvalidDiffLine(line.to_string()))?;

                        self.current_file_patch.diff_line_from_path = Some(from);
                        self.current_file_patch.diff_line_to_path = Some(to);
                    }  else if let Some(rest) = line.strip_prefix("old mode ") {
                        self.current_file_patch.old_mode = Some(rest);
                    } else if let Some(rest) = line.strip_prefix("new mode ") {
                        self.current_file_patch.new_mode = Some(rest);
                    } else if let Some(rest) = line.strip_prefix("deleted file mode ") {
                        self.current_file_patch.deleted_file_mode = Some(rest);
                    } else if let Some(rest) = line.strip_prefix("new file mode ") {
                        self.current_file_patch.new_file_mode = Some(rest);
                    } else if let Some(rest) = line.strip_prefix("rename from ") {
                        self.current_file_patch.rename_from = Some(parse_header_path(rest).ok_or_else(|| Error::InvalidPatchPath(line.to_string()))?);
                    } else if let Some(rest) = line.strip_prefix("rename to ") {
                        self.current_file_patch.rename_to = Some(parse_header_path(rest).ok_or_else(|| Error::InvalidPatchPath(line.to_string()))?);
                    } else if line.starts_with("index ") {
                        if let Some(m) = INDEX_REGEXP.captures(line) {
                            self.current_file_patch.before_hash = m.get(1).map(|m| m.as_str());
                            self.current_file_patch.after_hash = m.get(2).map(|m| m.as_str());
                        }
                    } else if let Some(rest) = line.strip_prefix("semver exclusivity ") {
                        self.current_file_patch.semver_exclusivity = Some(rest);
                    } else if let Some(rest) = line.strip_prefix("--- ") {
                        self.current_file_patch.from_path = Some(parse_file_line_path(rest).ok_or_else(|| Error::InvalidPatchPath(line.to_string()))?);
                    } else if let Some(rest) = line.strip_prefix("+++ ") {
                        self.current_file_patch.to_path = Some(parse_file_line_path(rest).ok_or_else(|| Error::InvalidPatchPath(line.to_string()))?);
                    }
                }

                ParseState::Hunks => {
                    let line_type = match line.chars().next() {
                        Some('@') => PatchMutationPartKind::Header,
                        Some('\\') => PatchMutationPartKind::Pragma,
                        Some(' ') => PatchMutationPartKind::Context,
                        Some('+') => PatchMutationPartKind::Insertion,
                        Some('-') => PatchMutationPartKind::Deletion,
                        None => PatchMutationPartKind::Context,
                        _ => PatchMutationPartKind::Unknown,
                    };

                    match line_type {
                        PatchMutationPartKind::Unknown => {
                            self.current_state = ParseState::Header;
                            self.commit_file_patch()?;

                            continue;
                        },

                        PatchMutationPartKind::Header => {
                            self.commit_hunk();

                            let hunk_header
                                = HunkHeader::from_file_string(line)?;

                            self.current_hunk = Some(Hunk {
                                header: hunk_header,
                                parts: Vec::new(),
                            });
                        },

                        PatchMutationPartKind::Pragma => {
                            if !line.starts_with("\\ No newline at end of file") {
                                return Err(Error::UnrecognizedPatchPragma(line.to_string()));
                            }

                            let current_hunk_mutation_part = self.current_hunk_mutation_part
                                .as_mut()
                                .ok_or(Error::UnsufficientPragmaContext)?;

                            current_hunk_mutation_part.no_newline_at_eof = true;
                        },

                        PatchMutationPartKind::Context | PatchMutationPartKind::Insertion | PatchMutationPartKind::Deletion => {
                            let current_hunk = self.current_hunk
                                .as_mut()
                                .ok_or(Error::HunkLinesBeforeHeader)?;

                            if let Some(current_hunk_mutation_part) = self.current_hunk_mutation_part.as_mut() {
                                if current_hunk_mutation_part.kind != line_type {
                                    let current_hunk_mutation_part
                                        = self.current_hunk_mutation_part.take()
                                            .expect("Expected the current hunk mutation part to exist, since we checked for that right before");

                                    current_hunk.parts.push(current_hunk_mutation_part);
                                }
                            }

                            if self.current_hunk_mutation_part.is_none() {
                                self.current_hunk_mutation_part = Some(PatchMutationPart {
                                    kind: line_type,
                                    lines: Vec::new(),
                                    no_newline_at_eof: false,
                                });
                            }

                            let current_hunk_mutation_part = self.current_hunk_mutation_part
                                .as_mut()
                                .expect("Expected the current hunk mutation part to have been set if missing");

                            if !line.is_empty() {
                                current_hunk_mutation_part.lines.push(line[1..].to_string());
                            } else {
                                current_hunk_mutation_part.lines.push("".to_string());
                            }
                        },
                    }
                }
            }

            idx += 1;
        }

        self.commit_file_patch()?;

        if self.result.is_empty() {
            return Err(Error::EmptyPatchFile);
        }

        Ok(std::mem::take(&mut self.result))
    }
}
