#![deny(unused_crate_dependencies)]

pub mod build;
pub mod cache;
pub mod commands;
pub mod config;
pub mod error;
pub mod fetcher;
pub mod formats;
pub mod git;
pub mod hash;
pub mod http;
pub mod install;
pub mod linker;
pub mod lockfile;
pub mod manifest;
pub mod misc;
pub mod pack;
pub mod path;
pub mod primitives;
pub mod project;
pub mod resolver;
pub mod script;
pub mod semver;
pub mod serialize;
pub mod settings;
pub mod system;
pub mod tree_resolver;
