// SPDX-License-Identifier: GPL-3.0-only

//! Compatibility loader for a `yet_zero` (Python) workspace.
//!
//! A Python workspace contains `Config.csv` and `Stimuli/Stimuli.csv` instead
//! of `config.toml` and a native `Stimuli.csv`. The presentation model is the
//! same flat slideshow the prototype implements: every row is an image shown
//! for `SLIDE_TIME` seconds, optionally shuffled. Byte-identical CSV output is
//! preserved because the stimulus id is the `File` value from `Stimuli.csv`.
//!
//! Machine settings are derived from `Config.csv`. If the directory also
//! contains a native `config.toml`, that file wins for display, presenter,
//! overlay and audio settings; `Config.csv` still supplies the camera index
//! unless the TOML explicitly contains `camera.usb_index`.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use thiserror::Error;

use crate::config::{
    AppConfig, AudioConfig, CameraConfig, ConfigError, DisplayConfig, OverlayConfig, OverlayMode,
    PresenterBackend, PresenterConfig,
};
use crate::experiment::{
    Block, Experiment, ExperimentError, Stimulus, StimulusId, StimulusKind, Trial,
};
use crate::plan::{self, SessionPlan};

/// Errors from loading a Python `yet_zero` workspace.
#[derive(Debug, Error)]
pub enum LegacyError {
    /// A required file could not be read.
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
    /// A configuration key occurs more than once.
    #[error("duplicate key `{key}` in `{path}`")]
    DuplicateKey {
        /// Path to `Config.csv`.
        path: PathBuf,
        /// Duplicated key.
        key: String,
    },
    /// A configuration value is invalid.
    #[error("invalid value `{value}` for `{key}` in `{path}`")]
    InvalidValue {
        /// Path to `Config.csv`.
        path: PathBuf,
        /// Configuration key.
        key: String,
        /// Supplied value.
        value: String,
    },
    /// The native machine configuration is invalid TOML.
    #[error("invalid TOML in `{path}`: {source}")]
    Toml {
        /// Attempted path.
        path: PathBuf,
        /// TOML parse error.
        #[source]
        source: Box<toml::de::Error>,
    },
    /// The stimulus table is missing.
    #[error("`{path}` was not found; expected a `Stimuli/Stimuli.csv` table")]
    MissingStimuli {
        /// Expected path.
        path: PathBuf,
    },
    /// The stimulus table has no rows.
    #[error("`{path}` contains no stimuli")]
    EmptyStimuli {
        /// Path to the stimulus table.
        path: PathBuf,
    },
    /// A stimulus row has an empty file name.
    #[error("stimulus row {row} in `{path}` has an empty File value")]
    EmptyFile {
        /// Path to the stimulus table.
        path: PathBuf,
        /// 1-based row number, including the header.
        row: usize,
    },
    /// Two rows refer to the same file.
    #[error("duplicate stimulus file `{id}` in `{path}`")]
    DuplicateStimulus {
        /// Path to the stimulus table.
        path: PathBuf,
        /// Duplicated file name.
        id: String,
    },
    /// A stimulus image is missing.
    #[error(transparent)]
    Experiment(#[from] ExperimentError),
    /// The native machine configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A shuffle seed could not be derived from the system clock.
    #[error("could not derive a shuffle seed from the system clock")]
    Seed,
}

/// A loaded Python workspace, before it is wrapped into [`crate::LoadedExperiment`].
pub struct LegacyWorkspace {
    /// Machine configuration derived from `Config.csv` and an optional TOML.
    pub config: AppConfig,
    /// Flat slideshow experiment.
    pub experiment: Experiment,
    /// Ordered session plan.
    pub plan: SessionPlan,
    /// Default result directory (`Data/`).
    pub result_dir: PathBuf,
}

impl fmt::Debug for LegacyWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LegacyWorkspace")
            .field("config", &self.config)
            .field("experiment", &self.experiment)
            .field("plan", &self.plan)
            .field("result_dir", &self.result_dir)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    #[serde(rename = "Config", alias = "config", alias = "\u{feff}Config")]
    key: String,
    #[serde(rename = "Value", alias = "value", alias = "\u{feff}Value", default)]
    value: String,
}

#[derive(Debug, Deserialize)]
struct StimulusRow {
    #[serde(rename = "File", alias = "file", alias = "\u{feff}File")]
    file: String,
}

