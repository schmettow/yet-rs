// SPDX-License-Identifier: GPL-3.0-only

//! Deterministic trial planning.

use std::fmt;

use crate::experiment::{BlockId, Experiment, ExperimentError, Trial};
use crate::rng::shuffled_indices;

/// One trial in presentation order.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedTrial {
    /// Zero-based position within the block.
    pub position: usize,
    /// The trial and its stimulus.
    pub trial: Trial,
}

/// A block with its trials in presentation order.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedBlock {
    /// Block identifier.
    pub id: BlockId,
    /// Trials in presentation order.
    pub trials: Vec<PlannedTrial>,
}

/// A fully ordered session plan.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionPlan {
    /// Experiment identifier.
    pub experiment_id: String,
    /// Whether the trials were shuffled.
    pub shuffle: bool,
    /// Seed used for the shuffle.
    pub seed: Option<u64>,
    /// Blocks in presentation order.
    pub blocks: Vec<PlannedBlock>,
}

impl SessionPlan {
    /// Total number of trials across all blocks.
    pub fn trial_count(&self) -> usize {
        self.blocks.iter().map(|block| block.trials.len()).sum()
    }
}

impl fmt::Display for SessionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Experiment: {}", self.experiment_id)?;
        if self.shuffle {
            writeln!(
                f,
                "Trial order: shuffled (seed {})",
                self.seed.unwrap_or_default()
            )?;
        } else {
            writeln!(f, "Trial order: CSV order")?;
        }
        for block in &self.blocks {
            let noun = if block.trials.len() == 1 {
                "trial"
            } else {
                "trials"
            };
            writeln!(f, "Block {} ({} {}):", block.id, block.trials.len(), noun)?;
            for planned in &block.trials {
                let stimulus = &planned.trial.stimulus;
                let audio = if stimulus.audio.is_some() {
                    " + audio"
                } else {
                    ""
                };
                let fps = stimulus
                    .fps
                    .map_or_else(String::new, |fps| format!(", {fps} fps"));
                writeln!(
                    f,
                    "  {}. {} [{}] {} ms{}{} {}",
                    planned.position + 1,
                    stimulus.id,
                    stimulus.kind,
                    stimulus.duration.as_millis(),
                    fps,
                    audio,
                    stimulus.path.display()
                )?;
            }
        }
        Ok(())
    }
}

/// Build a presentation order for an experiment.
///
/// Trials are shuffled within each block. Block order follows the first
/// appearance of each block in `Stimuli.csv`. Block `i` uses `seed + i`, so
/// blocks receive independent deterministic streams from one session seed.
pub fn build(experiment: &Experiment) -> Result<SessionPlan, ExperimentError> {
    experiment.validate()?;

    let mut blocks = Vec::with_capacity(experiment.blocks.len());
    for (block_index, block) in experiment.blocks.iter().enumerate() {
        let order = if experiment.shuffle {
            let seed = experiment.seed.ok_or(ExperimentError::MissingSeed)?;
            shuffled_indices(block.trials.len(), seed.wrapping_add(block_index as u64))
        } else {
            (0..block.trials.len()).collect()
        };

        let trials = order
            .into_iter()
            .enumerate()
            .map(|(position, index)| PlannedTrial {
                position,
                trial: block.trials[index].clone(),
            })
            .collect();

        blocks.push(PlannedBlock {
            id: block.id.clone(),
            trials,
        });
    }

    Ok(SessionPlan {
        experiment_id: experiment.id.clone(),
        shuffle: experiment.shuffle,
        seed: experiment.seed,
        blocks,
    })
}
