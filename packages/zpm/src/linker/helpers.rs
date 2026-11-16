use std::{collections::{BTreeMap, BTreeSet}, vec};

use zpm_formats::iter_ext::IterExt;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, VersionFilter, Locator, Reference};
use zpm_utils::{IoResultExt, Path, PathError, System, ToHumanString};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::{
    build,
    error::Error,
    fetchers::PackageData,
    install::Install,
    project::Project,
    resolvers::Resolution,
    tree_resolver::ResolutionTree,
};

#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMeta {
    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub built: Option<bool>,

    #[serde(default, skip_serializing_if = "zpm_utils::is_default")]
    pub unplugged: Option<bool>,
}

#[serde_as]
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopLevelConfiguration {
    #[serde(default)]
    #[serde_as(as = "BTreeMap<_, _>")]
    dependencies_meta: Vec<(VersionFilter, PackageMeta)>,
}

impl TopLevelConfiguration {
    pub fn from_project(project: &Project) -> Vec<(VersionFilter, PackageMeta)> {
        project.manifest_path()
            .if_exists()
            .and_then(|path| path.fs_read_text().ok()).map(|data| JsonDocument::hydrate_from_str::<TopLevelConfiguration>(&data).unwrap().dependencies_meta)
            .unwrap_or_default()
    }
}

pub fn fs_remove_nm(nm_path: Path) -> Result<(), Error> {
    // Not going through `fs_read_dir`, as a missing folder is expected and
    // shouldn't build a `PathError`
    let entries = std::fs::read_dir(nm_path.to_path_buf())
        .ok_missing()
        .map_err(PathError::from)?;

    match entries {
        None
            => Ok(()),

        Some(entries) => {
            let mut has_dot_entries = false;

            for entry in entries.flatten() {
                let path
                    = Path::try_from(entry.path())?;

                let basename = path.basename()
                    .unwrap();

                if basename.starts_with(".") && basename != ".bin" && path.fs_is_dir() {
                    has_dot_entries = true;
                    continue;
                }

                path.fs_rm()
                    .unwrap();
            }

            if !has_dot_entries {
                nm_path.fs_rm()?;
            }

            Ok(())
        },
    }
}

pub fn fs_extract_archive(destination: &Path, package_data: &PackageData) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::Classic, false)
}

/// Like [`fs_extract_archive`] but hardlinks each file through a
/// content-addressed index at `index_root` so identical contents share
/// one inode across packages and projects. Modified hardlinks are
/// detected via the mtime stamp and repaired in place.
///
/// `skip_node_modules` leaves out the archive's own `node_modules`, for
/// a destination whose `node_modules` holds nested packages instead.
///
/// Files that can't be hardlinked (see [`CopyFallbackReason`]) are
/// copied instead and counted in `fallbacks`.
pub fn fs_extract_archive_with_cas(destination: &Path, package_data: &PackageData, index_root: &Path, skip_node_modules: bool, fallbacks: &CopyFallbacks) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::Cas { index_root, fallbacks }, skip_node_modules)
}

/// Project-local dedup with no side-channel index. The first
/// destination for a given content hash is written normally and
/// recorded in the index; later destinations hardlink to it.
/// `skip_node_modules` and `fallbacks` are as for
/// [`fs_extract_archive_with_cas`].
pub fn fs_extract_archive_with_local_dedup(
    destination: &Path,
    package_data: &PackageData,
    local_index: &LocalDedupIndex,
    skip_node_modules: bool,
    fallbacks: &CopyFallbacks,
) -> Result<bool, Error> {
    fs_extract_archive_impl(destination, package_data, ExtractMode::LocalDedup { local_index, fallbacks }, skip_node_modules)
}

const DEDUP_SHARDS: usize = 64;

/// Content-hash index shared between parallel extractions. Sharded by
/// the hash's first byte so identical contents serialize on one lock
/// while unrelated files proceed concurrently.
pub struct LocalDedupIndex {
    shards: Vec<std::sync::Mutex<BTreeMap<zpm_utils::Hash64, Path>>>,
}

impl LocalDedupIndex {
    pub fn new() -> Self {
        Self {
            shards: (0..DEDUP_SHARDS)
                .map(|_| std::sync::Mutex::new(BTreeMap::new()))
                .collect(),
        }
    }

    fn shard(&self, hash: &zpm_utils::Hash64) -> &std::sync::Mutex<BTreeMap<zpm_utils::Hash64, Path>> {
        &self.shards[hash.first_byte() as usize % DEDUP_SHARDS]
    }
}

impl Default for LocalDedupIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializes concurrent global-CAS writes of identical contents;
/// without it, two threads could interleave writes to the same index
/// entry. Keyed by the entry's content hash.
fn cas_shard_lock(first_byte: u8) -> &'static std::sync::Mutex<()> {
    use std::sync::OnceLock;

    static LOCKS: OnceLock<Vec<std::sync::Mutex<()>>> = OnceLock::new();

    let locks = LOCKS.get_or_init(|| (0..DEDUP_SHARDS).map(|_| std::sync::Mutex::new(())).collect());

    &locks[first_byte as usize % DEDUP_SHARDS]
}

