use arca::{Path, ToArcaPath};
use walkdir::WalkDir;

use crate::error::Error;
use crate::manifest::BinField;
use crate::project::{Project, Workspace};

static OVERRIDES: &'static [&'static str] = &[
    // Those patterns must always be packed

    "package.json",

    "readme",
    "readme.*",

    "license",
    "license.*",

    "licence",
    "licence.*",

    "changelog",
    "changelog.*",

    // Those patterns must never be packed

    "!yarn.lock",
    "!.pnp.cjs",
    "!.pnp.loader.mjs",
    "!.pnp.data.json",

    "!.yarn/**",

    "!package.tgz",
    "!package.tar",
  
    "!.github/**",
    "!.git/**",
    "!.hg/**",
    "!node_modules/**",

    "!.gitignore",

    "!.#*",
    "!.DS_Store",
];

pub fn pack_list(_project: &Project, workspace: &Workspace) -> Result<Vec<Path>, Error> {
    let mut entries = vec![];

    let mut raw_patterns = match &workspace.manifest.files {
        Some(files) => files.clone(),
        None => vec!["**/*".to_string()],
    };

    raw_patterns.extend(OVERRIDES.iter().map(|s| s.to_string()));

    if let Some(main) = &workspace.manifest.main {
        raw_patterns.push(format!("/{}", main));
    }

    // TODO: Deprecate/remove in a future release
    if let Some(module) = &workspace.manifest.module {
        raw_patterns.push(format!("/{}", module));
    }

    // TODO: Deprecate/remove in a future release
    if let Some(browser) = &workspace.manifest.browser {
        raw_patterns.push(format!("/{}", browser));
    }

    if let Some(bin) = &workspace.manifest.bin {
        match bin {
            BinField::String(target) => {
                raw_patterns.push(format!("/{}", target));
            },
            BinField::Map(targets) => {
                for target in targets.values() {
                    raw_patterns.push(format!("/{}", target));
                }
            },
        }
    }

    let mut regular_glob_build = globset::GlobSetBuilder::new();
    let mut negated_glob_build = globset::GlobSetBuilder::new();

    for raw_pattern in raw_patterns {
        let (negated, base_pattern) = match raw_pattern.starts_with("!") {
            true => (true, &raw_pattern[1..]),
            false => (false, raw_pattern.as_str()),
        };

        let processed_patterns = match base_pattern.starts_with("/") {
            true => vec![(negated, format!("{}", base_pattern)), (negated, format!("{}/**", base_pattern))],
            false => match base_pattern.contains("/") {
                true => vec![(negated, format!("/{}", base_pattern)), (negated, format!("/{}/**", base_pattern))],
                false => vec![(negated, format!("/**/{}", base_pattern)), (negated, format!("/**/{}/**", base_pattern))],
            },
        };

        for (negated, pattern) in processed_patterns {
            let glob = globset::Glob::new(&pattern)
                .map_err(|_| Error::InvalidFilePattern(pattern))?;

            if negated {
                negated_glob_build.add(glob);
            } else {
                regular_glob_build.add(glob);
            }
        }
    }

    let regular_glob = regular_glob_build.build()
        .expect("Expected the glob pattern to be valid");
    let negated_glob = negated_glob_build.build()
        .expect("Expected the glob pattern to be valid");

    let walk = WalkDir::new(workspace.path.to_path_buf())
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| match err.into_io_error() {
            Some(err) => err.into(),
            None => Error::Unsupported,
        })?;

    for entry in walk {
        if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
            continue;
        }

        let rel_path = entry.path()
            .to_arca()
            .relative_to(&workspace.path);

        let rooted_path = Path::from("/")
            .with_join(&rel_path);

        let candidate
            = globset::Candidate::new(rooted_path.as_str());

        if !regular_glob.is_match_candidate(&candidate) || negated_glob.is_match_candidate(&candidate) {
            continue;
        }

        entries.push(rel_path);
    }

    Ok(entries)
}
