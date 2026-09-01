use super::Ident;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Registry {
    None,
    Workspace(Ident),
    Npm(Ident),
    Pypi(Ident),
}
