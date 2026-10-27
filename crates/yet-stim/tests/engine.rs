// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;
use std::time::Duration;

use yet_core::{CalibrationSample, Clock, GazePoint, QuadBrightness, TestClock};
use yet_stim::{
    step, BlockId, ConsoleEvent, Effect, EngineError, EngineEvent, EngineState, FrameRecord, Key,
    Phase, PlannedBlock, PlannedTrial, SessionPlan, Stimulus, StimulusId, StimulusKind, Trial,
    TrialRecord,
};
use yet_stim::{MediaId, PresenterCommand, SessionTime};

fn at(millis: u64) -> SessionTime {
    SessionTime::from_duration(Duration::from_millis(millis))
}

fn stimulus(id: &str, millis: u64) -> Stimulus {
    Stimulus {
        id: StimulusId::from(id),
        kind: StimulusKind::Image,
        path: PathBuf::from(format!("stimuli/{id}.png")),
        duration: Duration::from_millis(millis),
        fps: None,
        looped: false,
        audio: None,
    }
}

fn planned(id: &str, position: usize) -> PlannedTrial {
    PlannedTrial {
        position,
        trial: Trial::new(stimulus(id, 100)),
    }
}

/// Two blocks with two 100 ms trials each.
fn plan() -> SessionPlan {
    SessionPlan {
        experiment_id: "demo".to_owned(),
        shuffle: false,
        seed: None,
        blocks: vec![
            PlannedBlock {
                id: BlockId::from("B1"),
                trials: vec![planned("t01", 0), planned("t02", 1)],
            },
            PlannedBlock {
                id: BlockId::from("B2"),
                trials: vec![planned("t03", 0), planned("t04", 1)],
            },
        ],
    }
}

fn trial_records(effects: &[Effect]) -> Vec<&TrialRecord> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::AppendTrial(record) => Some(record),
            _ => None,
        })
        .collect()
}

fn frame_records(effects: &[Effect]) -> Vec<&FrameRecord> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::AppendFrame(record) => Some(record),
            _ => None,
        })
        .collect()
}

#[test]
fn start_without_a_plan_enters_the_running_phase_and_clears_the_screen() {
    let mut state = EngineState::default();
    let now = at(100);

    let effects = step(&mut state, EngineEvent::Start, now);

    assert_eq!(state.phase, Phase::Running);
    assert_eq!(state.session_start, Some(now));
    assert_eq!(state.last_tick, Some(now));
    assert!(
        effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Running
        )))
    );
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::Present(scene) if scene.is_empty())));
    assert!(trial_records(&effects).is_empty());
}

#[test]
fn start_with_a_plan_announces_the_first_trial() {
    let mut state = EngineState::new(plan());

    let effects = step(&mut state, EngineEvent::Start, at(0));

    assert_eq!(state.phase, Phase::Running);
    assert_eq!(state.block_index, 0);
    assert_eq!(state.trial_index, 0);
    assert_eq!(state.trial_started_at, Some(at(0)));
    assert_eq!(state.deadline, Some(at(100)));
    assert_eq!(
        effects,
        vec![
            Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Running)),
            Effect::TrialStarted {
                block: BlockId::from("B1"),
                trial_index: 0,
                stimulus: stimulus("t01", 100),
            },
        ]
    );
}

#[test]
fn a_tick_before_the_deadline_records_a_frame() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Tick, at(40));

    assert_eq!(
        frame_records(&effects),
        vec![&FrameRecord {
            at: at(40),
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: StimulusId::from("t01"),
            trial_started_at: at(0),
            gaze: None,
        }]
    );
    assert!(trial_records(&effects).is_empty());
    assert_eq!(state.deadline, Some(at(100)));
    assert_eq!(state.phase, Phase::Running);
}

#[test]
fn a_tick_at_the_deadline_completes_the_trial_and_starts_the_next() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Tick, at(100));

    assert_eq!(
        trial_records(&effects),
        vec![&TrialRecord {
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: StimulusId::from("t01"),
            started_at: at(0),
            ended_at: at(100),
            completed: true,
            late_by: Duration::ZERO,
        }]
    );
    assert!(effects.contains(&Effect::TrialStarted {
        block: BlockId::from("B1"),
        trial_index: 1,
        stimulus: stimulus("t02", 100),
    }));
    assert_eq!(state.deadline, Some(at(200)));
    assert_eq!(state.trial_started_at, Some(at(100)));
    assert_eq!(state.phase, Phase::Running);
}

