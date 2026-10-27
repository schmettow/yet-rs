// SPDX-License-Identifier: GPL-3.0-only

//! yet_zero-compatible result CSV.
//!
//! The prototype writes one row per gaze observation with the columns
//! `Exp,Part,Stim,time,x,y,x_pro,y_pro`. The sink owns those columns and no UI,
//! camera or clock types; the application supplies epoch time and the gaze
//! sample.

use std::io::Write;

use crate::SessionTime;
use thiserror::Error;

use crate::experiment::StimulusId;
use yet_core::GazePoint;

/// Column names, in write order, exactly as in the Python prototype.
pub const DATA_COLUMNS: [&str; 8] = ["Exp", "Part", "Stim", "time", "x", "y", "x_pro", "y_pro"];

/// Experiment and participant identifiers for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    /// Value of the `Exp` column: experiment id followed by the experimenter.
    pub experiment: String,
    /// Value of the `Part` column.
    pub participant: String,
}

impl SessionIdentity {
    /// Build the identity used in every row.
    ///
    /// As in the prototype, `Exp` concatenates the experiment id and the
    /// experimenter without a separator.
    pub fn new(
        experiment_id: &str,
        experimenter: Option<&str>,
        participant: impl Into<String>,
    ) -> Self {
        let mut experiment = experiment_id.to_owned();
        if let Some(experimenter) = experimenter {
            experiment.push_str(experimenter);
        }
        Self {
            experiment,
            participant: participant.into(),
        }
    }

    /// Result file name for a given prefix, e.g. `yet1_DemoMS_1765224885.csv`.
    pub fn file_name(&self, prefix: &str) -> String {
        format!("{prefix}_{}_{}.csv", self.experiment, self.participant)
    }
}

/// One gaze observation attached to a presented stimulus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GazeSample {
    /// Session time of the observation.
    pub at: SessionTime,
    /// Gaze in stimulus pixels.
    pub pixel: GazePoint,
    /// Gaze normalized to the stimulus size, with `0.0..=1.0` inside it.
    pub normalized: GazePoint,
}

impl GazeSample {
    /// Create a gaze sample.
    pub const fn new(at: SessionTime, pixel: GazePoint, normalized: GazePoint) -> Self {
        Self {
            at,
            pixel,
            normalized,
        }
    }
}

/// Errors from writing the result CSV.
#[derive(Debug, Error)]
pub enum DataError {
    /// A row could not be written.
    #[error("could not write the result CSV: {0}")]
    Csv(#[from] csv::Error),
    /// Buffered rows could not be flushed.
    #[error("could not flush the result CSV: {0}")]
    Io(#[from] std::io::Error),
}

/// Writes yet_zero-compatible result rows.
///
/// The header is written when the sink is created, so an interrupted session
/// still leaves a valid file. The application flushes at every trial boundary.
/// Times are epoch seconds with microsecond precision; pixel coordinates use
/// two decimals and normalized coordinates four, matching the prototype's
/// observed output.
#[derive(Debug)]
pub struct DataSink<W: Write> {
    writer: csv::Writer<W>,
    identity: SessionIdentity,
    epoch_start: f64,
}

impl<W: Write> DataSink<W> {
    /// Create a sink and write the header.
    ///
    /// `epoch_start` is the wall-clock time at session time zero, in seconds
    /// since the Unix epoch. Row times are derived from it plus the sample's
    /// session time.
    pub fn new(writer: W, identity: SessionIdentity, epoch_start: f64) -> Result<Self, DataError> {
        let mut writer = csv::WriterBuilder::new().from_writer(writer);
        writer.write_record(DATA_COLUMNS)?;
        writer.flush()?;
        Ok(Self {
            writer,
            identity,
            epoch_start,
        })
    }

    /// Append one gaze observation.
    pub fn append(&mut self, stimulus: &StimulusId, sample: GazeSample) -> Result<(), DataError> {
        let time = format!(
            "{:.6}",
            self.epoch_start + sample.at.as_duration().as_secs_f64()
        );
        let x = format!("{:.2}", sample.pixel.x);
        let y = format!("{:.2}", sample.pixel.y);
        let x_pro = format!("{:.4}", sample.normalized.x);
        let y_pro = format!("{:.4}", sample.normalized.y);

        self.writer.write_record([
            self.identity.experiment.as_str(),
            self.identity.participant.as_str(),
            stimulus.as_str(),
            &time,
            &x,
            &y,
            &x_pro,
            &y_pro,
        ])?;
        Ok(())
    }

    /// Push buffered rows to the underlying writer.
    pub fn flush(&mut self) -> Result<(), DataError> {
        self.writer.flush()?;
        Ok(())
    }

    /// The session identity written into every row.
    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }
}