/// Load a Python workspace.
pub fn load(root: &Path) -> Result<LegacyWorkspace, LegacyError> {
    let config_path = root.join("Config.csv");
    let settings = load_config_csv(&config_path)?;

    let usb_index = parse_u32(&settings, "USB", &config_path, 0)? as usize;
    let width = parse_u32(&settings, "WIDTH", &config_path, 800)?;
    let height = parse_u32(&settings, "HEIGHT", &config_path, 600)?;
    if width == 0 || height == 0 {
        return Err(LegacyError::InvalidValue {
            path: config_path,
            key: "WIDTH/HEIGHT".to_owned(),
            value: format!("{width}x{height}"),
        });
    }
    let slide_seconds = parse_f64(&settings, "SLIDE_TIME", &config_path, 3.0)?;
    if !slide_seconds.is_finite() || slide_seconds <= 0.0 || slide_seconds > 86_400.0 {
        return Err(LegacyError::InvalidValue {
            path: config_path,
            key: "SLIDE_TIME".to_owned(),
            value: slide_seconds.to_string(),
        });
    }
    let duration = Duration::from_secs_f64(slide_seconds);
    let shuffle = parse_bool(&settings, "SHUFFLE", &config_path, false)?;
    let explicit_seed = match settings.get("SEED") {
        Some(value) if !value.trim().is_empty() => Some(value.trim().parse::<u64>().map_err(
            |_| LegacyError::InvalidValue {
                path: config_path.clone(),
                key: "SEED".to_owned(),
                value: value.clone(),
            },
        )?),
        _ => None,
    };
    let seed = if shuffle {
        Some(match explicit_seed {
            Some(seed) => seed,
            None => system_seed()?,
        })
    } else {
        explicit_seed
    };

    let stem = settings
        .get("EXP_ID")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("unnamed")
        .to_owned();
    let experimenter = settings
        .get("EXPERIMENTER")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let stimuli_dir = root.join("Stimuli");
    let stimuli_csv = stimulus_table(root, &stimuli_dir, &settings)?;
    let stimuli = load_stimuli(&stimuli_csv, &stimuli_dir, duration)?;
    let experiment = Experiment {
        id: stem,
        experimenter,
        instructions: None,
        blocks: vec![Block::new("default", stimuli)],
        shuffle,
        seed,
    };
    let plan = plan::build(&experiment)?;

    let config = machine_config(root, &settings, usb_index, width, height)?;

    Ok(LegacyWorkspace {
        config,
        experiment,
        plan,
        result_dir: root.join("Data"),
    })
}

fn load_config_csv(path: &Path) -> Result<BTreeMap<String, String>, LegacyError> {
    let file = fs::File::open(path).map_err(|source| LegacyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(file);
    let mut settings = BTreeMap::new();
    for row in reader.deserialize::<ConfigRow>() {
        let row = row.map_err(|source| LegacyError::Csv {
            path: path.to_path_buf(),
            source,
        })?;
        let key = row.key.trim().to_ascii_uppercase();
        if key.is_empty() {
            continue;
        }
        if settings.insert(key.clone(), row.value).is_some() {
            return Err(LegacyError::DuplicateKey {
                path: path.to_path_buf(),
                key,
            });
        }
    }
    Ok(settings)
}

fn stimulus_table(
    root: &Path,
    stimuli_dir: &Path,
    settings: &BTreeMap<String, String>,
) -> Result<PathBuf, LegacyError> {
    let requested = settings
        .get("STIM_FILE")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Stimuli.csv");
    let requested_path = Path::new(requested);

    let candidates = if requested_path.is_absolute() {
        vec![requested_path.to_path_buf()]
    } else {
        vec![stimuli_dir.join(requested_path), root.join(requested_path)]
    };
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Ok(path);
    }

    // Python ignores STIM_FILE and always reads Stimuli/Stimuli.csv. Keep that
    // as the final fallback so a stale key does not break an otherwise valid
    // workspace.
    let default = stimuli_dir.join("Stimuli.csv");
    if default.is_file() {
        return Ok(default);
    }
    Err(LegacyError::MissingStimuli { path: default })
}

