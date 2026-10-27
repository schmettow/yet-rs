// SPDX-License-Identifier: GPL-3.0-only

//! Experiment definition, CSV loading and structural validation.
//!
//! `Experiment.csv` is an optional key/value metadata file. `Stimuli.csv`
//! defines the stimuli and their blocks. Media paths are resolved relative to
//! the experiment directory.

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

/// Stable identifier of a stimulus.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StimulusId(String);

impl StimulusId {
    /// Create a stimulus identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StimulusId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for StimulusId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Stable identifier of a block.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(String);

impl BlockId {
    /// Create a block identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for BlockId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Media kind of a stimulus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StimulusKind {
    /// A single still image.
    Image,
    /// A pre-decoded frame sequence.
    Sequence,
}

impl fmt::Display for StimulusKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image => f.write_str("image"),
            Self::Sequence => f.write_str("sequence"),
        }
    }
}

/// One stimulus definition with resolved media paths.
#[derive(Clone, Debug, PartialEq)]
pub struct Stimulus {
    /// Stable identifier.
    pub id: StimulusId,
    /// Image or frame sequence.
    pub kind: StimulusKind,
    /// Resolved image file or sequence directory.
    pub path: PathBuf,
    /// Intended presentation duration.
    pub duration: Duration,
    /// Frame rate for a sequence.
    pub fps: Option<f32>,
    /// Whether a sequence loops.
    pub looped: bool,
    /// Optional WAV track for a sequence.
    pub audio: Option<PathBuf>,
}

