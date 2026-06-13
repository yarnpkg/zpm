use std::collections::BTreeMap;

use clipanion::cli;
use zpm_parsers::JsonDocument;
use zpm_sync::{SyncItem, SyncTree};
use zpm_utils::Path;

use crate::error::Error;

/// Preview filesystem sync operations
///
/// This debug command reads a JSON sync definition, applies it to the destination tree in memory, and prints the operations that would be run.
///
#[cli::command]
#[cli::path("debug", "sync-fs")]
pub struct SyncFs {
    /// Destination directory for the sync plan
    destination: Path,

    /// JSON file containing the sync definition
    definition_file: Path,
}

impl SyncFs {
    pub async fn execute(&self) -> Result<(), Error> {
        let definition_file_contents
            = self.definition_file.fs_read_text()?;

        let definition: BTreeMap<Path, SyncItem<'_>>
            = JsonDocument::hydrate_from_str(&definition_file_contents)?;

        let mut sync_tree
            = SyncTree::new();

        for (path, item) in definition {
            sync_tree.register_entry(path, item)?;
        }

        let ops
            = sync_tree.run(self.destination.fs_canonicalize()?)?;

        for op in ops {
            println!("{}", op);
        }

        Ok(())
    }
}
