// SPDX-License-Identifier: GPL-3.0-only

//! Golden scenes for calibration, stimulus placement and instructions.
//!
//! The expected values come from the Python prototype:
//! `Calib.__init__` (proportions, scale) and `Stimulus.load` (placement).

use std::time::{Duration, Instant};

use yet_core::calibration::{
    CalibrationGrid, ACTIVE_TARGET_COLOR, TARGET_COLOR, TARGET_RADIUS_PX, TARGET_STROKE_PX,
};
use yet_core::layout::{
    instruction_scene, StimulusPlacement, BACKGROUND_COLOR, INSTRUCTION_FONT,
    INSTRUCTION_RELATIVE_POSITION,
};
use yet_present::{
    Anchor, ClockSource, Color, DrawCommand, Filter, FrameStamp, MediaId, PointPx, Presenter,
    RecordingPresenter, RectPx, Scene, SessionTime,
};

fn frame_stamp() -> FrameStamp {
    let now = Instant::now();
    FrameStamp {
        experiment_t: SessionTime::ZERO,
        requested_at: now,
        presented_at: now,
        dt: Duration::ZERO,
        media_pts: None,
        audio_pts: None,
        master: ClockSource::Monotonic,
        dropped: false,
    }
}

/// Present a scene through the recording presenter and return its commands.
fn recorded(scene: &Scene) -> Vec<DrawCommand> {
    let mut presenter = RecordingPresenter::default();
    presenter.present(&frame_stamp(), scene);
    assert_eq!(presenter.scenes().len(), 1);
    presenter.scenes()[0].commands().to_vec()
}

#[test]
fn the_calibration_grid_uses_the_python_proportions_in_row_major_order() {
    let grid = CalibrationGrid::new((1920, 1080));

    assert_eq!(grid.len(), 9);
    assert_eq!(
        grid.targets()
            .iter()
            .map(|target| target.position_px)
            .collect::<Vec<_>>(),
        vec![
            PointPx::new(240.0, 135.0),
            PointPx::new(960.0, 135.0),
            PointPx::new(1680.0, 135.0),
            PointPx::new(240.0, 540.0),
            PointPx::new(960.0, 540.0),
            PointPx::new(1680.0, 540.0),
            PointPx::new(240.0, 945.0),
            PointPx::new(960.0, 945.0),
            PointPx::new(1680.0, 945.0),
        ]
    );
    assert_eq!(
        grid.targets()
            .iter()
            .map(|target| (target.normalized.x, target.normalized.y))
            .collect::<Vec<_>>(),
        vec![
            (0.125, 0.125),
            (0.5, 0.125),
            (0.875, 0.125),
            (0.125, 0.5),
            (0.5, 0.5),
            (0.875, 0.5),
            (0.125, 0.875),
            (0.5, 0.875),
            (0.875, 0.875),
        ]
    );
}

#[test]
fn the_calibration_scene_draws_all_targets_and_highlights_the_active_one() {
    let grid = CalibrationGrid::new((1920, 1080));

    let commands = recorded(&grid.scene(Some(4)));

    assert_eq!(commands.len(), 10);
    assert_eq!(commands[0], DrawCommand::Clear(BACKGROUND_COLOR));
    for (index, target) in grid.targets().iter().enumerate() {
        let color = if index == 4 {
            ACTIVE_TARGET_COLOR
        } else {
            TARGET_COLOR
        };
        assert_eq!(
            commands[index + 1],
            DrawCommand::Circle {
                center_px: target.position_px,
                radius_px: TARGET_RADIUS_PX,
                stroke_px: TARGET_STROKE_PX,
                color,
            }
        );
    }
}

#[test]
fn a_calibration_scene_without_an_active_target_is_all_grey() {
    let grid = CalibrationGrid::new((800, 600));

    let commands = recorded(&grid.scene(None));

    assert_eq!(commands.len(), 10);
    assert!(commands[1..].iter().all(|command| matches!(
        command,
        DrawCommand::Circle { color, .. } if *color == TARGET_COLOR
    )));
}

#[test]
fn an_out_of_range_active_target_is_ignored() {
    let grid = CalibrationGrid::new((800, 600));

    assert_eq!(grid.scene(Some(99)), grid.scene(None));
    assert_eq!(grid.target(99), None);
    assert_eq!(grid.target(0).map(|target| target.index), Some(0));
}

#[test]
fn a_landscape_stimulus_is_scaled_to_fit_and_centered() {
    // 640x480 on 1920x1080: the height is limiting, scale 2.25.
    let placement = StimulusPlacement::fit((640, 480), (1920, 1080)).unwrap();

    assert_eq!(placement.scale, 2.25);
    assert_eq!(placement.dest_px, RectPx::new(240.0, 0.0, 1440.0, 1080.0));
}

#[test]
fn a_portrait_stimulus_is_scaled_to_fit_and_centered() {
    // 1000x2000 on 1920x1080: the height is limiting, scale 0.54.
    let placement = StimulusPlacement::fit((1000, 2000), (1920, 1080)).unwrap();

    assert_eq!(placement.scale, 0.54);
    assert_eq!(placement.dest_px, RectPx::new(690.0, 0.0, 540.0, 1080.0));
}

#[test]
fn a_small_stimulus_is_upscaled_to_fill_the_height() {
    // 320x240 on 1920x1080: scale 4.5, exactly filling the height.
    let placement = StimulusPlacement::fit((320, 240), (1920, 1080)).unwrap();

    assert_eq!(placement.scale, 4.5);
    assert_eq!(placement.dest_px, RectPx::new(240.0, 0.0, 1440.0, 1080.0));
}

#[test]
fn zero_dimensions_have_no_placement() {
    assert_eq!(StimulusPlacement::fit((0, 480), (1920, 1080)), None);
    assert_eq!(StimulusPlacement::fit((640, 480), (1920, 0)), None);
}

#[test]
fn the_placement_scene_clears_the_surface_and_draws_the_media() {
    let media = MediaId::new(7);
    let placement = StimulusPlacement::fit((640, 480), (1920, 1080)).unwrap();

    let commands = recorded(&placement.scene(media));

    assert_eq!(
        commands,
        vec![
            DrawCommand::Clear(BACKGROUND_COLOR),
            DrawCommand::Image {
                media,
                dest_px: RectPx::new(240.0, 0.0, 1440.0, 1080.0),
                filter: Filter::Linear,
                alpha: 1.0,
            },
        ]
    );
}

#[test]
fn the_instruction_scene_places_text_like_the_python_helper() {
    let commands = recorded(&instruction_scene(
        "Follow the orange circle.",
        (1920, 1080),
    ));

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0], DrawCommand::Clear(BACKGROUND_COLOR));
    assert_eq!(
        commands[1],
        DrawCommand::Text {
            pos_px: PointPx::new(
                INSTRUCTION_RELATIVE_POSITION.0 * 1920.0,
                INSTRUCTION_RELATIVE_POSITION.1 * 1080.0,
            ),
            text: "Follow the orange circle.".to_owned(),
            font: INSTRUCTION_FONT,
            size_px: 18.0,
            color: Color::BLACK,
            anchor: Anchor::TopLeft,
        }
    );
}

#[test]
fn calibration_targets_can_seed_engine_samples() {
    // The engine carries normalized targets as `GazePoint`; the grid must be
    // directly usable for calibration samples.
    let grid = CalibrationGrid::new((1920, 1080));
    let target = grid.target(8).unwrap();

    assert_eq!(target.normalized, yet_core::GazePoint::new(0.875, 0.875));
    assert_eq!(target.position_px, PointPx::new(1680.0, 945.0));
}
