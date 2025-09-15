# zpm-prototype

This repository is a prototype of a Rust-powered package manager heavily inspired from Yarn Berry.

It's a work in progress and is not meant to be used in production.

## Usage

1. Make sure you have Yarn Switch installed.

```bash
curl -s https://repo.yarnpkg.com/install | bash
```

2. Clone both this repository and the `berry` repository next to each other.

```bash
git clone https://github.com/yarnpkg/zpm.git
git clone https://github.com/yarnpkg/berry.git
```

3. Move into the `zpm` directory, then build the project. We build in release mode to reproduce as closely as possible the performances in which zpm will be used. Rust is known to be significantly slower in debug mode.

```bash
cargo build --release -p zpm-switch -p zpm
```

4. (optional) Configure Yarn Switch to use your local binary when working on this repository.

```bash
yarn switch link target/release/yarn-bin
```

5. Run the tests.

```bash
yarn berry test:integration
```

> [!NOTE]
> You can set the `BERRY_DIR` environment variable to a pre-existing clone of the `berry` repository to avoid cloning it again.

## Differences in architecture

### Yarn Switch

After the failure of Corepack, I decided to implement a Yarn-specific jumper called Yarn Switch. Its source code lives in the `zpm-switch` crate and is distributed as the `yarn` binary (the true standard Yarn binary is called `yarn-bin`).

Yarn Switch uses the `packageManager` field just like Corepack, but doesn't support package managers other than Yarn. It supports local installations but only when set through the `YARNSW_DEFAULT` environment variable. This is an attempt to avoid running binaries from the repository itself.

> [!NOTE]
> TODO: Revamp that to instead allow Yarn Switch to track a list of folders that should use local paths. Also rename the `local:` protocol into the more classic `file:` protocol.

### Installing Yarn ZPM

With the switch to Yarn Switch, the installation instructions are now different. Users who wish to install Yarn will now have to run:

```bash
curl -s https://repo.yarnpkg.com/install | bash
```

This url returns the content of the `install-script.sh` file in the main branch of the repository and will install Yarn Switch in `~/.yarn/switch/bin`. It'll then run `yarn switch postinstall` which will perform some additional setup to add the aforementioned folder into the `PATH` environment variable.

### Redesigned steps

The Berry codebase uses a fairly sequential architecture: resolution, then fetching, then linking. The zpm codebase, on the other hand, interlaces the resolution and the fetching. There are a few reasons for this:

- Various non-semver protocols require fetching to be able to resolve the dependencies (git dependencies, `file:` dependencies), so in practice even with separate steps we need a way to call one step from the other.

- Rust doesn't have great and efficient primitives to handle mutating a single store from multiple places (in practice we'd have to use `Arc<Mutex<Store>>` or something similar, but that kills some of the benefits of running the fetch in parallel).

- One of the goals of the project is to make commands as fast as we can. By interlacing the resolution and the fetching, we can start fetching the first package as soon as we know we need it, rather than waiting for the resolution to be done.

### Serialization protocol

I wasn't satisfied with the `Display` and `Debug` traits, as they don't differentiate output intended for humans from output intended for serialization format (`Display` is arguably for humans, but `Debug` most certainly isn't intended for serialization).

> [!NOTE]
> I could have used the `Serialize` and `Deserialize` traits from `serde`, but if I remember correctly I was thinking that some data structures may want to be serialized / deserialized differently when targeting a file vs when targeting a string (typically a command-line argument).

To address that, I created three different traits:

- `ToHumanString` is meant to be used when printing things on the screen.
- `ToFileString` is meant to be used when writing something to a file.
- `FromFileString` is meant to be used when reading something from a file.

> [!NOTE]
> The `ToHumanString` trait used to also derive into a `Display` implementation, but it was far too easy to accidentally call the wrong trait when passing a data structure to `format!`. To avoid that I removed the `Display` implementation, and callers now must explicitly decide whether they want to use `ToHumanString` or `ToFileString` in format strings.
>
> It'd be nice if we could have `format!`-like macros that use either trait depending on the context (`format_for_screen!` and `format_for_file!`), but I didn't find a clean way to do that.

### Settings

Settings are split into three categories: env settings, user settings, and project settings.

- **Env settings** are settings that can *only* be set through environment variables. Very few settings are of this type. We only really use them for testing and debug purposes.

- **User settings** are settings that users shouldn't check-in because they depend either on personal preferences (`enable_progress_bars`) or on the system they're running on (`http_retry`). They must be defined through the `.yarnrc.yml` file in the user's home directory

- **Project settings** are settings that are specific to a project. That's where most of our settings are stored. They're stored in the `.yarnrc.yml` file in the root of the project.

To make maintaining the settings easier (especially when dealing with default values, environment variables, etc), I created a `yarn_config` macro that's used to define the settings.

### Enumerations

I wrote a special macro to define enumerations. Its main features are:

- Adds support for parsing variants from strings through regexes.
  - Supports capture groups, which are assigned into the corresponding struct fields.
  - Validates that the captured groups *can* be turned into the corresponding type (through `FromStr`).

- Turn all variants into standalone structs.
  - For example, `enum Foo { Test { field: usize; } }` will be turned into `struct TestFoo { field: usize; }; enum Foo { Test(TestFoo) }`.

> [!NOTE]
> TODO: I didn't find a way to handle the serialization of the enum variants yet, but it'd be really nice to have. Something where we'd use the pattern specs in the opposite way, to generate text rather than parse it?

### JSON lockfile

The Berry lockfile was written in Yaml. Since performances are a heavy focus of zpm, I decided to switch to JSON for the lockfile. This allows us to use `sonic_rs` or `serde_json`, which are both much faster than `serde_yaml`.

Some improvements to the output format would be useful to decrease risks of conflicts when merging branches together, in particular by adding blank lines between each lockfile record, but we don't require Yaml for that.

### Zip files

Since ZPM aims to download dependencies as they are needed when running scripts, less emphasis is put on zero-installs. As a result I didn't implement compression support when generating zip archives: all files are stored uncompressed.

### Edit-in-place JSON/YAML

I implemented some modules in `zpm_formats` to allow updating specific fields in JSON/YAML files without touching the surrounding fields and comments. It's very much a work in progress and I expect bugs to be present, or features to be missing.

### No plugins

This implementation doesn't currently support plugins. It's a significant departure from Berry, and I'm not sure whether it'll remain that way or not - it was implemented this way to make it easier to incrementally build this prototype, not because of an overarching design decision.