enum ExtractMode<'a> {
    Classic,
    Cas { index_root: &'a Path, fallbacks: &'a CopyFallbacks },
    LocalDedup { local_index: &'a LocalDedupIndex, fallbacks: &'a CopyFallbacks },
}

fn fs_extract_archive_impl(destination: &Path, package_data: &PackageData, mode: ExtractMode<'_>, skip_node_modules: bool) -> Result<bool, Error> {
    let ready_path = destination
        .with_join_str(".ready");

    let already_extracted = ready_path.fs_exists();

    // Classic mode trusts the .ready marker and skips the zip walk.
    // CAS / local-dedup re-walk so out-of-band edits can be repaired.
    let skip_for_classic = matches!(mode, ExtractMode::Classic) && already_extracted;
    if skip_for_classic || matches!(package_data, &PackageData::MissingZip {..}) {
        return Ok(false);
    }

    let package_subpath
        = package_data.package_subpath();

    let package_bytes = match package_data {
        PackageData::Zip {archive_path, ..} => archive_path.fs_read()?,
        _ => panic!("Expected a zip archive"),
    };

    let entries
        = zpm_formats::zip::entries_from_zip(&package_bytes)?
            .into_iter()
            .strip_path_prefix(&package_subpath)
            .filter(|entry| !skip_node_modules || entry.name.components().next() != Some("node_modules"))
            .collect::<Vec<_>>();

    for entry in entries {
        let target_path = destination
            .with_join(&entry.name);

        target_path.fs_create_parent()?;

        match &mode {
            ExtractMode::Cas { index_root, fallbacks } => {
                fallbacks.record(link_into_cas(&target_path, &entry.data, entry.mode as u32, index_root)?);
            },
            ExtractMode::LocalDedup { local_index, fallbacks } => {
                fallbacks.record(link_into_local_index(&target_path, &entry.data, entry.mode as u32, local_index)?);
            },
            ExtractMode::Classic => {
                target_path
                    .fs_write(&entry.data)?
                    .fs_set_mode(entry.mode as u32)?;
            },
        }
    }

    if !already_extracted {
        ready_path
            .fs_write(vec![])?;
    }

    Ok(!already_extracted)
}

/// Why a file was copied instead of hardlinked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFallbackReason {
    /// The two paths are on different filesystems (`EXDEV`, or
    /// `ERROR_NOT_SAME_DEVICE` on Windows).
    CrossDevice,

    /// The source already has as many links as the filesystem allows
    /// (`EMLINK`, or `ERROR_TOO_MANY_LINKS` past NTFS' 1023 links).
    LinkLimit,

    /// An up-to-date copy from an earlier fallback could have become a
    /// link, but replacing it failed (on Windows, typically because a
    /// process has it open); the copy is kept.
    ReplaceFailed,
}

