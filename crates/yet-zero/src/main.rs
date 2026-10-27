// SPDX-License-Identifier: GPL-3.0-only

//! yet-rs binary.
//!
//! Sprint 2 exposes the resolved experiment plan. Sprint 3 adds the headless
//! runner; Sprint 6 adds the console and the stimulus presenter.

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use yet_stim::media::{PngSequence, WavAudio};
use yet_stim::{load_experiment, LoadError, StimulusKind};
use yet_zero::live::{self, CalibrateOptions, CameraProbe};
use yet_zero::{run, run_console, ConsoleOptions, HeadlessOptions, RunReport};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "plan" => run_plan(path),
        [command, path] if command == "media" => run_media(path),
        [command, rest @ ..] if command == "run" => run_session(rest),
        [command, rest @ ..] if command == "camera" => run_camera(rest),
        [command, rest @ ..] if command == "calibrate" => run_calibrate(rest),
        [command, path, rest @ ..] if command == "overlay" => run_overlay(path, rest),
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
        "usage:\n  yet-zero plan <experiment-dir>\n  \
         yet-zero media <experiment-dir>\n  \
         yet-zero run <experiment-dir> [--headless] [--participant <id>] [--output <csv>] [--device <n>]\n  \
         yet-zero camera <experiment-dir> [--device <n>] [--seconds <s>] [--save-roi <file>]\n  \
         yet-zero calibrate <experiment-dir> [--device <n>] [--samples <n>] [--quick] [--skip-live] [--live-seconds <s>] [--terminal]\n  \
         yet-zero overlay <experiment-dir> [--seconds <s>]"
    );
}

#[derive(Debug)]
enum CliError {
    Usage,
    Plan(LoadError),
    Run(yet_zero::RunError),
    Live(live::LiveError),
    Media(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => f.write_str("invalid arguments"),
            Self::Plan(error) => error.fmt(f),
            Self::Run(error) => error.fmt(f),
            Self::Live(error) => error.fmt(f),
            Self::Media(error) => f.write_str(error),
        }
    }
}

impl From<LoadError> for CliError {
    fn from(error: LoadError) -> Self {
        Self::Plan(error)
    }
}

impl From<yet_zero::RunError> for CliError {
    fn from(error: yet_zero::RunError) -> Self {
        Self::Run(error)
    }
}

impl From<live::LiveError> for CliError {
    fn from(error: live::LiveError) -> Self {
        Self::Live(error)
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
        "Camera: index={} {}x{} @ {} fps format={} roi={:?}",
        loaded.config.camera.usb_index,
        loaded.config.camera.width,
        loaded.config.camera.height,
        loaded.config.camera.fps,
        loaded.config.camera.format,
        loaded.config.camera.eye_region
    );
    println!(
        "Audio: enabled={} device={} volume={}",
        loaded.config.audio.enabled, loaded.config.audio.device, loaded.config.audio.volume
    );
    print!("{}", loaded.plan);
    Ok(())
}

fn run_media(directory: &str) -> Result<(), CliError> {
    let loaded = load_experiment(directory)?;
    let mut seen = HashSet::new();
    let mut sequences = 0;

    for block in &loaded.plan.blocks {
        for planned in &block.trials {
            let stimulus = &planned.trial.stimulus;
            if !seen.insert(stimulus.id.clone()) {
                continue;
            }
            match stimulus.kind {
                StimulusKind::Image => {
                    println!("image {}: {}", stimulus.id, stimulus.path.display());
                }
                StimulusKind::Sequence => {
                    let fps = stimulus.fps.ok_or_else(|| {
                        CliError::Media(format!("stimulus `{}` has no frame rate", stimulus.id))
                    })?;
                    let sequence = PngSequence::open(&stimulus.path, fps)
                        .map_err(|error| CliError::Media(format!("{}: {error}", stimulus.id)))?;
                    let info = sequence
                        .validate()
                        .map_err(|error| CliError::Media(format!("{}: {error}", stimulus.id)))?;
                    println!(
                        "sequence {}: {} frames · {:.3} fps · {:.3} s · {}×{} px",
                        stimulus.id,
                        info.frames,
                        info.fps,
                        info.duration.as_secs_f64(),
                        info.size_px.0,
                        info.size_px.1
                    );
                    if let Some(audio) = &stimulus.audio {
                        let wav = WavAudio::load(audio).map_err(|error| {
                            CliError::Media(format!("{}: {error}", stimulus.id))
                        })?;
                        let delta = wav.duration().as_secs_f64() - info.duration.as_secs_f64();
                        println!(
                            "  audio: {} · {} Hz · {} ch · {:.3} s",
                            audio.display(),
                            wav.sample_rate(),
                            wav.channels(),
                            wav.duration().as_secs_f64()
                        );
                        if delta.abs() > 0.1 {
                            println!(
                                "  warning: audio is {delta:+.3} s longer than the frames; \
                                 the trial duration is authoritative"
                            );
                        }
                    }
                    sequences += 1;
                }
            }
        }
    }

    println!("Validated {sequences} sequence(s).");
    Ok(())
}

