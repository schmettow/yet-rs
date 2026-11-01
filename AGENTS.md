# Agent notes — yet-rs

## Release versioning

- A minor bump (`0.x+1.0`, e.g. `0.1.0` → `0.2.0`) is always a **workspace-level**
  release. Bump `[workspace.package] version` and all crates that inherit it.
  Use it whenever more than one crate changed.
- A solo crate change is a **patch** bump (`0.x.y+1`, e.g. `0.1.0` → `0.1.1`).
  Set that crate's `version` explicitly in its `Cargo.toml` instead of inheriting
  `version.workspace`; it is decoupled from the workspace version and published
  on its own.
- Any change to a library crate (`yet-core`, `yet-stim`, `yet-egui`) makes the
  release a workspace minor, even when `yet-zero` is the main target.
- A change confined to `yet-zero` (source, README, or its changelog entries) is
  a `yet-zero` patch.
- Check the published versions on crates.io and the manifests before choosing;
  do not assume the previous release plan still holds. Publish dependencies
  before dependents: `yet-core` → `yet-stim` → `yet-egui` → `yet-zero`.
