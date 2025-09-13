use clipanion::cli;
use zpm_utils::{tree, AbstractValue, IoResultExt, Path, TimeAgo};

use crate::{cache, errors::Error};

#[cli::command]
#[cli::path("switch", "cache")]
#[cli::category("Cache management")]
#[cli::description("List all cached Yarn binaries")]
#[derive(Debug)]
pub struct CacheListCommand {
}

impl CacheListCommand {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut nodes
            = vec![];

        let cache_dir
            = cache::cache_dir()?;

        let Some(cache_entries) = cache_dir.fs_read_dir().ok_missing()? else {
            return Ok(());
        };

        for entry in cache_entries {
            let entry
                = entry?;

            let entry_path
                = Path::try_from(entry.path())?;
            let entry_meta
                = cache::cache_metadata(&entry_path);
            let entry_age
                = cache::cache_last_used(&entry_path);

            let Ok(entry_meta) = entry_meta else {
                continue;
            };

            let Ok(entry_age) = entry_age else {
                continue;
            };

            nodes.push(tree::Node {
                label: None,
                value: Some(AbstractValue::new(entry_meta.version)),
                children: Some(tree::TreeNodeChildren::Map(tree::Map::from([
                    ("path".to_string(), tree::Node {
                        label: Some("Path".to_string()),
                        value: Some(AbstractValue::new(entry_path)),
                        children: None,
                    }),
                    ("age".to_string(), tree::Node {
                        label: Some("Age".to_string()),
                        value: Some(AbstractValue::new(TimeAgo::new(entry_age.elapsed().unwrap()))),
                        children: None,
                    }),
                ]))),
            });
        }

        let root = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(nodes)),
        };

        print!("{}", root.to_string());

        Ok(())
    }
}
