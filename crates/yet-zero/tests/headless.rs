// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::process::Command;

use tempfile::TempDir;
use yet_core::GazePoint;
use yet_stim::RecordingPresenter;
use yet_stim::{load_experiment, DataSink, GazeSample, SessionIdentity};
use yet_zero::{drive, run, HeadlessOptions, RunError};

const CONFIG: &str = r#"
[display]
presenter_monitor = 0
console_monitor = 0
width = 640
height = 480
fullscreen = true
"#;

const METADATA: &str = "key,value\nid,demo\nexperimenter,MS\nshuffle,false\n";

const STIMULI: &str = "id,kind,path,fps,duration_ms,loop,audio,block\n\
t01,image,stimuli/t01.png,,20,false,,B1\n\
t02,image,stimuli/t02.png,,20,false,,B1\n\
t03,image,stimuli/t03.png,,20,false,,B1\n\
t04,image,stimuli/t04.png,,20,false,,B2\n\
t05,image,stimuli/t05.png,,20,false,,B2\n\
t06,image,stimuli/t06.png,,20,false,,B2\n";

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.toml"), CONFIG).unwrap();
    fs::write(dir.path().join("Experiment.csv"), METADATA).unwrap();
    fs::write(dir.path().join("Stimuli.csv"), STIMULI).unwrap();
    fs::create_dir_all(dir.path().join("stimuli")).unwrap();
    for name in ["t01", "t02", "t03", "t04", "t05", "t06"] {
        fs::write(
            dir.path().join(format!("stimuli/{name}.png")),
            b"not decoded in a headless run",
        )
        .unwrap();
    }
    dir
}

#[test]
fn the_synthetic_clock_runs_two_blocks_and_writes_every_gaze_sample() {
    let dir = fixture();
    let loaded = load_experiment(dir.path()).unwrap();
    let mut presenter = RecordingPresenter::default();
    let mut buffer = Vec::new();
    let report;

    {
        let identity = SessionIdentity::new("demo", Some("MS"), "42");
        let mut sink = DataSink::new(&mut buffer, identity, 0.0).unwrap();
        let mut frame_count = 0_u32;

        report = drive(loaded.plan, &mut sink, &mut presenter, |frame, _| {
            frame_count += 1;
            Some(GazeSample::new(
                frame.at,
                GazePoint::new(frame_count as f32, 2.0),
                GazePoint::new(0.5, 0.5),
            ))
        })
        .unwrap();
        sink.flush().unwrap();
    }

    // Two 10 ms ticks per 20 ms trial, six trials.
    assert_eq!(report.trials, 6);
    assert_eq!(report.completed, 6);
    assert_eq!(report.samples, 12);
    assert_eq!(report.late_trials, 0);
    assert_eq!(report.max_late, std::time::Duration::ZERO);
    assert_eq!(presenter.applied_commands().len(), 13);
    assert_eq!(
        presenter.applied_commands().last(),
        Some(&yet_stim::PresenterCommand::Stop)
    );

    let text = String::from_utf8(buffer).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 13);
    assert_eq!(lines[0], "Exp,Part,Stim,time,x,y,x_pro,y_pro");
    assert!(lines[1].starts_with("demoMS,42,t01,0.010000,"));
    assert!(lines.iter().any(|line| line.contains(",t06,")));

    let times: Vec<f64> = lines[1..]
        .iter()
        .map(|line| line.split(',').nth(3).unwrap().parse().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[0] < pair[1]),
        "timestamps must increase: {times:?}"
    );
}

#[test]
fn a_headless_run_creates_a_result_file_with_a_header() {
    let dir = fixture();
    let output = dir.path().join("out/result.csv");

    let report = run(HeadlessOptions::new(dir.path())
        .participant("42")
        .output(&output))
    .unwrap();

    assert_eq!(report.result_file.as_deref(), Some(output.as_path()));
    assert_eq!(report.trials, 6);
    assert_eq!(report.completed, 6);
    assert_eq!(report.samples, 0);
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        "Exp,Part,Stim,time,x,y,x_pro,y_pro\n"
    );
}

#[test]
fn the_default_result_file_lives_in_the_experiment_directory() {
    let dir = fixture();

    let report = run(HeadlessOptions::new(dir.path()).participant("42")).unwrap();

    let result_file = report.result_file.unwrap();
    assert_eq!(result_file.parent(), Some(dir.path()));
    assert!(result_file.ends_with("yet1_demoMS_42.csv"));
    assert!(result_file.is_file());
}

#[test]
fn a_missing_experiment_directory_is_reported() {
    let dir = tempfile::tempdir().unwrap();

    let error = run(HeadlessOptions::new(&dir.path().join("missing"))).unwrap_err();

    assert!(matches!(error, RunError::Load(_)));
}

#[test]
fn the_cli_runs_headless_and_reports_the_trial_count() {
    let dir = fixture();
    let output = dir.path().join("cli.csv");

    let output_result = Command::new(env!("CARGO_BIN_EXE_yet-zero"))
        .arg("run")
        .arg("--headless")
        .arg(dir.path())
        .arg("--participant")
        .arg("7")
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();

    assert!(
        output_result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output_result.stderr)
    );
    let stdout = String::from_utf8(output_result.stdout).unwrap();
    assert!(stdout.contains("Trials: 6 of 6 completed"), "{stdout}");
    assert!(stdout.contains("Samples: 0"), "{stdout}");
    assert!(output.is_file());
}

#[test]
fn the_cli_rejects_invalid_arguments_with_exit_code_two() {
    let status = Command::new(env!("CARGO_BIN_EXE_yet-zero"))
        .arg("run")
        .arg("--headless")
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(2));
}

#[test]
fn the_plan_command_still_works() {
    let dir = fixture();

    let output = Command::new(env!("CARGO_BIN_EXE_yet-zero"))
        .arg("plan")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Experiment: demo"), "{stdout}");
    assert!(stdout.contains("Block B1 (3 trials)"), "{stdout}");
}
