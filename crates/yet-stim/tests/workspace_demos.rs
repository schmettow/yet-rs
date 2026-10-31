// SPDX-License-Identifier: GPL-3.0-only

//! Repository-level checks for the checked-in demo workspaces.
//!
//! These tests read `../../examples`, which is not part of the published
//! `yet-stim` package. `Cargo.toml` excludes this file from packaging; the
//! workspace test run still executes it.

use std::path::PathBuf;

use yet_stim::{load_experiment, StimulusKind};

#[test]
fn the_repository_demo_experiment_stays_valid() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let loaded = load_experiment(root).unwrap();

    assert_eq!(loaded.plan.blocks.len(), 2);
    assert_eq!(loaded.plan.trial_count(), 5);
}

#[test]
fn the_image_demo_for_the_console_stays_valid() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo-gui");
    let loaded = load_experiment(root).unwrap();

    assert_eq!(loaded.plan.blocks.len(), 1);
    assert_eq!(loaded.plan.trial_count(), 4);
    assert!(loaded
        .experiment
        .blocks
        .iter()
        .flat_map(|block| &block.trials)
        .all(|trial| trial.stimulus.kind == StimulusKind::Image));
}
