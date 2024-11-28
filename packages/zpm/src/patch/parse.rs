use std::{str::FromStr, sync::LazyLock};

use arca::Path;
use regex::Regex;

use crate::{error::Error, semver, yarn_serialization_protocol};

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

yarn_serialization_protocol!(HunkHeader, {
    deserialize(src) {
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
});

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
        semver_exclusivity: Option<semver::Range>,
        path: Path,
        hunks: Vec<Hunk>,
        before_hash: Option<String>,
        after_hash: Option<String>,
    },

    FileDeletion {
        semver_exclusivity: Option<semver::Range>,
        path: Path,
        mode: u32,
        hunk: Option<Hunk>,
        hash: Option<String>,
    },

    FileCreation {
        semver_exclusivity: Option<semver::Range>,
        path: Path,
        mode: u32,
        hunk: Option<Hunk>,
        hash: Option<String>,
    },

    FileRename {
        semver_exclusivity: Option<semver::Range>,
        from: Path,
        to: Path,
    },

    FileModeChange {
        semver_exclusivity: Option<semver::Range>,
        path: Path,
        old_mode: u32,
        new_mode: u32,
    },
}

impl PatchFilePart {
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

    diff_line_from_path: Option<&'a str>,
    diff_line_to_path: Option<&'a str>,

    old_mode: Option<&'a str>,
    new_mode: Option<&'a str>,

    deleted_file_mode: Option<&'a str>,
    new_file_mode: Option<&'a str>,

    rename_from: Option<&'a str>,
    rename_to: Option<&'a str>,

    before_hash: Option<&'a str>,
    after_hash: Option<&'a str>,

    from_path: Option<&'a str>,
    to_path: Option<&'a str>,

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

impl<'a> Default for PatchParser<'a> {
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

    fn commit_hunk(&mut self) -> () {
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
            = std::mem::replace(&mut self.current_file_patch, FileDeets::default());

        for hunk in &file_patch.hunks {
            if !hunk.verify_integrity() {
                return Err(Error::HunkIntegrityCheckFailed);
            }
        }

        let semver_exclusivity = file_patch.semver_exclusivity
            .map(|s| semver::Range::from_str(s))
            .transpose()?;

        let mut current_destination_file_path = None;

        if let Some(rename_from) = file_patch.rename_from {
            let rename_to = file_patch.rename_to
                .ok_or(Error::MissingRenameTarget)?;

            self.result.push(PatchFilePart::FileRename {
                semver_exclusivity: semver_exclusivity.clone(),
                from: Path::from(rename_from),
                to: Path::from(rename_to),
            });

            current_destination_file_path = Some(Path::from(rename_to));
        } else if let Some(deleted_file_mode) = file_patch.deleted_file_mode {
            let path = file_patch.diff_line_from_path
                .or(file_patch.from_path)
                .ok_or(Error::MissingFromPath)?;

            self.result.push(PatchFilePart::FileDeletion {
                semver_exclusivity: semver_exclusivity.clone(),
                path: Path::from(path),
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
                path: Path::from(path),
                mode: parse_file_mode(new_file_mode)?,
                hunk: file_patch.hunks.first().cloned(),
                hash: file_patch.after_hash.map(|s| s.to_string()),
            });
        } else {
            current_destination_file_path = file_patch.to_path
                .or(file_patch.diff_line_to_path)
                .map(Path::from);
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

            if file_patch.hunks.len() > 0 {
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

                        let m
                            = DIFF_LINE_REGEXP.captures(line)
                                .ok_or_else(|| Error::InvalidDiffLine(line.to_string()))?;

                        self.current_file_patch.diff_line_from_path = m.get(1).map(|m| m.as_str());
                        self.current_file_patch.diff_line_to_path = m.get(2).map(|m| m.as_str());
                    }  else if line.starts_with("old mode ") {
                        self.current_file_patch.old_mode = Some(&line[9..]);
                    } else if line.starts_with("new mode ") {
                        self.current_file_patch.new_mode = Some(&line[9..]);
                    } else if line.starts_with("deleted file mode ") {
                        self.current_file_patch.deleted_file_mode = Some(&line[18..]);
                    } else if line.starts_with("new file mode ") {
                        self.current_file_patch.new_file_mode = Some(&line[14..]);
                    } else if line.starts_with("rename from ") {
                        self.current_file_patch.rename_from = Some(&line[12..]);
                    } else if line.starts_with("rename to ") {
                        self.current_file_patch.rename_to = Some(&line[10..]);
                    } else if line.starts_with("index ") {
                        if let Some(m) = INDEX_REGEXP.captures(line) {
                            self.current_file_patch.before_hash = m.get(1).map(|m| m.as_str());
                            self.current_file_patch.after_hash = m.get(2).map(|m| m.as_str());
                        }
                    } else if line.starts_with("semver exclusivity ") {
                        self.current_file_patch.semver_exclusivity = Some(&line[20..]);
                    } else if line.starts_with("--- ") {
                        self.current_file_patch.from_path = Some(&line[6..]);
                    } else if line.starts_with("+++ ") {
                        self.current_file_patch.to_path = Some(&line[6..]);
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
                                = HunkHeader::from_str(line)?;

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

                            if line.len() > 0 {
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

        if self.result.len() == 0 {
            return Err(Error::EmptyPatchFile);
        }

        Ok(std::mem::take(&mut self.result))
    }
}
