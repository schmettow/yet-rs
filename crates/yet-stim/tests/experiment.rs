// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use yet_stim::{
    load_experiment, Block, ConfigError, Experiment, ExperimentError, LoadError, Stimulus,
    StimulusId, StimulusKind,
};

const CONFIG: &str = r#"
[display]
presenter_monitor = 1
console_monitor = 0
width = 1920
height = 1080
fullscreen = true

[camera]
usb_index = 0

[presenter]
backend = "egui"
overlay = "auto"

[overlay]
monitor = 1

[audio]
enabled = true
device = "default"
volume = 0.8
"#;

const METADATA: &str = "key,value\nid,demo\nexperimenter,Martin\ninstructions,Look at the stimuli\nshuffle,true\nseed,42\n";

const STIMULI: &str = "id,kind,path,fps,duration_ms,loop,audio,block\n\
t01,image,stimuli/t01.png,,3000,false,,B1\n\
t02,image,stimuli/t02.png,,3000,false,,B1\n\
t03,sequence,stimuli/t03/,30,5000,false,,B2\n";

fn write_valid(root: &Path) {
    fs::write(root.join("config.toml"), CONFIG).unwrap();
    fs::write(root.join("Experiment.csv"), METADATA).unwrap();
    fs::write(root.join("Stimuli.csv"), STIMULI).unwrap();
    fs::create_dir_all(root.join("stimuli/t03")).unwrap();
    fs::write(root.join("stimuli/t01.png"), b"fixture").unwrap();
    fs::write(root.join("stimuli/t02.png"), b"fixture").unwrap();
    fs::write(root.join("stimuli/t03/frame_00001.png"), b"frame").unwrap();
}

fn valid_experiment() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_valid(dir.path());
    dir
}

#[test]
fn valid_experiment_loads_with_blocks_and_resolved_paths() {
    let dir = valid_experiment();
    let loaded = load_experiment(dir.path()).unwrap();

    assert_eq!(loaded.experiment.id, "demo");
    assert_eq!(loaded.experiment.experimenter.as_deref(), Some("Martin"));
    assert_eq!(loaded.experiment.blocks.len(), 2);
    assert_eq!(loaded.plan.trial_count(), 3);
    assert_eq!(loaded.plan.blocks[0].id.as_str(), "B1");
    assert_eq!(loaded.plan.blocks[0].trials.len(), 2);
    assert!(loaded.plan.blocks[0].trials.iter().all(|planned| planned
        .trial
        .stimulus
        .path
        .is_file()));
    assert!(loaded.plan.blocks[1].trials[0].trial.stimulus.path.is_dir());
    assert_eq!(loaded.experiment.seed, Some(42));
}

#[test]
fn the_same_seed_produces_the_same_order() {
    let dir = valid_experiment();
    let first = load_experiment(dir.path()).unwrap();
    let second = load_experiment(dir.path()).unwrap();

    assert_eq!(first.plan, second.plan);
}

#[test]
fn disabling_shuffle_preserves_csv_order() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Experiment.csv"),
        METADATA
            .replace("shuffle,true", "shuffle,false")
            .replace("seed,42\n", ""),
    )
    .unwrap();

    let loaded = load_experiment(dir.path()).unwrap();
    let ids: Vec<_> = loaded.plan.blocks[0]
        .trials
        .iter()
        .map(|planned| planned.trial.stimulus.id.as_str())
        .collect();

    assert_eq!(ids, vec!["t01", "t02"]);
}

#[test]
fn metadata_is_optional_and_defaults_to_csv_order() {
    let dir = valid_experiment();
    fs::remove_file(dir.path().join("Experiment.csv")).unwrap();

    let loaded = load_experiment(dir.path()).unwrap();

    assert_eq!(loaded.experiment.id, "unnamed");
    assert!(!loaded.experiment.shuffle);
    assert_eq!(loaded.plan.trial_count(), 3);
}

#[test]
fn missing_config_is_a_config_io_error() {
    let dir = valid_experiment();
    fs::remove_file(dir.path().join("config.toml")).unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(error, LoadError::Config(ConfigError::Io { .. })));
}

#[test]
fn missing_stimuli_file_is_an_experiment_io_error() {
    let dir = valid_experiment();
    fs::remove_file(dir.path().join("Stimuli.csv")).unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::Io { .. })
    ));
}

#[test]
fn missing_image_is_reported_with_id_and_path() {
    let dir = valid_experiment();
    fs::remove_file(dir.path().join("stimuli/t01.png")).unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    match error {
        LoadError::Experiment(ExperimentError::MissingMedia { id, path }) => {
            assert_eq!(id.as_str(), "t01");
            assert!(path.ends_with("stimuli/t01.png"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn invalid_duration_is_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace(
            "t01,image,stimuli/t01.png,,3000",
            "t01,image,stimuli/t01.png,,abc",
        ),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::InvalidDuration { .. })
    ));
}

#[test]
fn duplicate_stimulus_ids_are_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace(
            "t02,image,stimuli/t02.png,,3000,false,,B1",
            "t01,image,stimuli/t01.png,,3000,false,,B1",
        ),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::DuplicateStimulusId { .. })
    ));
}

