// SPDX-License-Identifier: GPL-3.0-only

//! yet-rs binary.
//!
//! Sprint 2 exposes the resolved experiment plan. Later sprints add the
//! headless runner and the console/presenter.

use std::env;
use std::process::ExitCode;

use yet_core::{load_experiment, LoadError};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("plan"), Some(directory), None) => match run_plan(&directory) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        (Some("-h" | "--help"), None, None) => {
            usage();
            ExitCode::SUCCESS
        }
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage: yet-app plan <experiment-dir>");
}

fn run_plan(directory: &str) -> Result<(), LoadError> {
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