impl CopyFallbackReason {
    /// The hardlink failures we recover from by copying; any other
    /// error still fails the install.
    pub fn from_link_error(error: &std::io::Error) -> Option<Self> {
        match error.kind() {
            std::io::ErrorKind::CrossesDevices
                => Some(Self::CrossDevice),

            std::io::ErrorKind::TooManyLinks
                => Some(Self::LinkLimit),

            _
                => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkOutcome {
    Linked,
    Copied(CopyFallbackReason),
}

/// Counts the files an install had to copy instead of hardlinking, so
/// the linker can warn once at the end rather than once per file.
/// Shared between parallel extractions.
#[derive(Debug, Default)]
pub struct CopyFallbacks {
    cross_device: std::sync::atomic::AtomicUsize,
    link_limit: std::sync::atomic::AtomicUsize,
    replace_failed: std::sync::atomic::AtomicUsize,
}

pub const COPY_FALLBACK_DOCS_URL: &str = "https://yarnpkg.com/concepts/hardlinks";

impl CopyFallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&self, outcome: LinkOutcome) {
        let counter = match outcome {
            LinkOutcome::Linked => return,
            LinkOutcome::Copied(CopyFallbackReason::CrossDevice) => &self.cross_device,
            LinkOutcome::Copied(CopyFallbackReason::LinkLimit) => &self.link_limit,
            LinkOutcome::Copied(CopyFallbackReason::ReplaceFailed) => &self.replace_failed,
        };

        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// The warning to print, or `None` if every file got hardlinked.
    /// `store` is the global index the files link from, or `None` for
    /// `hardlinks-local` (where they link to each other).
    pub fn warning(&self, store: Option<&Path>, project_cwd: &Path) -> Option<String> {
        let cross_device_reason = match store {
            Some(store) => format!("the store at {} and the project at {} are on different filesystems", store.to_print_string(), project_cwd.to_print_string()),
            None => format!("parts of the project at {} are on different filesystems", project_cwd.to_print_string()),
        };

        let reasons = [
            (&self.cross_device, cross_device_reason),
            (&self.link_limit, "their content reached the filesystem's maximum number of hard links per file".to_string()),
            (&self.replace_failed, "existing copies couldn't be replaced by hard links".to_string()),
        ];

        let reasons
            = reasons.into_iter()
                .map(|(counter, reason)| (counter.load(std::sync::atomic::Ordering::Relaxed), reason))
                .filter(|(count, _)| *count > 0)
                .collect::<Vec<_>>();

        let total: usize
            = reasons.iter().map(|(count, _)| count).sum();

        let reason = match reasons.as_slice() {
            [] => return None,
            [(_, reason)] => format!("because {}", reason),
            _ => format!("({})", reasons.iter().map(|(count, reason)| format!("{} because {}", count, reason)).collect::<Vec<_>>().join(", ")),
        };

        let files = match total {
            1 => "1 file was".to_string(),
            _ => format!("{} files were", total),
        };

        Some(format!(
            "{} copied instead of hard-linked {}; installs are slower and use more disk space as a result (see {})",
            files,
            reason,
            COPY_FALLBACK_DOCS_URL,
        ))
    }

    pub fn warn_if_any(&self, store: Option<&Path>, project_cwd: &Path) {
        if let Some(message) = self.warning(store, project_cwd) {
            crate::report::if_active(|report| {
                report.warn(message);
            });
        }
    }
}

/// Ensures `target` is a hardlink to `source` (no-op if they already
/// share an inode), or a copy of it when the filesystem can't link
/// them. `source` must exist and hold `data`.
fn ensure_hardlink(target: &Path, source: &Path, data: &[u8]) -> Result<LinkOutcome, Error> {
    let injected_error
        = injected_link_error();

    let link = |source: &Path, target: &Path| match injected_error {
        Some(kind) => Err(kind.into()),
        None => std::fs::hard_link(source.to_path_buf(), target.to_path_buf()),
    };

    let rename = |from: &Path, to: &Path| {
        std::fs::rename(from.to_path_buf(), to.to_path_buf())
    };

    ensure_hardlink_with(target, source, data, &link, &rename)
}

/// Acceptance tests can't mount a second filesystem or create a
/// thousand links per file, so they make every link fail instead.
fn injected_link_error() -> Option<std::io::ErrorKind> {
    use std::sync::OnceLock;

    static INJECTED_ERROR: OnceLock<Option<std::io::ErrorKind>> = OnceLock::new();

    *INJECTED_ERROR.get_or_init(|| {
        if std::env::var("YARN_IS_TEST_ENV").is_err() {
            return None;
        }

        match std::env::var("YARN_INJECT_HARDLINK_ERROR").as_deref() {
            Ok("EXDEV") => Some(std::io::ErrorKind::CrossesDevices),
            Ok("EMLINK") => Some(std::io::ErrorKind::TooManyLinks),
            _ => None,
        }
    })
}

type FsOp<'a> = &'a dyn Fn(&Path, &Path) -> std::io::Result<()>;

fn ensure_hardlink_with(target: &Path, source: &Path, data: &[u8], link: FsOp<'_>, rename: FsOp<'_>) -> Result<LinkOutcome, Error> {
    if is_same_file(target, source) {
        return Ok(LinkOutcome::Linked);
    }

    // A copy left by an earlier fallback is kept as long as linking still
    // fails, so reinstalling over a copied tree doesn't rewrite it. The
    // probe goes next to it, and replaces it if linking works again (the
    // store moved, or an entry got below the link limit). The copy is
    // valid, so failing to replace it never fails the install.
    if is_fallback_copy_of(target, source, data) {
        return Ok(upgrade_fallback_copy(target, source, link, rename));
    }

    if target.fs_exists() {
        target.fs_rm_file().ok();
    }

    match link(source, target) {
        Ok(()) => {
            Ok(LinkOutcome::Linked)
        },

        Err(error) => {
            let Some(reason) = CopyFallbackReason::from_link_error(&error) else {
                return Err(zpm_utils::PathError::from(error).into());
            };

            // Copying carries the permissions over, so the executable bit
            // matches what the hardlink would have had
            source.fs_copy_file(target)?;

            Ok(LinkOutcome::Copied(reason))
        },
    }
}

fn upgrade_fallback_copy(target: &Path, source: &Path, link: FsOp<'_>, rename: FsOp<'_>) -> LinkOutcome {
    let probe_path = target.dirname()
        .expect("Expected the target to have a parent")
        .with_join_str(format!(".{}.link-probe", target.basename().unwrap_or_default()));

    // Also clears a probe left behind by an interrupted install
    let _ = probe_path.fs_rm_file().ok_missing();

    let outcome = match link(source, &probe_path) {
        Ok(()) => match rename(&probe_path, target) {
            Ok(()) => return LinkOutcome::Linked,
            Err(_) => LinkOutcome::Copied(CopyFallbackReason::ReplaceFailed),
        },

        Err(error) => LinkOutcome::Copied(
            CopyFallbackReason::from_link_error(&error)
                .unwrap_or(CopyFallbackReason::ReplaceFailed),
        ),
    };

    let _ = probe_path.fs_rm_file().ok_missing();

    outcome
}

/// Whether `target` is a regular file with the permissions of `source`
/// and the expected content. Comparing the content (rather than, say,
/// the mtime) catches both edits to the copy and package updates that
/// change a file without changing its size.
fn is_fallback_copy_of(target: &Path, source: &Path, data: &[u8]) -> bool {
    let (Ok(target_meta), Ok(source_meta)) = (target.fs_symlink_metadata(), source.fs_metadata()) else {
        return false;
    };

    target_meta.is_file()
        && target_meta.len() == data.len() as u64
        && target_meta.permissions() == source_meta.permissions()
        && target.fs_read_with_size(target_meta.len()).is_ok_and(|content| content == data)
}

/// Whether `target` (not following symlinks) and `source` are the same
/// file on disk.
fn is_same_file(target: &Path, source: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        // The target is usually missing (first install), which mustn't
        // cost a `PathError`
        let dest_meta = target.fs_symlink_metadata_if_exists().ok().flatten();
        let source_meta = source.fs_metadata_if_exists().ok().flatten();

        match (&dest_meta, &source_meta) {
            (Some(d), Some(s)) => d.dev() == s.dev() && d.ino() == s.ino(),
            _ => false,
        }
    }

    // The volume serial number and file index aren't exposed by the stable
    // std APIs on Windows, so we go through same-file (which compares them).
    #[cfg(windows)]
    {
        !target.fs_is_symlink() && same_file::is_same_file(target.to_path_buf(), source.to_path_buf()).unwrap_or(false)
    }
}

/// Writes `data` at `target` with `mode_bits` permissions, overwriting
/// if present.
fn write_canonical(target: &Path, data: &[u8], mode_bits: u32) -> Result<(), Error> {
    if target.fs_exists() {
        target.fs_rm_file().ok();
    }

    target
        .fs_write(data)?
        .fs_set_mode(mode_bits)?;

    Ok(())
}

fn link_into_local_index(
    target_path: &Path,
    data: &[u8],
    mode: u32,
    local_index: &LocalDedupIndex,
) -> Result<LinkOutcome, Error> {
    let mode_bits = mode & 0o777;
    let hash = zpm_utils::Hash64::from_data(data);

    let mut shard = local_index.shard(&hash)
        .lock()
        .unwrap();

    // Fall through to write_canonical when the recorded path is gone;
    // that re-registers a new home for this hash. The canonical write
    // happens under the shard lock so a concurrent extraction of the
    // same content can't hardlink to a half-written file.
    let canonical_path = shard.get(&hash).cloned()
        .filter(|p| p.fs_exists());

    let Some(canonical_path) = canonical_path else {
        write_canonical(target_path, data, mode_bits)?;
        shard.insert(hash, target_path.clone());
        return Ok(LinkOutcome::Linked);
    };

    drop(shard);

    ensure_hardlink(target_path, &canonical_path, data)
}

/// Extracts every entry of the package archive under `destination`,
/// with no completion marker; used for the unpacked store and as the
/// fallback when a clone fails halfway.
pub(crate) fn extract_zip_entries_to(destination: &Path, package_data: &PackageData) -> Result<(), Error> {
    let package_subpath
        = package_data.package_subpath();

    let package_bytes = match package_data {
        PackageData::Zip {archive_path, ..} => archive_path.fs_read()?,
        _ => panic!("Expected a zip archive"),
    };

    let entries
        = zpm_formats::zip::entries_from_zip(&package_bytes)?
            .into_iter()
            .strip_path_prefix(&package_subpath)
            .collect::<Vec<_>>();

    for entry in entries {
        let target_path = destination
            .with_join(&entry.name);

        target_path.fs_create_parent()?;

        target_path
            .fs_write(&entry.data)?
            .fs_set_mode(entry.mode as u32)?;
    }

    Ok(())
}

/// Whether copy-on-write clones work between the unpacked store and
/// the project. Probed once per process by cloning a marker file; any
/// failure (other platform, other filesystem, cross-volume setup)
/// simply disables clone-based materialization.
pub fn clonefile_supported(store_root: &Path, project_cwd: &Path) -> bool {
    use std::sync::OnceLock;

    static SUPPORTED: OnceLock<bool> = OnceLock::new();

    // `fs_clonefile` always fails elsewhere; no need to pay for the probe
    if !cfg!(target_os = "macos") {
        return false;
    }

    *SUPPORTED.get_or_init(|| {
        let probe = || -> Result<(), Error> {
            store_root.fs_create_dir_all()?;

            let source = store_root
                .with_join_str(format!(".clone-probe-{}", std::process::id()));
            let target = project_cwd
                .with_join_str(format!(".clone-probe-{}", std::process::id()));

            source.fs_write([])?;

            if target.fs_exists() {
                let _ = target.fs_rm_file();
            }

            let cloned = source.fs_clonefile(&target);

            let _ = source.fs_rm_file().ok_missing();
            let _ = target.fs_rm_file().ok_missing();

            cloned?;

            Ok(())
        };

        probe().is_ok()
    })
}

/// Ensures the unpacked store holds a pristine copy of the package,
/// keyed by locator and content checksum, and returns its path. The
/// entry is built in a temp folder and renamed in place so concurrent
/// installs can race safely.
pub fn ensure_unpacked_store_entry(
    store_root: &Path,
    locator: &Locator,
    checksum: &zpm_utils::Hash64,
    package_data: &PackageData,
) -> Result<Path, Error> {
    let entry_path = store_root.with_join_str(format!(
        "{}-{}-{}",
        locator.ident.slug(),
        locator.reference.slug(),
        checksum.short(),
    ));

    if entry_path.fs_exists() {
        return Ok(entry_path);
    }

    // The nonce keeps parallel extractions of the same package (the
    // tree may materialize one locator at several locations) from
    // clobbering each other's temp folder.
    static STORE_TMP_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let tmp_path = store_root.with_join_str(format!(
        ".tmp-{}-{}-{}",
        std::process::id(),
        STORE_TMP_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        checksum.short(),
    ));

    // A crashed run can leave this exact path behind (pids and the
    // nonce both reset, typically in containers); extracting over its
    // leftovers would graft stale files into an immutable store entry.
    // (Checked first, as this runs for every package and the path is
    // normally missing)
    if tmp_path.fs_exists() {
        let _ = tmp_path.fs_rm();
    }

    tmp_path.fs_create_dir_all()?;

    if let Err(error) = extract_zip_entries_to(&tmp_path, package_data) {
        let _ = tmp_path.fs_rm().ok_missing();
        return Err(error);
    }

    if let Err(error) = tmp_path.fs_rename(&entry_path) {
        let _ = tmp_path.fs_rm().ok_missing();

        // Another process finished the same entry first; use theirs.
        if !entry_path.fs_exists() {
            return Err(error.into());
        }
    }

    Ok(entry_path)
}

/// Materializes `destination` as a copy-on-write clone of the store
/// entry, replacing whatever was there.
pub fn clone_package_from_store(destination: &Path, store_entry: &Path) -> Result<(), Error> {
    if destination.fs_exists() {
        destination.fs_rm()?;
    }

    destination.fs_create_parent()?;
    store_entry.fs_clonefile(destination)?;

    Ok(())
}

/// Stamped on every CAS entry. An mtime that doesn't match flags
/// out-of-band modification and triggers a repair on the next install.
const CAS_SAFE_TIME_SECS: i64 = 456_789_000;

const CAS_DEFAULT_MODE: u32 = 0o644;

fn link_into_cas(target_path: &Path, data: &[u8], mode: u32, index_root: &Path) -> Result<LinkOutcome, Error> {
    use sha1::{Digest, Sha1};

    let mode_bits = mode & 0o777;

    let hash = {
        let mut hasher = Sha1::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    };

    let index_filename = if mode_bits == CAS_DEFAULT_MODE {
        format!("{}.dat", hash)
    } else {
        format!("{}{:o}.dat", hash, mode_bits)
    };

    let index_dir = index_root
        .with_join_str(&hash[..2]);
    let index_path = index_dir
        .with_join_str(&index_filename);

    index_dir.fs_create_dir_all()?;

    // Serialize on the content hash so parallel extractions of the
    // same file can't interleave their writes to the index entry.
    let _shard_guard = cas_shard_lock(u8::from_str_radix(&hash[..2], 16).unwrap_or(0))
        .lock()
        .unwrap();

    let mut needs_rewrite = !index_path.fs_exists();

    if !needs_rewrite {
        // mtime != SAFE_TIME ⇒ external write since the last install.
        if let Ok(metadata) = index_path.fs_metadata() {
            let mtime_secs = metadata.modified().ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64);

            if mtime_secs != Some(CAS_SAFE_TIME_SECS) {
                needs_rewrite = true;
            }
        }
    }

