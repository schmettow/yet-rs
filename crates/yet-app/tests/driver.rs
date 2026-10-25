// SPDX-License-Identifier: GPL-3.0-only

//! Scene composition and presenter wiring for the shared session driver.

use std::cell::Cell;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use yet_app::driver::{gaze_sample, RunError, SessionRunner};
use yet_core::layout::StimulusPlacement;
use yet_core::{
    BlockId, DataSink, Effect, FrameRecord, GazePoint, SessionIdentity, Stimulus, StimulusId,
    StimulusKind,
};
use yet_present::{
    DrawCommand, FrameStamp, MediaId, PresentStats, Presenter, PresenterCommand, PresenterError,
    RecordingPresenter, RectPx, Scene, ScreenGeometry, SessionTime,
};

fn image_stimulus(path: impl AsRef<Path>) -> Stimulus {
    Stimulus {
        id: StimulusId::new("t01"),
        kind: StimulusKind::Image,
        path: path.as_ref().to_path_buf(),
        duration: Duration::from_secs(3),
        fps: None,
        looped: false,
        audio: None,
    }
}

fn sink(directory: &Path) -> DataSink<BufWriter<File>> {
    let file = File::create(directory.join("result.csv")).unwrap();
    DataSink::new(
        BufWriter::new(file),
        SessionIdentity::new("demo", None, "42"),
        0.0,
    )
    .unwrap()
}

fn started(stimulus: Stimulus) -> Effect {
    Effect::TrialStarted {
        block: BlockId::new("B1"),
        trial_index: 0,
        stimulus,
    }
}

#[test]
fn a_trial_start_loads_media_and_composes_the_placed_scene() {
    let directory = tempfile::tempdir().unwrap();
    let stimulus = image_stimulus(directory.path().join("t01.png"));
    let mut sink = sink(directory.path());
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(1, None, (1440, 2560), 1.0))
        .with_media_size(MediaId::new(1), (4000, 3000));
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, _| None);

    runner.apply(started(stimulus.clone())).unwrap();

    let expected = StimulusPlacement::fit((4000, 3000), (1440, 2560)).unwrap();
    assert_eq!(runner.placement(), Some(expected));
    assert_eq!(runner.current(), Some(&StimulusId::new("t01")));
    assert_eq!(
        runner.scene().commands(),
        &[
            DrawCommand::Clear(yet_core::layout::BACKGROUND_COLOR),
            DrawCommand::Image {
                media: MediaId::new(1),
                dest_px: expected.dest_px,
                filter: yet_present::Filter::Linear,
                alpha: 1.0,
            },
        ]
    );

    let applied = presenter.applied_commands();
    assert_eq!(applied.len(), 2);
    assert!(matches!(
        &applied[0],
        yet_present::PresenterCommand::LoadImage { id, path }
            if *id == MediaId::new(1) && path == &stimulus.path
    ));
    assert!(matches!(
        &applied[1],
        yet_present::PresenterCommand::Play { id } if *id == MediaId::new(1)
    ));
}

#[test]
fn presenting_after_a_trial_start_sends_the_scene_to_the_presenter() {
    let directory = tempfile::tempdir().unwrap();
    let stimulus = image_stimulus(directory.path().join("t01.png"));
    let mut sink = sink(directory.path());
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(0, None, (1920, 1080), 1.0))
        .with_media_size(MediaId::new(1), (640, 480));
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, _| None);
    runner.apply(started(stimulus)).unwrap();

    let stats = runner.present(
        SessionTime::from_duration(Duration::from_millis(40)),
        Duration::from_millis(16),
    );
    let scene = runner.scene().clone();
    drop(runner);

    assert_eq!(presenter.scenes().len(), 1);
    assert_eq!(presenter.scenes()[0], scene);
    assert_eq!(
        stats.frame.experiment_t,
        SessionTime::from_duration(Duration::from_millis(40))
    );
    assert_eq!(stats.frame.dt, Duration::from_millis(16));
    assert!(!stats.frame.dropped);
}

#[test]
fn unknown_media_size_leaves_the_scene_empty() {
    let directory = tempfile::tempdir().unwrap();
    let mut sink = sink(directory.path());
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(0, None, (1920, 1080), 1.0));
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, _| None);

    runner
        .apply(started(image_stimulus(directory.path().join("t01.png"))))
        .unwrap();

    assert!(runner.scene().is_empty());
    assert_eq!(runner.placement(), None);
}

