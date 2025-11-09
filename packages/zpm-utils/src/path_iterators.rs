use std::str::FromStr;

use crate::Path;

pub struct PathIterator<'a> {
    path_str: &'a str,
    lookup_idx: Option<(usize, usize)>,
    emit_empty_path: bool,
}

impl<'a> PathIterator<'a> {
    pub fn new(path: &'a Path) -> Self {
        let path_str
            = path.as_str();
        let lookup_idx
            = Some((0, path_str.len()));
        let emit_empty_path
            = path.is_relative();

        Self {
            path_str,
            lookup_idx,
            emit_empty_path,
        }
    }
}

impl<'a> Iterator for PathIterator<'a> {
    type Item = Path;

    fn next(&mut self) -> Option<Self::Item> {
        let Some((lookup_idx, back_idx)) = self.lookup_idx else {
            return None;
        };

        if lookup_idx == 0 && self.emit_empty_path {
            self.emit_empty_path = false;
            return Some(Path::new());
        }

        let next_slash_idx
            = self.path_str[lookup_idx..]
                .find('/')
                .map(|idx| idx + lookup_idx + 1)
                .unwrap_or(back_idx);

        self.lookup_idx = if back_idx > next_slash_idx {
            Some((next_slash_idx + 1, back_idx))
        } else {
            None
        };

        let mut sub_path
            = &self.path_str[0..next_slash_idx];

        if sub_path.ends_with('/') && sub_path.len() > 1 {
            sub_path = &sub_path[..sub_path.len() - 1];
        }

        Some(Path::from_str(sub_path).unwrap())
    }
}

impl<'a> DoubleEndedIterator for PathIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let Some((lookup_idx, back_idx)) = self.lookup_idx else {
            return None;
        };

        if back_idx == 0 && self.emit_empty_path {
            self.emit_empty_path = false;
            self.lookup_idx = None;
            return Some(Path::new());
        }

        let last_slash_idx
            = self.path_str[lookup_idx..back_idx]
                .strip_suffix('/')
                .unwrap_or(&self.path_str[lookup_idx..back_idx])
                .rfind('/')
                .map(|idx| idx + lookup_idx + 1)
                .unwrap_or(lookup_idx);

        self.lookup_idx = if lookup_idx < last_slash_idx || (lookup_idx == 0 && last_slash_idx == 0 && self.emit_empty_path) {
            Some((lookup_idx, last_slash_idx))
        } else {
            None
        };

        let mut sub_path
            = &self.path_str[0..back_idx];

        if sub_path.ends_with('/') && sub_path.len() > 1 {
            sub_path = &sub_path[..sub_path.len() - 1];
        }

        Some(Path::from_str(sub_path).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("/a/b/c", vec!["/", "/a", "/a/b", "/a/b/c"])]
    #[case("/a/b/c/", vec!["/", "/a", "/a/b", "/a/b/c"])]
    #[case("a/b/c", vec!["a", "a/b", "a/b/c"])]
    #[case("", vec![""])]
    #[case("/", vec!["/"])]
    fn test_path_iterator(#[case] path: Path, #[case] expected: Vec<&'static str>) {
        let yielded_paths = path
            .iter_path()
            .collect::<Vec<_>>();

        let yielded_path_strs = yielded_paths.iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>();

        assert_eq!(yielded_path_strs, expected);
    }

    #[rstest]
    #[case("/a/b/c", vec!["/a/b/c", "/a/b", "/a", "/"])]
    #[case("/a/b/c/", vec!["/a/b/c", "/a/b", "/a", "/"])]
    #[case("a/b/c", vec!["a/b/c", "a/b", "a"])]
    #[case("", vec![""])]
    #[case("/", vec!["/"])]
    fn test_path_iterator_reverse(#[case] path: Path, #[case] expected: Vec<&'static str>) {
        let yielded_paths = path
            .iter_path()
            .rev()
            .collect::<Vec<_>>();

        let yielded_path_strs = yielded_paths.iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>();

        assert_eq!(yielded_path_strs, expected);
    }
}
