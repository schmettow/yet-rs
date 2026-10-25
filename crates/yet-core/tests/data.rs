// SPDX-License-Identifier: GPL-3.0-only

use std::time::Duration;

use yet_core::data::{DataSink, GazeSample, SessionIdentity, DATA_COLUMNS};
use yet_core::{GazePoint, StimulusId};
use yet_present::SessionTime;

fn identity() -> SessionIdentity {
    SessionIdentity::new("Demo", Some("MS"), "1765224885")
}

#[test]
fn the_header_matches_the_yet_zero_schema() {
    assert_eq!(
        DATA_COLUMNS,
        ["Exp", "Part", "Stim", "time", "x", "y", "x_pro", "y_pro"]
    );

    let mut buffer = Vec::new();
    DataSink::new(&mut buffer, identity(), 0.0).unwrap();

    assert_eq!(
        String::from_utf8(buffer).unwrap(),
        "Exp,Part,Stim,time,x,y,x_pro,y_pro\n"
    );
}

#[test]
fn the_experiment_column_concatenates_id_and_experimenter() {
    let identity = identity();

    assert_eq!(identity.experiment, "DemoMS");
    assert_eq!(identity.participant, "1765224885");
    assert_eq!(identity.file_name("yet1"), "yet1_DemoMS_1765224885.csv");
}

#[test]
fn an_experiment_without_an_experimenter_still_has_an_experiment_column() {
    let identity = SessionIdentity::new("Demo", None, "42");

    assert_eq!(identity.experiment, "Demo");
    assert_eq!(identity.file_name("yet1"), "yet1_Demo_42.csv");
}

#[test]
fn a_row_keeps_the_python_column_order_and_precision() {
    let mut buffer = Vec::new();
    {
        let mut sink = DataSink::new(&mut buffer, identity(), 1_765_224_907.0).unwrap();
        sink.append(
            &StimulusId::from("t01"),
            GazeSample::new(
                SessionTime::from_duration(Duration::from_millis(88)),
                GazePoint::new(233.58, 219.37),
                GazePoint::new(0.2595, 0.2437),
            ),
        )
        .unwrap();
        sink.flush().unwrap();
    }

    let text = String::from_utf8(buffer).unwrap();
    assert_eq!(
        text,
        "Exp,Part,Stim,time,x,y,x_pro,y_pro\n\
         DemoMS,1765224885,t01,1765224907.088000,233.58,219.37,0.2595,0.2437\n"
    );
}

#[test]
fn flushing_persists_every_completed_trial() {
    let mut buffer = Vec::new();
    {
        let mut sink = DataSink::new(&mut buffer, identity(), 0.0).unwrap();
        for (index, stimulus) in ["t01", "t02"].into_iter().enumerate() {
            sink.append(
                &StimulusId::from(stimulus),
                GazeSample::new(
                    SessionTime::from_duration(Duration::from_secs(index as u64)),
                    GazePoint::new(10.0 + index as f32, 20.0),
                    GazePoint::new(0.1, 0.2),
                ),
            )
            .unwrap();
            sink.flush().unwrap();
        }
    }

    let text = String::from_utf8(buffer).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[1].contains(",t01,"));
    assert!(lines[2].contains(",t02,"));
}

#[test]
fn an_interrupted_session_keeps_the_rows_already_flushed() {
    let mut buffer = Vec::new();
    {
        let mut sink = DataSink::new(&mut buffer, identity(), 0.0).unwrap();
        sink.append(
            &StimulusId::from("t01"),
            GazeSample::new(
                SessionTime::from_duration(Duration::from_millis(100)),
                GazePoint::new(1.0, 2.0),
                GazePoint::new(0.01, 0.02),
            ),
        )
        .unwrap();
        sink.flush().unwrap();
        // The session aborts here; a second sample is never written.
    }

    let text = String::from_utf8(buffer).unwrap();
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn the_sink_exposes_its_identity() {
    let buffer = Vec::new();
    let sink = DataSink::new(buffer, identity(), 0.0).unwrap();

    assert_eq!(sink.identity(), &identity());
}