#[test]
fn an_empty_surface_is_reported_as_an_invalid_placement() {
    let directory = tempfile::tempdir().unwrap();
    let mut sink = sink(directory.path());
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(0, None, (0, 0), 1.0))
        .with_media_size(MediaId::new(1), (640, 480));
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, _| None);

    let result = runner.apply(started(image_stimulus(directory.path().join("t01.png"))));

    assert!(matches!(
        result,
        Err(RunError::InvalidPlacement {
            media_px: (640, 480),
            surface_px: (0, 0),
            ..
        })
    ));
}

#[test]
fn monitor_gaze_maps_to_stimulus_pixels_and_normalized_units() {
    let placement = StimulusPlacement::fit((640, 480), (1920, 1080)).unwrap();
    let record = FrameRecord {
        at: SessionTime::from_duration(Duration::from_millis(1234)),
        block: BlockId::new("B1"),
        trial_index: 0,
        stimulus: StimulusId::new("t01"),
        trial_started_at: SessionTime::ZERO,
        gaze: None,
    };

    let sample = gaze_sample(&record, GazePoint::new(960.0, 540.0), placement);

    assert_eq!(sample.at, record.at);
    assert_eq!(sample.pixel, GazePoint::new(320.0, 240.0));
    assert_eq!(sample.normalized, GazePoint::new(0.5, 0.5));
}

#[test]
fn gaze_outside_the_stimulus_is_not_clamped() {
    let placement = StimulusPlacement::fit((640, 480), (1920, 1080)).unwrap();
    let record = FrameRecord {
        at: SessionTime::ZERO,
        block: BlockId::new("B1"),
        trial_index: 0,
        stimulus: StimulusId::new("t01"),
        trial_started_at: SessionTime::ZERO,
        gaze: None,
    };

    let sample = gaze_sample(&record, GazePoint::new(-100.0, -100.0), placement);

    assert!(sample.pixel.x < 0.0);
    assert!(sample.pixel.y < 0.0);
    assert!(sample.normalized.x < 0.0);
    assert!(sample.normalized.y < 0.0);
}

#[test]
fn appending_a_frame_passes_the_current_placement_to_the_gaze_provider() {
    let directory = tempfile::tempdir().unwrap();
    let mut sink = sink(directory.path());
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(0, None, (1920, 1080), 1.0))
        .with_media_size(MediaId::new(1), (640, 480));
    let mut seen = None;
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, placement| {
        seen = placement;
        None
    });
    runner
        .apply(started(image_stimulus(directory.path().join("t01.png"))))
        .unwrap();

    runner
        .apply(Effect::AppendFrame(FrameRecord {
            at: SessionTime::ZERO,
            block: BlockId::new("B1"),
            trial_index: 0,
            stimulus: StimulusId::new("t01"),
            trial_started_at: SessionTime::ZERO,
            gaze: None,
        }))
        .unwrap();

    drop(runner);
    assert_eq!(
        seen,
        Some(StimulusPlacement::fit((640, 480), (1920, 1080)).unwrap())
    );
}

#[test]
fn the_placement_rectangle_matches_the_fit_helper() {
    let placement = StimulusPlacement::fit((4000, 3000), (1440, 2560)).unwrap();

    assert_eq!(placement.dest_px, RectPx::new(0.0, 740.0, 1440.0, 1080.0));
}

#[test]
fn a_surface_resize_recomposes_the_current_scene() {
    struct ResizablePresenter {
        geometry: Rc<Cell<ScreenGeometry>>,
    }

    impl Presenter for ResizablePresenter {
        fn geometry(&self) -> ScreenGeometry {
            self.geometry.get()
        }

        fn media_size(&self, _media: MediaId) -> Option<(u32, u32)> {
            Some((640, 480))
        }

        fn apply(&mut self, _command: PresenterCommand) -> Result<(), PresenterError> {
            Ok(())
        }

        fn present(&mut self, frame: &FrameStamp, _scene: &Scene) -> PresentStats {
            PresentStats {
                frame: frame.clone(),
                dropped_frames: 0,
                audio_drift_ms: None,
                audio_underruns: 0,
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let mut sink = sink(directory.path());
    let geometry = Rc::new(Cell::new(ScreenGeometry::new(0, None, (1920, 1080), 1.0)));
    let mut presenter = ResizablePresenter {
        geometry: Rc::clone(&geometry),
    };
    let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_, _| None);
    runner
        .apply(started(image_stimulus(directory.path().join("t01.png"))))
        .unwrap();

    let first = runner.placement().unwrap();
    assert_eq!(first.dest_px, RectPx::new(240.0, 0.0, 1440.0, 1080.0));

    geometry.set(ScreenGeometry::new(0, None, (960, 540), 1.0));
    runner.refresh_placement();

    let second = runner.placement().unwrap();
    assert_ne!(second, first);
    assert_eq!(second.dest_px, RectPx::new(120.0, 0.0, 720.0, 540.0));
}
