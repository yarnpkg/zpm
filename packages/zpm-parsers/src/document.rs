use crate::{Error, Path, Value};

pub trait Document {
    fn update_path(&mut self, path: &Path, value: Value) -> Result<(), Error>;
    fn set_path(&mut self, path: &Path, value: Value) -> Result<(), Error>;
}
