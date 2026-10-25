// SPDX-License-Identifier: GPL-3.0-only

//! The calibration flow: target order, reports, quick recalibration, failure.

use std::time::Duration;

use yet_core::{
    CalibrationError, CalibrationGrid, CalibrationPhase, CalibrationRun, CalibrationTiming,
    QuadBrightness, QuickBackground, ACTIVE_TARGET_COLOR, TARGET_COLOR,
};
use yet_present::{Color, DrawCommand, MediaId, RectPx, SessionTime};

const SURFACE: (u32, u32) = (1920, 1080);
const STEP: Duration = Duration::from_millis(5);

fn timing() -> CalibrationTiming {
    CalibrationTiming {
        settle: Duration::from_millis(10),
        min_collect: Duration::from_millis(10),
        max_collect: Duration::from_millis(50),
        report_hold: Duration::from_millis(10),
        samples_per_target: 3,
    }
}

fn run() -> CalibrationRun {
    CalibrationRun::with_timing(CalibrationGrid::new(SURFACE), timing())
}

fn features(run: &CalibrationRun) -> Option<QuadBrightness> {
    let target = run
        .snapshot()
        .target
        .and_then(|index| run.grid().target(index))?;
    Some(QuadBrightness::new(
        target.position_px.x,
        target.position_px.y,
        target.position_px.x * 0.5,
        target.position_px.y * 0.5,
    ))
}

fn advance(run: &mut CalibrationRun, now: &mut SessionTime, until: CalibrationPhase) {
    for _ in 0..10_000 {
        if run.phase() == until {
            return;
        }
        *now = *now + STEP;
        let features = features(run);
        run.tick(*now, features).unwrap();
    }
    panic!("did not reach {until:?}; phase is {:?}", run.phase());
}

fn complete_full(run: &mut CalibrationRun, now: &mut SessionTime) {
    run.start(*now).unwrap();
    advance(run, now, CalibrationPhase::FullReport);
    for _ in 0..10 {
        if run.phase() == CalibrationPhase::Done {
            return;
        }
        *now = *now + STEP;
        run.tick(*now, None).unwrap();
    }
    panic!("report did not finish");
}

#[test]
fn a_full_run_visits_every_target_and_reports_a_fitted_model() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    run.start(now).unwrap();

    let mut targets = Vec::new();
    for _ in 0..10_000 {
        if run.phase() == CalibrationPhase::FullReport {
            break;
        }
        let snapshot = run.snapshot();
        if snapshot.phase == CalibrationPhase::Settle && targets.last().copied() != snapshot.target
        {
            targets.push(snapshot.target.unwrap());
        }
        now = now + STEP;
        let features = features(&run);
        run.tick(now, features).unwrap();
    }

    assert_eq!(targets, (0..9).collect::<Vec<_>>());
    let snapshot = run.snapshot();
    assert_eq!(snapshot.phase, CalibrationPhase::FullReport);
    assert_eq!(snapshot.completed, 9);
    assert!(snapshot.fitted);
    let report = snapshot.report.expect("a full report");
    assert_eq!(report.samples, 9);
    assert!(report.rmse < 1.0, "rmse {}", report.rmse);
    assert_eq!(run.phase(), CalibrationPhase::FullReport);
}

#[test]
fn the_scene_marks_the_active_target_and_shows_progress() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    run.start(now).unwrap();

    let commands = run.scene().commands().to_vec();
    assert_eq!(commands.len(), 11);
    assert_eq!(commands[0], DrawCommand::Clear(Color::WHITE));
    assert!(matches!(
        commands[1],
        DrawCommand::Circle { color, .. } if color == ACTIVE_TARGET_COLOR
    ));
    assert!(matches!(
        commands[2],
        DrawCommand::Circle { color, .. } if color == TARGET_COLOR
    ));
    match commands.last().unwrap() {
        DrawCommand::Text { text, .. } => {
            assert!(text.contains("1/9"), "{text}");
        }
        other => panic!("expected progress text, got {other:?}"),
    }

    advance(&mut run, &mut now, CalibrationPhase::Collect);
    let commands = run.scene().commands().to_vec();
    assert!(matches!(
        commands[1],
        DrawCommand::Circle { color, .. } if color == ACTIVE_TARGET_COLOR
    ));
}

