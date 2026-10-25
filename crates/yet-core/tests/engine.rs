// SPDX-License-Identifier: GPL-3.0-only

use std::time::Duration;

use yet_core::{
    step, CalibrationSample, Clock, ConsoleEvent, Effect, EngineError, EngineEvent, EngineState,
    GazePoint, Key, Phase, QuadBrightness, TestClock, TrialRecord,
};
use yet_present::{MediaId, PresenterCommand, SessionTime};

fn at(millis: u64) -> SessionTime {
    SessionTime::from_duration(Duration::from_millis(millis))
}

#[test]
fn start_moves_idle_to_running_and_records_the_first_trial() {
    let mut state = EngineState::default();
    let now = at(100);

    let effects = step(&mut state, EngineEvent::Start, now);

    assert_eq!(state.phase, Phase::Running);
    assert_eq!(state.session_start, Some(now));
    assert_eq!(state.last_tick, Some(now));
    assert_eq!(state.trial_index, 0);
    assert!(
        effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Running
        )))
    );
    assert!(effects.contains(&Effect::AppendTrial(TrialRecord {
        index: 0,
        started_at: now,
        ended_at: None,
    })));
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::Present(scene) if scene.is_empty())));
}

#[test]
fn pause_and_resume_exclude_pause_time_from_elapsed_time() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));

    let pause_effects = step(&mut state, EngineEvent::Pause, at(100));

    assert_eq!(state.phase, Phase::Paused);
    assert_eq!(state.paused_at, Some(at(100)));
    assert_eq!(state.elapsed(at(200)), Some(Duration::from_millis(100)));
    assert!(
        pause_effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Paused
        )))
    );

    let resume_effects = step(&mut state, EngineEvent::Resume, at(300));

    assert_eq!(state.phase, Phase::Running);
    assert_eq!(state.paused_at, None);
    assert_eq!(state.paused_total, Duration::from_millis(200));
    assert_eq!(state.elapsed(at(400)), Some(Duration::from_millis(200)));
    assert!(
        resume_effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Running
        )))
    );
}

#[test]
fn elapsed_is_none_before_start() {
    let state = EngineState::default();

    assert_eq!(state.elapsed(at(10)), None);
}

#[test]
fn abort_stops_the_presenter() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Abort, at(10));

    assert_eq!(state.phase, Phase::Aborted);
    assert!(
        effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Aborted
        )))
    );
    assert!(effects.contains(&Effect::Presenter(PresenterCommand::Stop)));
}

#[test]
fn tick_updates_last_tick_and_notifies_the_console() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Tick, at(16));

    assert_eq!(state.last_tick, Some(at(16)));
    assert_eq!(
        effects,
        vec![Effect::NotifyConsole(ConsoleEvent::Tick { at: at(16) })]
    );
}

#[test]
fn tick_before_start_is_ignored() {
    let mut state = EngineState::default();

    let effects = step(&mut state, EngineEvent::Tick, at(10));

    assert!(effects.is_empty());
    assert_eq!(state.last_tick, None);
}

#[test]
fn start_after_abort_is_an_error() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));
    step(&mut state, EngineEvent::Abort, at(10));

    let effects = step(&mut state, EngineEvent::Start, at(20));

    assert_eq!(
        effects,
        vec![Effect::Error(EngineError::InvalidTransition {
            phase: Phase::Aborted,
            event: "start",
        })]
    );
    assert_eq!(state.phase, Phase::Aborted);
}

#[test]
fn escape_aborts_an_active_session() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Key(Key::Escape), at(10));

    assert_eq!(state.phase, Phase::Aborted);
    assert!(effects.contains(&Effect::Presenter(PresenterCommand::Stop)));
}

#[test]
fn external_error_aborts_an_active_session() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(
        &mut state,
        EngineEvent::Error(EngineError::NotStarted),
        at(10),
    );

    assert_eq!(state.phase, Phase::Aborted);
    assert!(effects.contains(&Effect::Error(EngineError::NotStarted)));
    assert!(effects.contains(&Effect::Presenter(PresenterCommand::Stop)));
}

#[test]
fn unknown_keys_are_forwarded_to_the_console() {
    let mut state = EngineState::default();

    let effects = step(&mut state, EngineEvent::Key(Key::Space), at(10));

    assert_eq!(
        effects,
        vec![Effect::NotifyConsole(ConsoleEvent::Key(Key::Space))]
    );
}

#[test]
fn calibration_samples_are_forwarded_to_the_console() {
    let mut state = EngineState::default();
    step(&mut state, EngineEvent::Start, at(0));
    let sample = CalibrationSample::new(
        QuadBrightness::new(1.0, 2.0, 3.0, 4.0),
        GazePoint::new(0.5, 0.25),
    );

    let effects = step(&mut state, EngineEvent::CalibrationSample(sample), at(10));

    assert_eq!(
        effects,
        vec![Effect::NotifyConsole(ConsoleEvent::CalibrationSample(
            sample
        ))]
    );
}

#[test]
fn media_ready_is_forwarded_to_the_console() {
    let media = MediaId::new(3);

    let effects = step(
        &mut EngineState::default(),
        EngineEvent::MediaReady(media),
        at(10),
    );

    assert_eq!(
        effects,
        vec![Effect::NotifyConsole(ConsoleEvent::MediaReady(media))]
    );
}

#[test]
fn test_clock_advances_and_can_be_set() {
    let mut clock = TestClock::new();

    assert_eq!(clock.now(), SessionTime::ZERO);
    assert_eq!(clock.advance(Duration::from_millis(500)), at(500));
    assert_eq!(clock.now(), at(500));

    clock.set(at(250));
    assert_eq!(clock.now(), at(250));
}
