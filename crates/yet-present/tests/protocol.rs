// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, Instant};

use yet_present::{
    Anchor, ClockSource, Color, DrawCommand, Filter, FontId, FrameStamp, MediaId, PointPx,
    Presenter, PresenterCommand, RecordingPresenter, RectPx, Scene, ScreenGeometry, SessionTime,
    SizePx,
};

fn geometry() -> ScreenGeometry {
    ScreenGeometry::new(1, Some((1920, 0)), (2560, 1440), 1.5)
}

fn frame_stamp() -> FrameStamp {
    let now = Instant::now();
    FrameStamp {
        experiment_t: SessionTime::from_duration(Duration::from_millis(10)),
        requested_at: now,
        presented_at: now,
        dt: Duration::from_millis(16),
        media_pts: None,
        audio_pts: None,
        master: ClockSource::Monotonic,
        dropped: false,
    }
}

#[test]
fn session_time_is_ordered_and_saturating() {
    let start = SessionTime::ZERO;
    let later = start + Duration::from_millis(250);

    assert_eq!(
        later,
        SessionTime::from_duration(Duration::from_millis(250))
    );
    assert_eq!(later.duration_since(start), Duration::from_millis(250));
    assert_eq!(start.duration_since(later), Duration::ZERO);
    assert_eq!(
        later + Duration::from_millis(50),
        SessionTime::from_duration(Duration::from_millis(300))
    );
    assert_eq!(
        later.saturating_add(Duration::MAX).as_duration(),
        Duration::MAX
    );
    assert_eq!(
        start.checked_add(Duration::MAX),
        Some(SessionTime::from_duration(Duration::MAX))
    );
    assert_eq!(later.checked_add(Duration::MAX), None);
    assert_eq!(SessionTime::default(), SessionTime::ZERO);
}

#[test]
fn geometry_converts_points_and_pixels() {
    let geometry = geometry();

    assert_eq!(geometry.width_px(), 2560);
    assert_eq!(geometry.height_px(), 1440);
    assert_eq!(geometry.points_to_px(100.0), 150.0);
    assert_eq!(geometry.px_to_points(150.0), 100.0);
    assert_eq!(
        RectPx::new(10.0, 20.0, 100.0, 50.0).center(),
        PointPx::new(60.0, 45.0)
    );
    assert_eq!(
        RectPx::from_size(SizePx::new(10.0, 20.0)),
        RectPx::new(0.0, 0.0, 10.0, 20.0)
    );
}

#[test]
fn scene_builder_emits_draw_commands() {
    let media = MediaId::new(7);
    let mut scene = Scene::new();

    scene
        .clear(Color::WHITE)
        .image(
            media,
            RectPx::new(0.0, 0.0, 640.0, 480.0),
            Filter::Linear,
            0.75,
        )
        .circle(PointPx::new(320.0, 240.0), 12.0, 2.0, Color::BLACK)
        .text(
            PointPx::new(10.0, 20.0),
            "fixation",
            FontId::new(1),
            24.0,
            Color::BLACK,
            Anchor::TopLeft,
        )
        .line(
            PointPx::new(0.0, 0.0),
            PointPx::new(10.0, 10.0),
            1.0,
            Color::BLACK,
        );

    assert_eq!(scene.len(), 5);
    assert!(!scene.is_empty());
    assert_eq!(scene.commands()[0], DrawCommand::Clear(Color::WHITE));
    assert_eq!(
        scene.commands()[1],
        DrawCommand::Image {
            media,
            dest_px: RectPx::new(0.0, 0.0, 640.0, 480.0),
            filter: Filter::Linear,
            alpha: 0.75,
        }
    );

    match &scene.commands()[3] {
        DrawCommand::Text {
            text,
            font,
            size_px,
            color,
            anchor,
            ..
        } => {
            assert_eq!(text, "fixation");
            assert_eq!(*font, FontId::new(1));
            assert_eq!(*size_px, 24.0);
            assert_eq!(*color, Color::BLACK);
            assert_eq!(*anchor, Anchor::TopLeft);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    scene.clear_commands();
    assert!(scene.is_empty());
    assert_eq!(scene, Scene::default());
}

#[test]
fn recording_presenter_records_commands_scenes_and_frames() {
    let mut presenter = RecordingPresenter::new(geometry());
    let image = PresenterCommand::LoadImage {
        id: MediaId::new(1),
        path: "stimulus.png".into(),
    };
    let show = PresenterCommand::ShowMonitor {
        monitor: 1,
        fullscreen: true,
    };

    presenter.apply(image.clone()).unwrap();
    presenter.apply(show.clone()).unwrap();

    let mut scene = Scene::new();
    scene.clear(Color::BLACK);
    let frame = frame_stamp();
    let stats = presenter.present(&frame, &scene);

    assert_eq!(presenter.geometry(), geometry());
    assert_eq!(presenter.applied_commands(), &[image, show]);
    assert_eq!(presenter.scenes(), std::slice::from_ref(&scene));
    assert_eq!(presenter.frames(), std::slice::from_ref(&frame));
    assert_eq!(stats.frame, frame);
    assert_eq!(stats.dropped_frames, 0);
    assert_eq!(stats.audio_drift_ms, None);
    assert_eq!(stats.audio_underruns, 0);
}

#[test]
fn recording_presenter_has_a_usable_default() {
    let presenter = RecordingPresenter::default();

    assert_eq!(presenter.geometry().monitor_index, 0);
    assert_eq!(presenter.geometry().scale_factor, 1.0);
    assert!(presenter.applied_commands().is_empty());
    assert!(presenter.scenes().is_empty());
    assert!(presenter.frames().is_empty());
}