    if needs_rewrite {
        // Write through the existing path: cross-project hardlinks
        // inherit the repair without losing inode identity.
        index_path.fs_write(data)?;
        index_path.fs_set_mode(mode_bits)?;
        set_safe_mtime(&index_path)?;
    }

    ensure_hardlink(target_path, &index_path, data)
}

fn set_safe_mtime(path: &Path) -> Result<(), Error> {
    let safe_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(CAS_SAFE_TIME_SECS as u64);

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path.to_path_buf())
        .map_err(zpm_utils::PathError::from)?;
    file.set_modified(safe_time).map_err(zpm_utils::PathError::from)?;
    Ok(())
}

pub fn populate_build_entry_dependencies(package_build_entries: &BTreeMap<Locator, usize>, locator_resolutions: &BTreeMap<Locator, Resolution>, descriptor_to_locator: &BTreeMap<Descriptor, Locator>) -> Result<BTreeMap<usize, BTreeSet<usize>>, Error> {
    let mut package_build_dependencies
        = BTreeMap::new();

    for locator in package_build_entries.keys() {
        let mut build_dependencies
            = BTreeSet::new();

        let mut queue
            = vec![locator.clone()];
        let mut seen
            = BTreeSet::new();

        seen.insert(locator.clone());

        while let Some(locator) = queue.pop() {
            let resolution = locator_resolutions.get(&locator)
                .expect("Failed to find locator resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = descriptor_to_locator.get(dependency)
                    .expect("Failed to find dependency locator");

                if !seen.insert(dependency_locator.clone()) {
                    continue;
                }

                let dependency_physical_locator
                    = dependency_locator.physical_locator();

                if let Some(dependency_entry_idx) = package_build_entries.get(&dependency_physical_locator) {
                    build_dependencies.insert(*dependency_entry_idx);
                }

                queue.push(dependency_locator.clone());
            }
        }

        let entry_idx = package_build_entries.get(locator)
            .expect("Failed to find build entry index");

        package_build_dependencies.insert(*entry_idx, build_dependencies);
    }

    Ok(package_build_dependencies)
}
pub struct PackageBuildInfo {
    pub must_extract: bool,
    pub build_step: build::BuildStep,
}