#[test]
fn a_late_tick_is_recorded_and_does_not_accumulate_drift() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));

    let effects = step(&mut state, EngineEvent::Tick, at(130));

    assert_eq!(
        trial_records(&effects),
        vec![&TrialRecord {
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: StimulusId::from("t01"),
            started_at: at(0),
            ended_at: at(130),
            completed: true,
            late_by: Duration::from_millis(30),
        }]
    );
    // The next trial starts at the tick time, not at the missed deadline.
    assert_eq!(state.trial_started_at, Some(at(130)));
    assert_eq!(state.deadline, Some(at(230)));
}

#[test]
fn a_late_tick_never_skips_a_trial() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));

    // The 350 ms tick is later than both B1 deadlines and B2's first deadline.
    let effects = step(&mut state, EngineEvent::Tick, at(350));

    assert_eq!(trial_records(&effects)[0].stimulus, StimulusId::from("t01"));
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::TrialStarted { .. }))
            .count(),
        1
    );
    assert_eq!(state.block_index, 0);
    assert_eq!(state.trial_index, 1);
}

#[test]
fn pause_freezes_the_clock_and_shifts_the_trial_deadline() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));
    step(&mut state, EngineEvent::Tick, at(40));

    step(&mut state, EngineEvent::Pause, at(50));
    assert_eq!(state.elapsed(at(200)), Some(Duration::from_millis(50)));

    let paused_tick = step(&mut state, EngineEvent::Tick, at(200));

    assert!(frame_records(&paused_tick).is_empty());
    assert_eq!(state.deadline, Some(at(100)));

    step(&mut state, EngineEvent::Resume, at(300));

    assert_eq!(state.paused_total, Duration::from_millis(250));
    assert_eq!(state.deadline, Some(at(350)));
    assert_eq!(state.phase, Phase::Running);

    let effects = step(&mut state, EngineEvent::Tick, at(350));

    assert_eq!(
        trial_records(&effects),
        vec![&TrialRecord {
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: StimulusId::from("t01"),
            started_at: at(0),
            ended_at: at(350),
            completed: true,
            late_by: Duration::ZERO,
        }]
    );
}

#[test]
fn aborting_mid_trial_records_an_incomplete_trial() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));
    step(&mut state, EngineEvent::Tick, at(40));

    let effects = step(&mut state, EngineEvent::Abort, at(60));

    assert_eq!(
        trial_records(&effects),
        vec![&TrialRecord {
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: StimulusId::from("t01"),
            started_at: at(0),
            ended_at: at(60),
            completed: false,
            late_by: Duration::ZERO,
        }]
    );
    assert_eq!(state.phase, Phase::Aborted);
    assert_eq!(state.deadline, None);
    assert!(effects.contains(&Effect::Presenter(PresenterCommand::Stop)));
}

#[test]
fn the_last_trial_finishes_the_session() {
    let mut state = EngineState::new(plan());
    step(&mut state, EngineEvent::Start, at(0));

    step(&mut state, EngineEvent::Tick, at(100));
    step(&mut state, EngineEvent::Tick, at(200));
    // Block B2 starts here.
    step(&mut state, EngineEvent::Tick, at(300));
    let effects = step(&mut state, EngineEvent::Tick, at(400));

    assert_eq!(state.phase, Phase::Finished);
    assert_eq!(state.deadline, None);
    assert_eq!(trial_records(&effects).len(), 1);
    assert_eq!(trial_records(&effects)[0].block, BlockId::from("B2"));
    assert!(
        effects.contains(&Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Finished
        )))
    );
    assert!(effects.contains(&Effect::Presenter(PresenterCommand::Stop)));
}

#[test]
fn an_empty_plan_is_rejected_without_starting() {
    let mut state = EngineState::new(SessionPlan {
        experiment_id: "empty".to_owned(),
        shuffle: false,
        seed: None,
        blocks: Vec::new(),
    });

    let effects = step(&mut state, EngineEvent::Start, at(0));

    assert_eq!(effects, vec![Effect::Error(EngineError::EmptyPlan)]);
    assert_eq!(state.phase, Phase::Idle);
    assert_eq!(state.session_start, None);
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