fn run_session(args: &[String]) -> Result<(), CliError> {
    let mut headless = false;
    let mut directory: Option<&Path> = None;
    let mut participant: Option<&str> = None;
    let mut output: Option<&Path> = None;
    let mut device: Option<usize> = None;
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
            "--device" => {
                index += 1;
                device = Some(
                    args.get(index)
                        .ok_or(CliError::Usage)?
                        .parse()
                        .map_err(|_| CliError::Usage)?,
                );
            }
            flag if flag.starts_with('-') => return Err(CliError::Usage),
            path if directory.is_none() => directory = Some(Path::new(path)),
            _ => return Err(CliError::Usage),
        }
        index += 1;
    }

    if !headless {
        let mut options = ConsoleOptions::new(directory.ok_or(CliError::Usage)?);
        if let Some(participant) = participant {
            options = options.participant(participant);
        }
        if let Some(output) = output {
            options = options.output(output);
        }
        if let Some(device) = device {
            options = options.device(device);
        }
        run_console(options)?;
        return Ok(());
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

fn run_overlay(directory: &str, args: &[String]) -> Result<(), CliError> {
    let mut seconds = 10.0_f32;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--seconds" => {
                let value = args.get(index + 1).ok_or(CliError::Usage)?;
                seconds = value.parse().map_err(|_| CliError::Usage)?;
                index += 2;
            }
            _ => return Err(CliError::Usage),
        }
    }
    yet_zero::run_overlay(yet_zero::OverlayOptions::new(Path::new(directory)).seconds(seconds))?;
    Ok(())
}

fn run_camera(args: &[String]) -> Result<(), CliError> {
    let mut directory: Option<&Path> = None;
    let mut seconds = 2.0_f64;
    let mut save_roi: Option<&Path> = None;
    let mut device = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                index += 1;
                device = Some(
                    args.get(index)
                        .ok_or(CliError::Usage)?
                        .parse()
                        .map_err(|_| CliError::Usage)?,
                );
            }
            "--seconds" => {
                index += 1;
                seconds = args
                    .get(index)
                    .ok_or(CliError::Usage)?
                    .parse()
                    .map_err(|_| CliError::Usage)?;
            }
            "--save-roi" => {
                index += 1;
                save_roi = Some(Path::new(args.get(index).ok_or(CliError::Usage)?));
            }
            flag if flag.starts_with('-') => return Err(CliError::Usage),
            path if directory.is_none() => directory = Some(Path::new(path)),
            _ => return Err(CliError::Usage),
        }
        index += 1;
    }

    let probe = live::probe(directory.ok_or(CliError::Usage)?, seconds, save_roi, device)?;
    print_probe(&probe);
    Ok(())
}

fn print_probe(probe: &CameraProbe) {
    let fps = probe
        .info
        .fps
        .map_or_else(|| "unknown".to_owned(), |fps| format!("{fps:.1}"));
    println!("Camera: {} ({})", probe.info.name, probe.info.device);
    println!(
        "Format: {} {}x{} stride={} fps={}",
        probe.info.format, probe.info.width, probe.info.height, probe.info.stride, fps
    );
    println!(
        "Capture: {} frames in {:.2} s ({:.1} fps); consumed={} dropped={} corrupt={} errors={}",
        probe.stats.captured,
        probe.elapsed.as_secs_f64(),
        probe.captured_fps,
        probe.consumed,
        probe.stats.dropped,
        probe.stats.corrupt,
        probe.stats.errors
    );
    if let Some(error) = &probe.stats.last_error {
        println!("Last error: {error}");
    }
    println!(
        "ROI: x={} y={} {}x{}",
        probe.roi.x, probe.roi.y, probe.roi.width, probe.roi.height
    );
    if let Some(features) = probe.quadrants {
        println!(
            "Quadrants: nw={:.2} ne={:.2} sw={:.2} se={:.2}",
            features.nw, features.ne, features.sw, features.se
        );
    }
    if let Some(path) = &probe.saved_roi {
        println!(
            "Saved ROI: {} ({}x{} raw gray)",
            path.display(),
            probe.roi.width,
            probe.roi.height
        );
    }
}

fn run_calibrate(args: &[String]) -> Result<(), CliError> {
    let mut directory: Option<&Path> = None;
    let mut options = CalibrateOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--device" => {
                index += 1;
                options.device = Some(
                    args.get(index)
                        .ok_or(CliError::Usage)?
                        .parse()
                        .map_err(|_| CliError::Usage)?,
                );
            }
            "--terminal" => options.terminal = true,
            "--samples" => {
                index += 1;
                options.samples = args
                    .get(index)
                    .ok_or(CliError::Usage)?
                    .parse()
                    .map_err(|_| CliError::Usage)?;
            }
            "--quick" => options.quick = true,
            "--skip-live" => options.skip_live = true,
            "--live-seconds" => {
                index += 1;
                options.live_seconds = args
                    .get(index)
                    .ok_or(CliError::Usage)?
                    .parse()
                    .map_err(|_| CliError::Usage)?;
            }
            flag if flag.starts_with('-') => return Err(CliError::Usage),
            path if directory.is_none() => directory = Some(Path::new(path)),
            _ => return Err(CliError::Usage),
        }
        index += 1;
    }

    let directory = directory.ok_or(CliError::Usage)?;
    let live_samples = if options.terminal {
        live::calibrate(directory, options)?.live_samples
    } else {
        match yet_zero::window::run_calibration(directory, options)? {
            Some(report) => report.live_samples,
            None => {
                println!("Calibration cancelled.");
                return Ok(());
            }
        }
    };

    println!("Calibration finished; {live_samples} live gaze predictions.");
    Ok(())
}