#[test]
fn images_may_not_have_audio() {
    let dir = valid_experiment();
    fs::write(dir.path().join("stimuli/audio.wav"), b"audio").unwrap();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace(
            "t01,image,stimuli/t01.png,,3000,false,,",
            "t01,image,stimuli/t01.png,,3000,false,stimuli/audio.wav,",
        ),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::ImageWithAudio { .. })
    ));
}

#[test]
fn sequences_require_fps() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace("t03,sequence,stimuli/t03/,30", "t03,sequence,stimuli/t03/,"),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::MissingFps { .. })
    ));
}

#[test]
fn invalid_loop_value_is_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace(
            "t01,image,stimuli/t01.png,,3000,false",
            "t01,image,stimuli/t01.png,,3000,maybe",
        ),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::InvalidLoop { .. })
    ));
}

#[test]
fn unknown_kind_is_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Stimuli.csv"),
        STIMULI.replace("t01,image", "t01,video"),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::InvalidKind { .. })
    ));
}

#[test]
fn missing_block_column_uses_the_default_block() {
    let dir = valid_experiment();
    let without_block = STIMULI
        .replace(",block", "")
        .replace(",B1", "")
        .replace(",B2", "");
    fs::write(dir.path().join("Stimuli.csv"), without_block).unwrap();

    let loaded = load_experiment(dir.path()).unwrap();

    assert_eq!(loaded.plan.blocks.len(), 1);
    assert_eq!(loaded.plan.blocks[0].id.as_str(), "default");
    assert_eq!(loaded.plan.trial_count(), 3);
}

#[test]
fn an_experiment_without_blocks_is_rejected() {
    let experiment = Experiment {
        id: "empty".to_owned(),
        experimenter: None,
        instructions: None,
        blocks: Vec::new(),
        shuffle: false,
        seed: None,
    };

    assert!(matches!(
        experiment.validate(),
        Err(ExperimentError::EmptyExperiment)
    ));
}

#[test]
fn empty_blocks_are_rejected() {
    let block = Block::new("empty", Vec::new());

    assert!(matches!(
        block.validate(),
        Err(ExperimentError::EmptyBlock { id }) if id.as_str() == "empty"
    ));
}

#[test]
fn shuffle_requires_a_seed() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Experiment.csv"),
        METADATA.replace("seed,42\n", ""),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::MissingSeed)
    ));
}

#[test]
fn unknown_metadata_keys_are_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Experiment.csv"),
        format!("{METADATA}typo,value\n"),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::UnknownMetadataKey { .. })
    ));
}

#[test]
fn duplicate_metadata_keys_are_rejected() {
    let dir = valid_experiment();
    fs::write(
        dir.path().join("Experiment.csv"),
        format!("{METADATA}id,again\n"),
    )
    .unwrap();

    let error = load_experiment(dir.path()).unwrap_err();

    assert!(matches!(
        error,
        LoadError::Experiment(ExperimentError::DuplicateMetadataKey { .. })
    ));
}

#[test]
fn plan_display_contains_seed_blocks_and_stimuli() {
    let dir = valid_experiment();
    let loaded = load_experiment(dir.path()).unwrap();
    let text = loaded.plan.to_string();

    assert!(text.contains("Experiment: demo"));
    assert!(text.contains("seed 42"));
    assert!(text.contains("Block B1"));
    assert!(text.contains("t01"));
    assert!(text.contains("sequence"));
}

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

#[test]
fn stimulus_validation_rejects_image_audio_without_io() {
    let stimulus = Stimulus {
        id: StimulusId::new("s"),
        kind: StimulusKind::Image,
        path: PathBuf::from("unused.png"),
        duration: std::time::Duration::from_millis(100),
        fps: None,
        looped: false,
        audio: Some(PathBuf::from("unused.wav")),
    };

    assert!(matches!(
        stimulus.validate(),
        Err(ExperimentError::ImageWithAudio { .. })
    ));
}

#[test]
fn sequence_validation_requires_positive_fps() {
    let stimulus = Stimulus {
        id: StimulusId::new("s"),
        kind: StimulusKind::Sequence,
        path: PathBuf::from("unused/"),
        duration: std::time::Duration::from_millis(100),
        fps: Some(0.0),
        looped: false,
        audio: None,
    };

    assert!(matches!(
        stimulus.validate(),
        Err(ExperimentError::InvalidFps { .. })
    ));
}
