# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 and follows semantic versioning for its public crates.

## [Unreleased]

### Added

- Cargo workspace with the `yet-present` and `yet-core` crates.
- `yet-present`: `SessionTime`, physical-pixel geometry, `Scene`/`DrawCommand`,
  presenter, overlay, media-sequence and audio traits, and `RecordingPresenter`.
- `yet-core`: a pure `step` engine with start/pause/resume/abort/tick effects,
  `TestClock`, and the calibration data types.
- GitHub Actions CI for Linux, macOS, Windows and MSRV 1.88.
- README, ROADMAP and CHANGELOG.

### Changed

- Project license changed from MIT to GPL-3.0-only, following the requirements
  recorded in `UI_analysis.md`.
