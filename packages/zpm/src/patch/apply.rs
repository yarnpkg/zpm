use std::{borrow::Cow, cmp, collections::BTreeMap};

use crate::{error::Error, formats::Entry};

use super::parse::{Hunk, PatchFilePart, PatchMutationPartKind};

#[cfg(test)]
#[path = "./apply.test.rs"]
mod apply_tests;

enum Modification {
    Push(String),
    Pop,
    Splice {
        index: usize,
        num_to_delete: usize,
        lines_to_insert: Vec<String>,
    },
}

fn evaluate_hunk(hunk: &Hunk, file_lines: &[String], mut offset: usize) -> Option<Vec<Modification>> {
    let mut modifications = vec![];

    for part in &hunk.parts {
        match part.kind {
            PatchMutationPartKind::Context | PatchMutationPartKind::Deletion => {
                for line in part.lines.iter() {
                    if offset >= file_lines.len() {
                        return None;
                    }

                    if file_lines[offset].as_str().trim_end() != line.trim_end() {
                        return None;
                    }

                    offset += 1;
                }

                if part.kind == PatchMutationPartKind::Deletion {
                    modifications.push(Modification::Splice {
                        index: offset - part.lines.len(),
                        num_to_delete: part.lines.len(),
                        lines_to_insert: vec![],
                    });

                    if part.no_newline_at_eof {
                        modifications.push(Modification::Push("".to_string()));
                    }
                }
            },

            PatchMutationPartKind::Insertion => {
                modifications.push(Modification::Splice {
                    index: offset,
                    num_to_delete: 0,
                    lines_to_insert: part.lines.clone(),
                });

                if part.no_newline_at_eof {
                    modifications.push(Modification::Pop);
                }
            },

            _ => {
                unimplemented!();
            },
        }
    }

    Some(modifications)
}

pub fn apply_patch<'a>(entries: Vec<Entry<'a>>, patch: &str) -> Result<Vec<Entry<'a>>, Error> {
    let mut entry_map = entries.into_iter()
        .map(|entry| (entry.name.clone(), entry))
        .collect::<BTreeMap<_, _>>();

    let patch_entries
        = crate::patch::parse::PatchParser::parse(patch)?;

    for patch_entry in patch_entries.iter() {
        match patch_entry {
            PatchFilePart::FileCreation {path, mode, hunk, ..} => {
                if entry_map.contains_key(path.as_str()) {
                    return Err(Error::ReplaceMe);
                }

                let file_contents = match hunk {
                    Some(hunk) => hunk.parts[0].lines.join("\n") + if hunk.parts[0].no_newline_at_eof { "" } else { "\n" },
                    None => "".to_string(),
                };

                let data = file_contents.as_bytes().to_vec();

                let entry = Entry {
                    name: path.to_string(),
                    mode: *mode as u64,
                    crc: 0,
                    data: Cow::Owned(data),
                };

                entry_map.insert(path.to_string(), entry);
            },

            PatchFilePart::FileDeletion {path, ..} => {
                entry_map
                    .remove(path.as_str())
                    .ok_or_else(|| Error::PatchedFileNotFound(path.to_string()))?;
            },

            PatchFilePart::FileModeChange {path, new_mode, ..} => {
                let entry = entry_map
                    .get_mut(path.as_str())
                    .ok_or_else(|| Error::PatchedFileNotFound(path.to_string()))?;

                entry.mode = *new_mode as u64;
            },

            PatchFilePart::FileRename {from, to, ..} => {
                let entry = entry_map
                    .remove(from.as_str())
                    .ok_or_else(|| Error::PatchedFileNotFound(from.to_string()))?;

                entry_map.insert(to.to_string(), entry);
            },

            PatchFilePart::FilePatch {path, hunks, ..} => {
                let entry = entry_map
                    .get_mut(path.as_str())
                    .ok_or_else(|| Error::PatchedFileNotFound(path.to_string()))?;

                let mut file_lines = std::str::from_utf8(&entry.data)?
                    .split('\n')
                    .map(str::to_string)
                    .collect::<Vec<_>>();

                let mut all_modifications = vec![];

                let mut fixup_offset: isize = 0;
                let mut max_frozen_line = 0;

                for (hunk_idx, hunk) in hunks.iter().enumerate() {
                    let first_guess = cmp::max(max_frozen_line, hunk.header.modified.start.checked_add_signed(fixup_offset).unwrap());

                    let max_prefix_fuzz = first_guess.saturating_sub(max_frozen_line);
                    let max_suffix_fuzz = file_lines.len().saturating_sub(first_guess).saturating_sub(hunk.header.original.length);

                    let max_fuzz = cmp::max(max_prefix_fuzz, max_suffix_fuzz);

                    let mut offset = 0;
                    let mut location = 0;

                    let mut modifications = None;
                    let mut next_fixup_offset = fixup_offset;

                    while offset <= max_fuzz {
                        if offset <= max_prefix_fuzz {
                            location = first_guess - offset;
                            if let Some(hunk_modifications) = evaluate_hunk(hunk, &file_lines, location) {
                                modifications = Some(hunk_modifications);
                                next_fixup_offset = fixup_offset - offset as isize;
                                break;
                            }
                        }

                        if offset <= max_suffix_fuzz {
                            location = first_guess + offset;
                            if let Some(hunk_modifications) = evaluate_hunk(hunk, &file_lines, location) {
                                modifications = Some(hunk_modifications);
                                next_fixup_offset = fixup_offset + offset as isize;
                                break;
                            }
                        }

                        offset += 1;
                    }

                    let modifications = modifications
                        .ok_or_else(|| Error::UnmatchedHunk(hunk_idx))?;

                    all_modifications.push(modifications);

                    fixup_offset = next_fixup_offset;
                    max_frozen_line = location + hunk.header.original.length;
                }

                let mut diff_offset = 0;

                for modification in all_modifications.into_iter() {
                    for modification in modification.into_iter() {
                        match modification {
                            Modification::Push(line) => {
                                file_lines.push(line);
                            },

                            Modification::Pop => {
                                file_lines.pop();
                            },

                            Modification::Splice { index, num_to_delete, lines_to_insert } => {
                                let first_line = index.checked_add_signed(diff_offset).unwrap();
                                diff_offset += lines_to_insert.len() as isize - num_to_delete as isize;

                                file_lines.splice(first_line..first_line + num_to_delete, lines_to_insert.into_iter());
                            },
                        }
                    }
                }

                entry.data = Cow::Owned(file_lines.join("\n").as_bytes().to_vec());
            },
        }
    }

    let entries = entry_map.into_values()
        .collect::<Vec<_>>();

    Ok(entries)
}
