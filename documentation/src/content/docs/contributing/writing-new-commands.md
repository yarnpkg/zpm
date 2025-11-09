---
category: contributing
slug: contributing/commands
title: Writing new commands
description: Learn how to write new commands for Yarn.
sidebar:
  order: 5
---

Yarn has used a library called Clipanion to power its CLI ever since the Berry codebase. Where other frameworks tend to either use functional dedicated APIs to declare their commands, Clipanion attempts to provide a more intuitive and user-friendly experience, while remaining highly integrated with TypeScript.

The same is true in the ZPM codebase, as we ported Clipanion over to Rust (repository [here](https://github.com/arcanis/clipanion-rs)). The syntax is similar to the TypeScript implementation of Clipanion, with some twists and new capabilities.

## Example

```rs
use clipanion::cli;

#[cli::command]
#[cli::path("commit")]
#[cli::category("Miscellaneous commands")]
struct MyCommand {
  #[cli::option("-v,--verbose")]
  verbose: bool,

  #[cli::option("-m,--message")]
  message: Option<String>,

  all: Vec<String>,
}

impl MyCommand {
  fn run(&self) -> Result<(), String> {
    // ...
    Ok(())
  }
}
```

## Typed parameters

Clipanion supports typed parameters out of the box. Any type that implements the `FromStr` trait can be used as a parameter. This is for example the case of the `Ident` / `Descriptor` / `Locator` types:

```rs
#[cli::command]
#[cli::path("add")]
struct AddCommand {
  packages: Vec<Ident>,
}
```

## Documentation

Clipanion will leverage Rust [doc comments](https://doc.rust-lang.org/rust-by-example/meta/doc.html#doc-comments) to generate documentation for each command. The documentation will be displayed in the help output of the CLI:

```rs
/// Commit changes to the repository.
///
/// This command will commit all changes to the repository.
#[cli::command]
#[cli::path("commit")]
#[cli::category("Miscellaneous commands")]
struct CommitCommand {
  /// Verbose mode.
  #[cli::option("-v,--verbose")]
  verbose: bool,

  /// Commit message.
  #[cli::option("-m,--message")]
  message: Option<String>,

  /// Files to commit.
  all: Vec<String>,
}
```