fn load_stimuli(
    path: &Path,
    stimuli_dir: &Path,
    duration: Duration,
) -> Result<Vec<Trial>, LegacyError> {
    let file = fs::File::open(path).map_err(|source| LegacyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(file);
    let mut trials = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (index, row) in reader.deserialize::<StimulusRow>().enumerate() {
        let row = row.map_err(|source| LegacyError::Csv {
            path: path.to_path_buf(),
            source,
        })?;
        let file_name = row.file.trim();
        if file_name.is_empty() {
            return Err(LegacyError::EmptyFile {
                path: path.to_path_buf(),
                row: index + 2,
            });
        }
        let id = StimulusId::new(file_name);
        if !seen.insert(id.clone()) {
            return Err(LegacyError::DuplicateStimulus {
                path: path.to_path_buf(),
                id: file_name.to_owned(),
            });
        }
        let media = stimuli_dir.join(file_name);
        if !media.is_file() {
            return Err(ExperimentError::MissingMedia { id, path: media }.into());
        }
        trials.push(Trial::new(Stimulus {
            id,
            kind: StimulusKind::Image,
            path: media,
            duration,
            fps: None,
            looped: false,
            audio: None,
        }));
    }

    if trials.is_empty() {
        return Err(LegacyError::EmptyStimuli {
            path: path.to_path_buf(),
        });
    }
    Ok(trials)
}

fn machine_config(
    root: &Path,
    settings: &BTreeMap<String, String>,
    usb_index: usize,
    width: u32,
    height: u32,
) -> Result<AppConfig, LegacyError> {
    let toml_path = root.join("config.toml");
    if toml_path.is_file() {
        let mut config = crate::config::load(&toml_path)?;
        if !toml_has_camera_index(&toml_path)? {
            config.camera.usb_index = usb_index;
        }
        return Ok(config);
    }

    let overlay_monitor = settings
        .get("OVERLAY_MONITOR")
        .and_then(|value| value.trim().parse().ok());
    Ok(AppConfig {
        // A Python workspace has no monitor selection and the prototype is a
        // 900x900 window, not a fullscreen surface. Add config.toml for a
        // multi-monitor fullscreen rig.
        display: DisplayConfig {
            presenter_monitor: 0,
            console_monitor: 0,
            width,
            height,
            fullscreen: false,
        },
        camera: CameraConfig {
            usb_index,
            ..CameraConfig::default()
        },
        presenter: PresenterConfig {
            backend: PresenterBackend::Egui,
            // The prototype has no desktop overlay.
            overlay: OverlayMode::Off,
        },
        overlay: OverlayConfig {
            monitor: overlay_monitor,
        },
        audio: AudioConfig::default(),
    })
}

fn toml_has_camera_index(path: &Path) -> Result<bool, LegacyError> {
    let text = fs::read_to_string(path).map_err(|source| LegacyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: toml::Value = toml::from_str(&text).map_err(|source| LegacyError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(value
        .get("camera")
        .and_then(|camera| camera.get("usb_index"))
        .is_some())
}

fn parse_u32(
    settings: &BTreeMap<String, String>,
    key: &str,
    path: &Path,
    default: u32,
) -> Result<u32, LegacyError> {
    match settings.get(key) {
        Some(value) if !value.trim().is_empty() => {
            value
                .trim()
                .parse::<u32>()
                .map_err(|_| LegacyError::InvalidValue {
                    path: path.to_path_buf(),
                    key: key.to_owned(),
                    value: value.clone(),
                })
        }
        _ => Ok(default),
    }
}

fn parse_f64(
    settings: &BTreeMap<String, String>,
    key: &str,
    path: &Path,
    default: f64,
) -> Result<f64, LegacyError> {
    match settings.get(key) {
        Some(value) if !value.trim().is_empty() => {
            value
                .trim()
                .parse::<f64>()
                .map_err(|_| LegacyError::InvalidValue {
                    path: path.to_path_buf(),
                    key: key.to_owned(),
                    value: value.clone(),
                })
        }
        _ => Ok(default),
    }
}

fn parse_bool(
    settings: &BTreeMap<String, String>,
    key: &str,
    path: &Path,
    default: bool,
) -> Result<bool, LegacyError> {
    let Some(value) = settings.get(key) else {
        return Ok(default);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default),
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(LegacyError::InvalidValue {
            path: path.to_path_buf(),
            key: key.to_owned(),
            value: value.clone(),
        }),
    }
}

fn system_seed() -> Result<u64, LegacyError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .map_err(|_| LegacyError::Seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_legacy_bool_accepts_python_and_text_forms() {
        let mut settings = BTreeMap::new();
        let path = Path::new("Config.csv");

        settings.insert("SHUFFLE".to_owned(), "1".to_owned());
        assert!(parse_bool(&settings, "SHUFFLE", path, false).unwrap());
        settings.insert("SHUFFLE".to_owned(), "0".to_owned());
        assert!(!parse_bool(&settings, "SHUFFLE", path, false).unwrap());
        settings.insert("SHUFFLE".to_owned(), "true".to_owned());
        assert!(parse_bool(&settings, "SHUFFLE", path, false).unwrap());
        settings.insert("SHUFFLE".to_owned(), "maybe".to_owned());
        assert!(parse_bool(&settings, "SHUFFLE", path, false).is_err());
    }
}
