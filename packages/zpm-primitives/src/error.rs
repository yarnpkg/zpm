#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Invalid ident ({0})")]
    InvalidIdent(String),

    #[error("Invalid range ({0})")]
    InvalidRange(String),

    #[error("Cannot convert range to peer range ({0})")]
    CannotConvertRangeToPeerRange(String),
}