#[test]
fn a_quick_recalibration_appends_the_center_sample() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    complete_full(&mut run, &mut now);
    let full = run.snapshot().report.expect("a full report");

    run.set_quick_background(Some(QuickBackground {
        media: MediaId::new(1),
        dest_px: RectPx::new(0.0, 0.0, 1920.0, 1080.0),
        radius_px: 8.0,
    }));
    run.quick(now).unwrap();
    assert_eq!(run.phase(), CalibrationPhase::QuickSettle);

    let commands = run.scene().commands().to_vec();
    assert_eq!(commands[0], DrawCommand::Clear(Color::WHITE));
    assert!(matches!(
        commands[1],
        DrawCommand::BlurredImage { media, .. } if media == MediaId::new(1)
    ));
    assert!(matches!(
        commands[2],
        DrawCommand::Circle { color, .. } if color == ACTIVE_TARGET_COLOR
    ));
    assert!(matches!(commands.last(), Some(DrawCommand::Text { .. })));

    advance(&mut run, &mut now, CalibrationPhase::QuickReport);
    let snapshot = run.snapshot();
    assert!(snapshot.fitted);
    let quick = snapshot.quick_report.expect("a quick report");
    assert_eq!(quick.samples, full.samples + 1);
    assert_eq!(snapshot.report, Some(full));
}

#[test]
fn a_quick_recalibration_needs_a_fit() {
    let mut run = run();
    let now = SessionTime::ZERO;

    assert_eq!(run.quick(now), Err(CalibrationError::NotFitted));
    assert_eq!(run.phase(), CalibrationPhase::Idle);
}

#[test]
fn a_full_run_cannot_start_twice() {
    let mut run = run();
    let now = SessionTime::ZERO;
    run.start(now).unwrap();

    assert_eq!(run.start(now), Err(CalibrationError::AlreadyActive));
}

#[test]
fn missing_frames_fail_with_an_actionable_message() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    run.start(now).unwrap();

    let mut result = Ok(());
    for _ in 0..100 {
        now = now + STEP;
        result = run.tick(now, None);
        if result.is_err() {
            break;
        }
    }

    assert!(result.is_err());
    assert_eq!(run.phase(), CalibrationPhase::Failed);
    let error = run.error().expect("an error message");
    assert!(error.contains("no camera frames"), "{error}");
    assert!(error.contains("target 1 of 9"), "{error}");
    assert!(!run.is_fitted());
}

#[test]
fn a_failed_full_run_restores_the_previous_calibration() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    complete_full(&mut run, &mut now);
    let previous = run.snapshot().report.expect("a previous report");

    run.start(now).unwrap();
    for _ in 0..100 {
        now = now + STEP;
        if run.tick(now, None).is_err() {
            break;
        }
    }

    assert_eq!(run.phase(), CalibrationPhase::Failed);
    assert!(run.is_fitted());
    assert_eq!(run.snapshot().report, Some(previous));
}

#[test]
fn cancelling_a_full_run_restores_the_previous_calibration() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    complete_full(&mut run, &mut now);
    let previous = run.snapshot().report.expect("a previous report");

    run.start(now).unwrap();
    for _ in 0..4 {
        now = now + STEP;
        let features = features(&run);
        run.tick(now, features).unwrap();
    }
    run.cancel();

    assert_eq!(run.phase(), CalibrationPhase::Cancelled);
    assert!(!run.is_active());
    assert!(run.is_fitted());
    assert_eq!(run.snapshot().report, Some(previous));
    assert!(run.scene().is_empty());
}

#[test]
fn a_failed_quick_recalibration_keeps_the_full_report() {
    let mut run = run();
    let mut now = SessionTime::ZERO;
    complete_full(&mut run, &mut now);
    let full = run.snapshot().report.expect("a full report");

    run.quick(now).unwrap();
    for _ in 0..100 {
        now = now + STEP;
        if run.tick(now, None).is_err() {
            break;
        }
    }

    assert_eq!(run.phase(), CalibrationPhase::Failed);
    assert!(run.is_fitted());
    assert_eq!(run.snapshot().report, Some(full));
    assert_eq!(run.snapshot().quick_report, None);
}