pub fn get_package_internal_info(project: &Project, install: &Install, dependencies_meta: &Vec<(VersionFilter, PackageMeta)>, locator: &Locator, resolution: &Resolution, physical_package_data: &PackageData) -> PackageBuildInfo {
    // The package meta is based on the top-level configuration extracted
    // from the `dependenciesMeta` field.
    //
    let package_ident = match &locator.reference {
        Reference::Registry(params) => &params.ident,
        _ => &locator.ident,
    };

    let package_meta
        = dependencies_meta.iter()
            .find(|(selector, _)| selector.check(package_ident, &resolution.version))
            .map(|(_, meta)| meta)
            .cloned()
            .unwrap_or_default();

    // The package flags are based on the actual package content. The flags
    // should always be the same for the same package, so we keep them in
    // the install state so we don't have to recompute them at every install.
    //
    let package_flags = &install.install_state.content_flags
        .get(&locator.physical_locator())
        .expect("Expected package flags to be set");

    // We don't take into account `is_compatible` here, as it may change
    // depending on the system and we don't want the paths encoded in the
    // .pnp.cjs file to change depending on the system.
    let has_build_commands = package_flags.build_commands.len() > 0;
    let scripts_allowed_by_meta = package_meta.built.unwrap_or(project.config.settings.enable_scripts.value);
    let scripts_can_run
        = locator.reference.is_workspace_reference() || scripts_allowed_by_meta;
    let should_build_if_compatible
        = has_build_commands && scripts_can_run;

    // Optional dependencies baked by zip archives are always extracted,
    // as we have no way to know whether they would be extracted if we
    // were to download them (this may change depending on the package's
    // files).
    let is_optional
        = install.install_state.resolution_tree.optional_builds.contains(locator);

    let is_baked_by_zip
        = matches!(physical_package_data, PackageData::Zip {..} | PackageData::MissingZip {..});

    let must_extract =
        (is_optional && is_baked_by_zip) || package_meta.unplugged.or(package_flags.prefer_extracted)
            .unwrap_or_else(|| should_build_if_compatible || package_flags.suggest_extracted);

    // We don't need to run the build if the package was marked as
    // incompatible with the current system (even if the package isn't
    // marked as optional).
    let is_compatible = resolution.requirements
        .validate_system(&System::from_current());

    let build_step = if !has_build_commands {
        build::BuildStep::Commands(Vec::new())
    } else if !is_compatible {
        build::BuildStep::Skip(build::BuildSkip::Incompatible)
    } else if !scripts_can_run {
        build::BuildStep::Skip(build::BuildSkip::Disabled)
    } else {
        build::BuildStep::Commands(package_flags.build_commands.clone())
    };

    PackageBuildInfo {
        must_extract,
        build_step,
    }
}

