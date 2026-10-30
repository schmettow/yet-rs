// SPDX-License-Identifier: GPL-3.0-only

//! Loading a `yet_zero` Python workspace unchanged.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use yet_stim::{load_experiment, LoadError, OverlayMode, StimulusKind, WorkspaceKind};

const CONFIG: &str = "\
Config,Value
USB,2
EXP_ID,Demo
EXPERIMENTER,MS
WIDTH,900
HEIGHT,900
SLIDE_TIME,2.5
STIM_FILE,Stimuli.csv
SHUFFLE,0
";

const STIMULI: &str = "\
File,width,height,hum_like,hum_skull,hum_eye,Face
18.jpg,450,450,38,0,1,18
92manipulated2.jpg,450,450,87,0,0,92
";

fn workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Stimuli")).unwrap();
    fs::write(dir.path().join("Config.csv"), CONFIG).unwrap();
    fs::write(dir.path().join("Stimuli/Stimuli.csv"), STIMULI).unwrap();
    fs::write(
        dir.path().join("Stimuli/18.jpg"),
        [0xff_u8, 0xd8, 0xff, 0xd9],
    )
    .unwrap();
    fs::write(
        dir.path().join("Stimuli/92manipulated2.jpg"),
        [0xff_u8, 0xd8, 0xff, 0xd9],
    )
    .unwrap();
    dir
}

fn load(root: &Path) -> yet_stim::LoadedExperiment {
    load_experiment(root).unwrap()
}

#[test]
fn a_python_workspace_loads_without_conversion() {
    let dir = workspace();

    let loaded = load(dir.path());

    assert_eq!(loaded.kind, WorkspaceKind::YetZeroPython);
    assert_eq!(loaded.config.display.width, 900);
    assert_eq!(loaded.config.display.height, 900);
    assert!(!loaded.config.display.fullscreen);
    assert_eq!(loaded.config.camera.usb_index, 2);
    assert_eq!(loaded.config.presenter.overlay, OverlayMode::Off);
    assert_eq!(loaded.experiment.id, "Demo");
    assert_eq!(loaded.experiment.experimenter.as_deref(), Some("MS"));
    assert!(!loaded.experiment.shuffle);
    assert_eq!(loaded.plan.trial_count(), 2);
    assert_eq!(loaded.plan.blocks.len(), 1);
    assert_eq!(loaded.plan.blocks[0].id.as_str(), "default");

    let first = &loaded.plan.blocks[0].trials[0].trial.stimulus;
    assert_eq!(first.id.as_str(), "18.jpg");
    assert_eq!(first.kind, StimulusKind::Image);
    assert_eq!(first.duration.as_millis(), 2500);
    assert_eq!(first.path, dir.path().join("Stimuli/18.jpg"));
    assert_eq!(loaded.result_dir, dir.path().join("Data"));
}

#[test]
fn stim_file_selects_the_short_table() {
    let dir = workspace();
    fs::write(
        dir.path().join("Stimuli/Stimuli_short.csv"),
        "File,width,height\n18.jpg,450,450\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Config.csv"),
        CONFIG.replace("Stimuli.csv", "Stimuli_short.csv"),
    )
    .unwrap();

    let loaded = load(dir.path());

    assert_eq!(loaded.plan.trial_count(), 1);
    assert_eq!(
        loaded.plan.blocks[0].trials[0].trial.stimulus.id.as_str(),
        "18.jpg"
    );
}

#[test]
fn an_explicit_seed_makes_the_python_shuffle_reproducible() {
    let dir = workspace();
    fs::write(
        dir.path().join("Config.csv"),
        format!("{CONFIG}SEED,7\n").replace("SHUFFLE,0", "SHUFFLE,1"),
    )
    .unwrap();

    let first = load(dir.path());
    let second = load(dir.path());

    assert!(first.experiment.shuffle);
    assert_eq!(first.experiment.seed, Some(7));
    assert_eq!(first.plan, second.plan);
}

#[test]
fn a_missing_stimulus_table_is_reported() {
    let dir = workspace();
    fs::remove_file(dir.path().join("Stimuli/Stimuli.csv")).unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(error, LoadError::Legacy(_)), "{error:?}");
}

#[test]
fn a_missing_image_is_reported_with_its_file_name_and_path() {
    let dir = workspace();
    fs::remove_file(dir.path().join("Stimuli/18.jpg")).unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    match error {
        LoadError::Legacy(yet_stim::legacy::LegacyError::Experiment(
            yet_stim::ExperimentError::MissingMedia { id, path },
        )) => {
            assert_eq!(id.as_str(), "18.jpg");
            assert!(path.ends_with("Stimuli/18.jpg"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn config_toml_overrides_machine_settings_but_keeps_the_csv_camera() {
    let dir = workspace();
    fs::write(
        dir.path().join("config.toml"),
        "\
[display]
presenter_monitor = 1
console_monitor = 0
width = 1440
height = 2560
fullscreen = true
",
    )
    .unwrap();

    let loaded = load(dir.path());

    assert_eq!(loaded.config.display.presenter_monitor, 1);
    assert_eq!(loaded.config.display.width, 1440);
    assert_eq!(loaded.config.camera.usb_index, 2);
}

#[test]
fn config_toml_camera_index_wins_when_present() {
    let dir = workspace();
    fs::write(
        dir.path().join("config.toml"),
        "\
[display]
presenter_monitor = 1
console_monitor = 0
width = 1440
height = 2560
fullscreen = true

[camera]
usb_index = 5
",
    )
    .unwrap();

    let loaded = load(dir.path());

    assert_eq!(loaded.config.camera.usb_index, 5);
}

#[test]
fn a_directory_that_is_neither_layout_is_rejected() {
    let dir = tempfile::tempdir().unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(error, LoadError::UnrecognizedWorkspace { .. }));
}
