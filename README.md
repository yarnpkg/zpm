# zpm-prototype

This repository is a prototype of a Rust-powered package manager heavily inspired from Yarn Berry.

It's a work in progress and is not meant to be used in production.

## Differences in architecture

### Many things are global to the process

The Berry codebase used `Configuration` and `Project` to store most information about the process and the project. It made it theoretically possible to instantiate multiple projects in the same process, but in practice this was rarely used.

In zpm I tried a different approach where everything is global to the process and lazily computed. For instance, the `project::root()` function will return the path to the project root based on the current working directory and cache it for the remainder of the process. Similarly, `project::lockfile()` will parse the lockfile relative to `project::root()` and cache the result.

### JSON lockfile

The Berry lockfile was written in Yaml. Since performances are a heavy focus of zpm, I decided to switch to JSON for the lockfile. This allows us to use `serde_json` which is much faster than `serde_yaml`. Some improvements would be useful to decrease the risks of conflicts (namely by adding blank lines between each lockfile record), but it doesn't require Yaml.

### No plugins

This implementation doesn't currently support plugins. It's a significant departure from Berry, and I'm not sure whether it'll remain that way or not - it was implemented this way to make it easier to incrementally build this prototype, not because of an overarching design decision.