/// The part of the tree the main (node) linkers are responsible for: venv islands
/// link their own packages (wheels aren't npm packages), so neither their
/// workspaces nor what they depend on belong in node_modules.
pub fn main_linker_tree(install: &Install) -> std::borrow::Cow<'_, ResolutionTree> {
    let tree
        = &install.install_state.resolution_tree;

    let venv_island_ids = install.resolved_islands.iter()
        .filter(|island| island.linker == zpm_config::IslandLinker::Venv)
        .map(|island| island.id.as_str())
        .collect::<BTreeSet<_>>();

    // PyPI releases without an artifact for this platform (pywin32 on
    // macOS) stay in the tree as abstract packages so that the marker edges
    // leading to them resolve; there's nothing to link
    let has_abstract_packages = tree.locator_resolutions.keys()
        .any(|locator| matches!(install.package_data.get(&locator.physical_locator()), Some(PackageData::Abstract)));

    if venv_island_ids.is_empty() && !has_abstract_packages {
        return std::borrow::Cow::Borrowed(tree);
    }

    let venv_workspaces = install.resolved_islands.iter()
        .filter(|island| venv_island_ids.contains(island.id.as_str()))
        .flat_map(|island| island.workspace_idents.iter())
        .collect::<BTreeSet<_>>();

    // Keep what the node workspaces can reach: island packages can be
    // shared with them (a pypi: dependency of a regular workspace resolves to
    // the same locator), so subtracting the island's locators isn't enough
    let mut filtered
        = ResolutionTree::default();

    filtered.optional_builds = tree.optional_builds.clone();
    filtered.roots = tree.roots.iter()
        .filter(|descriptor| !venv_workspaces.contains(&descriptor.ident))
        .cloned()
        .collect();

    // Every workspace outside venv islands is linked, so they all seed the
    // walk (the root workspace isn't listed in the roots)
    let mut queue
        = tree.locator_resolutions.keys()
            .filter(|locator| locator.reference.is_workspace_reference() && !venv_workspaces.contains(&locator.ident))
            .cloned()
            .collect::<Vec<_>>();

    while let Some(locator) = queue.pop() {
        if filtered.locator_resolutions.contains_key(&locator) {
            continue;
        }

        let Some(resolution) = tree.locator_resolutions.get(&locator) else {
            continue;
        };

        let mut resolution
            = resolution.clone();

        resolution.dependencies.retain(|_, descriptor| {
            let Some(dependency_locator) = tree.descriptor_to_locator.get(descriptor) else {
                return true;
            };

            !matches!(install.package_data.get(&dependency_locator.physical_locator()), Some(PackageData::Abstract))
        });

        for descriptor in resolution.dependencies.values() {
            if let Some(dependency_locator) = tree.descriptor_to_locator.get(descriptor) {
                filtered.descriptor_to_locator.insert(descriptor.clone(), dependency_locator.clone());
                queue.push(dependency_locator.clone());
            }
        }

        filtered.locator_resolutions.insert(locator, resolution);
    }

    for descriptor in &filtered.roots {
        if let Some(locator) = tree.descriptor_to_locator.get(descriptor) {
            filtered.descriptor_to_locator.insert(descriptor.clone(), locator.clone());
        }
    }

    std::borrow::Cow::Owned(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing_link(kind: std::io::ErrorKind) -> impl Fn(&Path, &Path) -> std::io::Result<()> {
        move |_, _| Err(std::io::Error::from(kind))
    }

    fn real_link(source: &Path, target: &Path) -> std::io::Result<()> {
        std::fs::hard_link(source.to_path_buf(), target.to_path_buf())
    }

    fn real_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from.to_path_buf(), to.to_path_buf())
    }

    fn ensure_with_link(target: &Path, source: &Path, data: &[u8], link: FsOp<'_>) -> Result<LinkOutcome, Error> {
        ensure_hardlink_with(target, source, data, link, &real_rename)
    }

    fn probe_files(dir: &Path) -> Vec<String> {
        dir.fs_read_dir().unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".link-probe"))
            .collect()
    }

    fn setup() -> (Path, Path, Path) {
        let dir
            = Path::temp_dir().unwrap();

        let source = dir
            .with_join_str("source.dat");
        let target = dir
            .with_join_str("pkg/index.js");

        source
            .fs_write("hello")
            .unwrap()
            .fs_set_mode(0o755)
            .unwrap();

        target.fs_create_parent().unwrap();

        (dir, source, target)
    }

    #[test]
    fn classifies_link_errors() {
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from(std::io::ErrorKind::CrossesDevices)), Some(CopyFallbackReason::CrossDevice));
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from(std::io::ErrorKind::TooManyLinks)), Some(CopyFallbackReason::LinkLimit));
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)), None);
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from(std::io::ErrorKind::NotFound)), None);
    }

    #[cfg(unix)]
    #[test]
    fn classifies_unix_link_errors() {
        // EXDEV and EMLINK have the same values on Linux and macOS
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from_raw_os_error(18)), Some(CopyFallbackReason::CrossDevice));
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from_raw_os_error(31)), Some(CopyFallbackReason::LinkLimit));
    }

    #[cfg(windows)]
    #[test]
    fn classifies_windows_link_errors() {
        // ERROR_NOT_SAME_DEVICE and ERROR_TOO_MANY_LINKS
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from_raw_os_error(17)), Some(CopyFallbackReason::CrossDevice));
        assert_eq!(CopyFallbackReason::from_link_error(&std::io::Error::from_raw_os_error(1142)), Some(CopyFallbackReason::LinkLimit));
    }

    #[test]
    fn links_when_possible() {
        let (_dir, source, target)
            = setup();

        assert_eq!(ensure_with_link(&target, &source, b"hello", &real_link).unwrap(), LinkOutcome::Linked);
        assert!(is_same_file(&target, &source));
    }

    #[test]
    fn copies_across_devices() {
        let (_dir, source, target)
            = setup();

        let outcome
            = ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::CrossesDevices)).unwrap();

        assert_eq!(outcome, LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        assert!(!is_same_file(&target, &source));
        assert_eq!(target.fs_read_text().unwrap(), "hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(target.fs_metadata().unwrap().permissions().mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn copies_at_the_link_limit() {
        let (_dir, source, target)
            = setup();

        let outcome
            = ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::TooManyLinks)).unwrap();

        assert_eq!(outcome, LinkOutcome::Copied(CopyFallbackReason::LinkLimit));
        assert_eq!(target.fs_read_text().unwrap(), "hello");
    }

    #[test]
    fn keeps_up_to_date_copies() {
        let (dir, source, target)
            = setup();

        let cross_device
            = failing_link(std::io::ErrorKind::CrossesDevices);

        ensure_with_link(&target, &source, b"hello", &cross_device).unwrap();

        // A second name for the copy tells us whether the next pass
        // replaced the file rather than leaving it alone
        let witness = dir
            .with_join_str("witness");

        real_link(&target, &witness).unwrap();

        let attempts
            = std::sync::atomic::AtomicUsize::new(0);

        let outcome = ensure_with_link(&target, &source, b"hello", &|source, target| {
            attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            cross_device(source, target)
        }).unwrap();

        assert_eq!(outcome, LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        assert!(is_same_file(&target, &witness));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn repairs_modified_copies() {
        let (_dir, source, target)
            = setup();

        let cross_device
            = failing_link(std::io::ErrorKind::CrossesDevices);

        ensure_with_link(&target, &source, b"hello", &cross_device).unwrap();

        target.fs_write("HELLO").unwrap();

        assert_eq!(ensure_with_link(&target, &source, b"hello", &cross_device).unwrap(), LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        assert_eq!(target.fs_read_text().unwrap(), "hello");
    }

    #[test]
    fn replaces_outdated_copies() {
        let (dir, source, target)
            = setup();

        let cross_device
            = failing_link(std::io::ErrorKind::CrossesDevices);

        ensure_with_link(&target, &source, b"hello", &cross_device).unwrap();

        // Same size and permissions, different content (a package update)
        let updated_source = dir
            .with_join_str("updated.dat");

        updated_source
            .fs_write("world")
            .unwrap()
            .fs_set_mode(0o755)
            .unwrap();

        assert_eq!(ensure_with_link(&target, &updated_source, b"world", &cross_device).unwrap(), LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        assert_eq!(target.fs_read_text().unwrap(), "world");
    }

    #[test]
    fn relinks_copies_once_linking_works() {
        let (_dir, source, target)
            = setup();

        ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::CrossesDevices)).unwrap();

        assert_eq!(ensure_with_link(&target, &source, b"hello", &real_link).unwrap(), LinkOutcome::Linked);
        assert!(is_same_file(&target, &source));
        assert_eq!(target.dirname().unwrap().fs_read_dir().unwrap().count(), 1);
    }

    #[test]
    fn keeps_copies_when_the_probe_fails_with_another_error() {
        let (_dir, source, target)
            = setup();

        ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::CrossesDevices)).unwrap();

        // Leaves a file behind before failing, like a partial link would
        let outcome = ensure_with_link(&target, &source, b"hello", &|source, target| {
            real_link(source, target)?;
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }).unwrap();

        assert_eq!(outcome, LinkOutcome::Copied(CopyFallbackReason::ReplaceFailed));
        assert!(!is_same_file(&target, &source));
        assert_eq!(target.fs_read_text().unwrap(), "hello");
        assert!(probe_files(&target.dirname().unwrap()).is_empty());
    }

    #[test]
    fn keeps_copies_when_replacing_them_fails() {
        let (_dir, source, target)
            = setup();

        ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::CrossesDevices)).unwrap();

        // What Windows does when a process has the target open
        let outcome = ensure_hardlink_with(&target, &source, b"hello", &real_link, &|_, _| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }).unwrap();

        assert_eq!(outcome, LinkOutcome::Copied(CopyFallbackReason::ReplaceFailed));
        assert!(!is_same_file(&target, &source));
        assert_eq!(target.fs_read_text().unwrap(), "hello");
        assert!(probe_files(&target.dirname().unwrap()).is_empty());
    }

    #[test]
    fn clears_probes_left_by_interrupted_installs() {
        let (_dir, source, target)
            = setup();

        let cross_device
            = failing_link(std::io::ErrorKind::CrossesDevices);

        ensure_with_link(&target, &source, b"hello", &cross_device).unwrap();

        let stale_probe = target.dirname().unwrap()
            .with_join_str(".index.js.link-probe");

        real_link(&source, &stale_probe).unwrap();

        assert_eq!(ensure_with_link(&target, &source, b"hello", &cross_device).unwrap(), LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        assert!(probe_files(&target.dirname().unwrap()).is_empty());
    }

    #[test]
    fn fails_on_other_errors() {
        let (_dir, source, target)
            = setup();

        assert!(ensure_with_link(&target, &source, b"hello", &failing_link(std::io::ErrorKind::PermissionDenied)).is_err());
        assert!(!target.fs_exists());
    }

    #[test]
    fn warns_only_when_files_were_copied() {
        let store
            = Path::try_from("/store/index").unwrap();
        let project
            = Path::try_from("/project").unwrap();

        let fallbacks
            = CopyFallbacks::new();

        fallbacks.record(LinkOutcome::Linked);

        assert_eq!(fallbacks.warning(Some(&store), &project), None);

        fallbacks.record(LinkOutcome::Copied(CopyFallbackReason::CrossDevice));

        assert_eq!(
            fallbacks.warning(Some(&store), &project).unwrap(),
            format!("1 file was copied instead of hard-linked because the store at /store/index and the project at /project are on different filesystems; installs are slower and use more disk space as a result (see {})", COPY_FALLBACK_DOCS_URL),
        );

        fallbacks.record(LinkOutcome::Copied(CopyFallbackReason::CrossDevice));
        fallbacks.record(LinkOutcome::Copied(CopyFallbackReason::LinkLimit));

        assert_eq!(
            fallbacks.warning(None, &project).unwrap(),
            format!("3 files were copied instead of hard-linked (2 because parts of the project at /project are on different filesystems, 1 because their content reached the filesystem's maximum number of hard links per file); installs are slower and use more disk space as a result (see {})", COPY_FALLBACK_DOCS_URL),
        );

        let fallbacks
            = CopyFallbacks::new();

        fallbacks.record(LinkOutcome::Copied(CopyFallbackReason::ReplaceFailed));

        assert_eq!(
            fallbacks.warning(Some(&store), &project).unwrap(),
            format!("1 file was copied instead of hard-linked because existing copies couldn't be replaced by hard links; installs are slower and use more disk space as a result (see {})", COPY_FALLBACK_DOCS_URL),
        );
    }
}