impl Stimulus {
    /// Validate the stimulus definition without touching the filesystem.
    pub fn validate(&self) -> Result<(), ExperimentError> {
        if self.duration.is_zero() {
            return Err(ExperimentError::InvalidDuration {
                id: self.id.clone(),
                value: "0".to_owned(),
            });
        }
        match self.kind {
            StimulusKind::Image => {
                if self.fps.is_some() {
                    return Err(ExperimentError::ImageWithFps {
                        id: self.id.clone(),
                    });
                }
                if self.looped {
                    return Err(ExperimentError::ImageWithLoop {
                        id: self.id.clone(),
                    });
                }
                if self.audio.is_some() {
                    return Err(ExperimentError::ImageWithAudio {
                        id: self.id.clone(),
                    });
                }
            }
            StimulusKind::Sequence => {
                let fps = self.fps.ok_or_else(|| ExperimentError::MissingFps {
                    id: self.id.clone(),
                })?;
                if !fps.is_finite() || fps <= 0.0 {
                    return Err(ExperimentError::InvalidFps {
                        id: self.id.clone(),
                        value: fps.to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// One trial: one stimulus presentation.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial {
    /// Stimulus to present.
    pub stimulus: Stimulus,
}

impl Trial {
    /// Create a trial.
    pub fn new(stimulus: Stimulus) -> Self {
        Self { stimulus }
    }
}

/// A named, ordered group of trials.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// Block identifier.
    pub id: BlockId,
    /// Trials in CSV order.
    pub trials: Vec<Trial>,
}

impl Block {
    /// Create a block.
    pub fn new(id: impl Into<BlockId>, trials: Vec<Trial>) -> Self {
        Self {
            id: id.into(),
            trials,
        }
    }

    /// Validate that the block has trials and that every stimulus is valid.
    pub fn validate(&self) -> Result<(), ExperimentError> {
        if self.trials.is_empty() {
            return Err(ExperimentError::EmptyBlock {
                id: self.id.clone(),
            });
        }
        for trial in &self.trials {
            trial.stimulus.validate()?;
        }
        Ok(())
    }
}

/// A complete experiment definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Experiment {
    /// Experiment identifier.
    pub id: String,
    /// Optional experimenter name.
    pub experimenter: Option<String>,
    /// Optional instructions text.
    pub instructions: Option<String>,
    /// Blocks in presentation order.
    pub blocks: Vec<Block>,
    /// Whether trials are shuffled within blocks.
    pub shuffle: bool,
    /// Seed for the shuffle; required when `shuffle` is true.
    pub seed: Option<u64>,
}

impl Experiment {
    /// Validate the complete experiment.
    pub fn validate(&self) -> Result<(), ExperimentError> {
        if self.blocks.is_empty() {
            return Err(ExperimentError::EmptyExperiment);
        }
        if self.shuffle && self.seed.is_none() {
            return Err(ExperimentError::MissingSeed);
        }

        let mut seen = HashSet::new();
        for block in &self.blocks {
            block.validate()?;
            for trial in &block.trials {
                if !seen.insert(trial.stimulus.id.clone()) {
                    return Err(ExperimentError::DuplicateStimulusId {
                        id: trial.stimulus.id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Experiment loading and validation errors.
#[derive(Debug, Error)]
pub enum ExperimentError {
    /// A file could not be read.
    #[error("could not read `{path}`: {source}")]
    Io {
        /// Attempted path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A CSV file could not be parsed.
    #[error("invalid CSV in `{path}`: {source}")]
    Csv {
        /// Attempted path.
        path: PathBuf,
        /// CSV parse error.
        #[source]
        source: csv::Error,
    },
    /// The experiment contains no blocks.
    #[error("the experiment contains no blocks")]
    EmptyExperiment,
    /// A block contains no trials.
    #[error("block `{id}` contains no trials")]
    EmptyBlock {
        /// Block identifier.
        id: BlockId,
    },
    /// A stimulus row has an empty identifier.
    #[error("stimulus row {row} has an empty id")]
    EmptyStimulusId {
        /// 1-based CSV row, including the header.
        row: usize,
    },
    /// Two stimuli share an identifier.
    #[error("duplicate stimulus id `{id}`")]
    DuplicateStimulusId {
        /// Duplicated identifier.
        id: StimulusId,
    },
    /// The `kind` column is not `image` or `sequence`.
    #[error("stimulus row {row} has unknown kind `{value}`; use `image` or `sequence`")]
    InvalidKind {
        /// 1-based CSV row.
        row: usize,
        /// Offending value.
        value: String,
    },
    /// The duration is missing, non-numeric or zero.
    #[error("stimulus `{id}` has invalid duration `{value}`; use positive milliseconds")]
    InvalidDuration {
        /// Stimulus identifier.
        id: StimulusId,
        /// Offending value.
        value: String,
    },
    /// A sequence has no frame rate.
    #[error("sequence `{id}` requires an fps value")]
    MissingFps {
        /// Stimulus identifier.
        id: StimulusId,
    },
    /// A sequence has a non-numeric or non-positive frame rate.
    #[error("stimulus `{id}` has invalid fps `{value}`")]
    InvalidFps {
        /// Stimulus identifier.
        id: StimulusId,
        /// Offending value.
        value: String,
    },
    /// The loop flag is not a boolean.
    #[error("stimulus `{id}` has invalid loop value `{value}`; use true or false")]
    InvalidLoop {
        /// Stimulus identifier.
        id: StimulusId,
        /// Offending value.
        value: String,
    },
    /// An image row supplied an fps.
    #[error("image `{id}` must not specify fps")]
    ImageWithFps {
        /// Stimulus identifier.
        id: StimulusId,
    },
    /// An image row enabled looping.
    #[error("image `{id}` must not loop")]
    ImageWithLoop {
        /// Stimulus identifier.
        id: StimulusId,
    },
    /// An image row supplied an audio track.
    #[error("image `{id}` must not specify audio; audio is valid for sequences only")]
    ImageWithAudio {
        /// Stimulus identifier.
        id: StimulusId,
    },
    /// The path column is empty.
    #[error("stimulus `{id}` has an empty path")]
    MissingPath {
        /// Stimulus identifier.
        id: StimulusId,
    },
    /// The image file or sequence directory does not exist.
    #[error(
        "media for stimulus `{id}` was not found at `{path}`; an image must be a file and a sequence must be a directory"
    )]
    MissingMedia {
        /// Stimulus identifier.
        id: StimulusId,
        /// Resolved path.
        path: PathBuf,
    },
    /// The audio track does not exist.
    #[error("audio for stimulus `{id}` was not found at `{path}`")]
    MissingAudio {
        /// Stimulus identifier.
        id: StimulusId,
        /// Resolved path.
        path: PathBuf,
    },
    /// Shuffling was requested without a seed.
    #[error("shuffle is enabled but no seed was provided in Experiment.csv")]
    MissingSeed,
    /// An unknown key appeared in `Experiment.csv`.
    #[error("unknown key `{key}` in Experiment.csv")]
    UnknownMetadataKey {
        /// Unknown key.
        key: String,
    },
    /// A key appeared twice in `Experiment.csv`.
    #[error("duplicate key `{key}` in Experiment.csv")]
    DuplicateMetadataKey {
        /// Duplicated key.
        key: String,
    },
    /// A metadata value could not be parsed.
    #[error("invalid value `{value}` for `{key}` in Experiment.csv")]
    InvalidMetadataValue {
        /// Metadata key.
        key: String,
        /// Offending value.
        value: String,
    },
}

#[derive(Debug, Default)]
struct Metadata {
    id: String,
    experimenter: Option<String>,
    instructions: Option<String>,
    shuffle: bool,
    seed: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MetadataRow {
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize)]
struct StimulusRow {
    id: String,
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    fps: String,
    #[serde(default)]
    duration_ms: String,
    #[serde(default, rename = "loop")]
    looped: String,
    #[serde(default)]
    audio: String,
    #[serde(default)]
    block: String,
}

/// Load `Experiment.csv` and `Stimuli.csv` from an experiment directory.
pub fn load(root: &Path) -> Result<Experiment, ExperimentError> {
    let metadata = load_metadata(&root.join("Experiment.csv"))?;
    let stimuli = load_stimuli(&root.join("Stimuli.csv"), root)?;

    let mut blocks: Vec<Block> = Vec::new();
    for (block_id, stimulus) in stimuli {
        if let Some(block) = blocks.iter_mut().find(|block| block.id == block_id) {
            block.trials.push(Trial::new(stimulus));
        } else {
            blocks.push(Block::new(block_id, vec![Trial::new(stimulus)]));
        }
    }

    let experiment = Experiment {
        id: if metadata.id.trim().is_empty() {
            "unnamed".to_owned()
        } else {
            metadata.id
        },
        experimenter: metadata.experimenter,
        instructions: metadata.instructions,
        blocks,
        shuffle: metadata.shuffle,
        seed: metadata.seed,
    };
    experiment.validate()?;
    Ok(experiment)
}

fn load_metadata(path: &Path) -> Result<Metadata, ExperimentError> {
    if !path.exists() {
        return Ok(Metadata::default());
    }

    let file = File::open(path).map_err(|source| ExperimentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(file);
    let mut metadata = Metadata::default();
    let mut seen = HashSet::new();

    for row in reader.deserialize::<MetadataRow>() {
        let row = row.map_err(|source| ExperimentError::Csv {
            path: path.to_path_buf(),
            source,
        })?;
        let key = row.key.trim().to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            return Err(ExperimentError::DuplicateMetadataKey { key });
        }
        match key.as_str() {
            "id" => {
                if row.value.trim().is_empty() {
                    return Err(ExperimentError::InvalidMetadataValue {
                        key,
                        value: row.value,
                    });
                }
                metadata.id = row.value.trim().to_owned();
            }
            "experimenter" => {
                metadata.experimenter = non_empty(row.value);
            }
            "instructions" => {
                metadata.instructions = non_empty(row.value);
            }
            "shuffle" => {
                metadata.shuffle =
                    parse_bool(&row.value).ok_or(ExperimentError::InvalidMetadataValue {
                        key,
                        value: row.value,
                    })?;
            }
            "seed" => {
                let value = row.value.trim();
                metadata.seed = Some(value.parse::<u64>().map_err(|_| {
                    ExperimentError::InvalidMetadataValue {
                        key,
                        value: value.to_owned(),
                    }
                })?);
            }
            _ => return Err(ExperimentError::UnknownMetadataKey { key }),
        }
    }

    if metadata.shuffle && metadata.seed.is_none() {
        return Err(ExperimentError::MissingSeed);
    }
    Ok(metadata)
}

fn load_stimuli(path: &Path, root: &Path) -> Result<Vec<(BlockId, Stimulus)>, ExperimentError> {
    let file = File::open(path).map_err(|source| ExperimentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(file);
    let mut stimuli = Vec::new();
    let mut seen = HashSet::new();

    for (index, row) in reader.deserialize::<StimulusRow>().enumerate() {
        let row = row.map_err(|source| ExperimentError::Csv {
            path: path.to_path_buf(),
            source,
        })?;
        stimuli.push(parse_stimulus(row, index + 2, root, &mut seen)?);
    }

    if stimuli.is_empty() {
        return Err(ExperimentError::EmptyExperiment);
    }
    Ok(stimuli)
}

fn parse_stimulus(
    row: StimulusRow,
    row_number: usize,
    root: &Path,
    seen: &mut HashSet<StimulusId>,
) -> Result<(BlockId, Stimulus), ExperimentError> {
    let id_text = row.id.trim();
    if id_text.is_empty() {
        return Err(ExperimentError::EmptyStimulusId { row: row_number });
    }
    let id = StimulusId::new(id_text);
    if !seen.insert(id.clone()) {
        return Err(ExperimentError::DuplicateStimulusId { id });
    }

    let kind = match row.kind.trim().to_ascii_lowercase().as_str() {
        "image" => StimulusKind::Image,
        "sequence" => StimulusKind::Sequence,
        value => {
            return Err(ExperimentError::InvalidKind {
                row: row_number,
                value: value.to_owned(),
            })
        }
    };

    let path_text = row.path.trim();
    if path_text.is_empty() {
        return Err(ExperimentError::MissingPath { id });
    }
    let path = resolve(root, path_text);

    let duration_value = row.duration_ms.trim();
    let duration_ms =
        duration_value
            .parse::<u64>()
            .map_err(|_| ExperimentError::InvalidDuration {
                id: id.clone(),
                value: duration_value.to_owned(),
            })?;
    if duration_ms == 0 {
        return Err(ExperimentError::InvalidDuration {
            id,
            value: duration_value.to_owned(),
        });
    }

    let fps = if row.fps.trim().is_empty() {
        None
    } else {
        let value = row.fps.trim();
        let fps = value
            .parse::<f32>()
            .map_err(|_| ExperimentError::InvalidFps {
                id: id.clone(),
                value: value.to_owned(),
            })?;
        if !fps.is_finite() || fps <= 0.0 {
            return Err(ExperimentError::InvalidFps {
                id: id.clone(),
                value: value.to_owned(),
            });
        }
        Some(fps)
    };

    let looped = parse_bool(&row.looped).ok_or_else(|| ExperimentError::InvalidLoop {
        id: id.clone(),
        value: row.looped.trim().to_owned(),
    })?;

    let audio = if row.audio.trim().is_empty() {
        None
    } else {
        let audio = resolve(root, row.audio.trim());
        if !audio.is_file() {
            return Err(ExperimentError::MissingAudio {
                id: id.clone(),
                path: audio,
            });
        }
        Some(audio)
    };

    let block = if row.block.trim().is_empty() {
        BlockId::new("default")
    } else {
        BlockId::new(row.block.trim())
    };

    match kind {
        StimulusKind::Image => {
            if fps.is_some() {
                return Err(ExperimentError::ImageWithFps { id });
            }
            if looped {
                return Err(ExperimentError::ImageWithLoop { id });
            }
            if audio.is_some() {
                return Err(ExperimentError::ImageWithAudio { id });
            }
            if !path.is_file() {
                return Err(ExperimentError::MissingMedia { id, path });
            }
        }
        StimulusKind::Sequence => {
            if fps.is_none() {
                return Err(ExperimentError::MissingFps { id });
            }
            if !path.is_dir() {
                return Err(ExperimentError::MissingMedia { id, path });
            }
        }
    }

    Ok((
        block,
        Stimulus {
            id,
            kind,
            path,
            duration: Duration::from_millis(duration_ms),
            fps,
            looped,
            audio,
        },
    ))
}

fn resolve(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "false" | "0" => Some(false),
        "true" | "1" => Some(true),
        _ => None,
    }
}
