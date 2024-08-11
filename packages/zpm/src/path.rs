use arca::{Path, ToArcaPath};

pub fn home(sub: &Path) -> Path {
    #[allow(deprecated)]
    std::env::home_dir()
        .map(|dir| dir.to_arca().with_join(sub))
        .unwrap()
}
