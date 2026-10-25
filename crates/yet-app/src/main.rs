// SPDX-License-Identifier: GPL-3.0-only

//! yet-rs binary.
//!
//! Sprint 2 exposes the resolved experiment plan. Sprint 3 adds the headless
//! runner; later sprints add the console and the presenter window.

use std::env;
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use yet_app::{run, HeadlessOptions, RunReport};
use yet_core::{load_experiment, LoadError};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "plan" => run_plan(path),
        [command, rest @ ..] if command == "run" => run_headless(rest),
        [flag] if flag == "-h" || flag == "--help" => {
            usage();
            return ExitCode::SUCCESS;
        }
        _ => Err(CliError::Usage),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage) => {
            usage();
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "usage:\n  yet-app plan <experiment-dir>\n  \
         yet-app run --headless <experiment-dir> [--participant <id>] [--output <csv>]"
    );
}

#[derive(Debug)]
enum CliError {
    Usage,
    Plan(LoadError),
    Run(yet_app::RunError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => f.write_str("invalid arguments"),
            Self::Plan(error) => error.fmt(f),
            Self::Run(error) => error.fmt(f),
        }
    }
}

impl From<LoadError> for CliError {
    fn from(error: LoadError) -> Self {
        Self::Plan(error)
    }
}

impl From<yet_app::RunError> for CliError {
    fn from(error: yet_app::RunError) -> Self {
        Self::Run(error)
    }
}

fn run_plan(directory: &str) -> Result<(), CliError> {
    let loaded = load_experiment(directory)?;

    println!("Experiment root: {}", loaded.root.display());
    if let Some(experimenter) = &loaded.experiment.experimenter {
        println!("Experimenter: {experimenter}");
    }
    if let Some(instructions) = &loaded.experiment.instructions {
        println!("Instructions: {instructions}");
    }
    println!(
        "Presenter monitor: {} | console monitor: {}",
        loaded.config.display.presenter_monitor, loaded.config.display.console_monitor
    );
    println!(
        "Presenter backend: {} | overlay: {}",
        loaded.config.presenter.backend, loaded.config.presenter.overlay
    );
    println!(
        "Camera USB index: {} | audio: enabled={} device={} volume={}",
        loaded.config.camera.usb_index,
        loaded.config.audio.enabled,
        loaded.config.audio.device,
        loaded.config.audio.volume
    );
    print!("{}", loaded.plan);
    Ok(())
}

fn run_headless(args: &[String]) -> Result<(), CliError> {
    let mut headless = false;
    let mut directory: Option<&Path> = None;
    let mut participant: Option<&str> = None;
    let mut output: Option<&Path> = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--headless" => headless = true,
            "--participant" => {
                index += 1;
                participant = Some(args.get(index).ok_or(CliError::Usage)?.as_str());
            }
            "--output" => {
                index += 1;
                output = Some(Path::new(args.get(index).ok_or(CliError::Usage)?));
            }
            flag if flag.starts_with('-') => return Err(CliError::Usage),
            path if directory.is_none() => directory = Some(Path::new(path)),
            _ => return Err(CliError::Usage),
        }
        index += 1;
    }

    if !headless {
        return Err(CliError::Usage);
    }

    let mut options = HeadlessOptions::new(directory.ok_or(CliError::Usage)?);
    if let Some(participant) = participant {
        options = options.participant(participant);
    }
    if let Some(output) = output {
        options = options.output(output);
    }

    let report = run(options)?;
    print_report(&report);
    Ok(())
}

fn print_report(report: &RunReport) {
    if let Some(path) = &report.result_file {
        println!("Result: {}", path.display());
    }
    println!(
        "Trials: {} of {} completed",
        report.completed, report.trials
    );
    if report.late_trials > 0 {
        println!(
            "Late trials: {} (max {} ms)",
            report.late_trials,
            report.max_late.as_millis()
        );
    }
    println!("Samples: {}", report.samples);
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}
