# Connectivity: linking Yet gaze to lab equipment

**Status:** analysis and decision record, 2026-10-31. No code changes.
**Scope:** how `yet-rs` can publish gaze predictions and session events to
Lab Streaming Layer (LSL), OpenSesame and PsychoPy.
**Out of scope:** implementation, fixation/saccade detection, binocular or
world-camera output, and the `yet_zero` Python prototype.

---

## 1. Executive summary

LSL is the right integration boundary. It is the de-facto standard in
psychophysiology, it gives us discovery, a shared clock and a recorder
(LabRecorder/XDF), and it lets OpenSesame and PsychoPy participate without a
Yet-specific plugin. The Rust side is feasible today: the pure-Rust
`labstream` crates expose exactly the calls Yet needs, including
`push_at`/`push_text_at` for caller-measured timestamps.

The main risk is not the transport. It is **timing semantics**:

- Yet currently timestamps a gaze sample at the controller tick, not at camera
  exposure. The camera mailbox discards capture time.
- Trial-start markers emitted at the engine tick lead the actual stimulus
  presentation by the presenter/display latency.
- Audio playback position has a measured lag (≈128 ms on the Linux workstation)
  that must be compensated if audio is used as the alignment event.

Those are prerequisites for scientifically useful LSL output. They also improve
the CSV independent of LSL.

OpenSesame and PsychoPy are **high-feasibility LSL consumers/producers**, not
direct integration targets. Both are Python applications; neither needs a native
Yet binding. Use `pylsl` in an `inline_script` (OpenSesame) or a Code Component
(PsychoPy), and let LabRecorder merge streams. A native plugin or PyO3 binding
would add coupling and maintenance without solving clock sync or recording.

| Option | Feasibility | Effort | Verdict |
|---|---|---|---|
| LSL outlet from Yet (`labstream`) | High, with a timing spike | Medium | **Recommended** |
| LSL via official `lsl`/liblsl bindings | High | Medium | Fallback only |
| OpenSesame direct integration | Low value | High | **Do not build** |
| OpenSesame via LSL/`pylsl` | High | Low on Python side | **Recommended** |
| PsychoPy direct integration | Low value | High | **Do not build** |
| PsychoPy via LSL/`pylsl` | High | Low on Python side | **Recommended** |
| ZeroMQ/OSC/UDP-only transport | Medium | Low | Lab-specific fallback |
| Write XDF directly from Yet | Low value | Medium | **Do not build** |

---

## 2. Lab Streaming Layer

### 2.1 What LSL gives us

LSL is an overlay network for time series and event streams. For Yet it
provides four things that a raw socket or CSV handoff does not:

1. **Discovery.** Consumers resolve streams by name, type or source id.
2. **A shared time base.** LSL timestamps are in the LSL clock domain and are
   corrected for network jitter on the inlet side.
3. **A standard recorder.** LabRecorder writes all resolved streams to XDF,
   preserving per-stream timestamps and metadata.
4. **An ecosystem.** `pylsl`, OpenSesame, PsychoPy, EEG packages, MATLAB and
   `pyxdf` already speak it.

LSL is a transport, not a file format. Yet should keep its CSV as the primary
analysis output and let LabRecorder produce XDF for multimodal alignment. XDF
readers are strongest in Python (`pyxdf`) and MATLAB; R support is less mature,
so the existing R pipeline should continue to read the CSV.

### 2.2 The Rust options

| Crate | Version / age | Model | Assessment |
|---|---|---|---|
| `labstream` | 0.1.2, Sep 2026 | Pure Rust API over `labstream-net` | **Preferred** high-level API |
| `labstream-core` | 0.1.2, Sep 2026 | Facade; no code, re-exports four crates | Use only if protocol-level access is needed |
| `labstream-net` | 0.1.2, Sep 2026 | Sockets, outlet/inlet, resolver, clock | The actual implementation (~4,255 LOC) |
| `lsl`, `lsl-sys` | 0.1.1, 2020 | Bindings to system liblsl, bindgen | Stale; needs liblsl + C++ toolchain |
| `lamquant-lsl` | 0.1.2, 2026 | Fork of `liblsl-rust` | Low adoption; not preferred |

`labstream-core` is a meta-crate. crates.io reports only 7 lines of Rust for it
because the implementation lives in `labstream-net`, `labstream-wire`,
`labstream-proto` and `labstream-time`. The link in the request therefore points
at a facade, not at the implementation. The high-level API is `labstream`.

At the `labstream-core` level the same publish operation is a
`Sample { timestamp, values }` plus `outlet.push(&sample)`; the high-level crate
is an ergonomic layer over that. For Yet, `labstream` is the better dependency.
Use `labstream-core` only if we need a protocol detail that the high-level API
hides.

The `labstream` project is MIT licensed, has an MSRV of 1.75 (compatible with
Yet's 1.95), and claims LSL protocol 1.10 conformance measured against liblsl.
Its README reports 165/165 C symbols, 21/21 liblsl examples, 42/42 XPath
queries, a 7.81-hour recording and 252 tests. These are **self-reported** and
should be independently verified with `pylsl` and LabRecorder before a
publication or a study depends on them.

The project is young: 0.1.x, one maintainer, 46 commits, no broad adoption.
That is a bus-factor risk, not a reason to reject it. The same README states
that CI runs on Linux, macOS and Windows, but only Linux carries the full
conformance measurement; macOS has one ignored test. Yet's own platform matrix
already treats macOS/Windows as less validated, so this is compatible with a
Linux-first rollout.

The older `lsl` crate is the official `labstreaminglayer/liblsl-rust` binding.
It is stale and requires a system liblsl plus a C/C++ build step, which
contradicts Yet's "no C++ build step" design goal. Keep it as a fallback if the
pure-Rust implementation fails conformance on a target platform.

### 2.3 API fit

The high-level API maps cleanly onto Yet:

```rust
// Sketch only; not implemented.
let gaze_info = StreamInfo::builder("Yet Gaze", "Gaze", Format::Float32)
    .rate(camera_fps)
    .source_id("yet-zero/<host>/<camera>")
    .channel_labels(["x_screen", "y_screen", "x_norm", "y_norm", "valid"])
    .build()?;
let gaze_outlet = Outlet::new(gaze_info)?;
gaze_outlet.push_at(&[x_screen, y_screen, x_norm, y_norm, valid], lsl_time)?;

let event_info = StreamInfo::builder("Yet Events", "Markers", Format::String)
    .irregular()
    .source_id("yet-zero-events/<host>/<session>")
    .channel_labels(["marker"])
    .build()?;
let event_outlet = Outlet::new(event_info)?;
event_outlet.push_text_at("trial_start;block=1;trial=3;stim=face_03", lsl_time)?;
```

Relevant calls, verified in the 0.1.2 docs:

- `StreamInfo::builder(name, type, format).rate(hz).irregular().source_id(..)
  .channel_labels(..).channels(..).build()`
- `Outlet::new(info)`, `push`, `push_at`, `push_chunk`, `push_text`,
  `push_text_at`
- `clock()` for the local LSL clock
- `Outlet` is `Send + Sync`

Two limitations matter:

1. **No documented arbitrary `desc` XML** in the high-level builder. It exposes
   name, type, rate, source id, channel labels and channels only. Rich metadata
   must go into the stream name/source id or into a `session_start` marker (see
   §2.5), or we must verify whether `labstream-net::StreamInfo` exposes the
   description tree directly.
2. **No stream-type registry.** `Gaze` and `Markers` are conventions, not
   enforced schemas. Consumers such as LabRecorder record what we publish; they
   do not validate channel meanings. The channel contract must be documented
   and versioned by us.

### 2.4 What Yet can publish today

From `crates/yet-stim/src/engine.rs` and `crates/yet-stim/src/data.rs`:

- `FrameRecord` carries `at: SessionTime`, `block`, `trial_index`, `stimulus`,
  `trial_started_at`, and `gaze: Option<GazePoint>`.
- `GazeSample` carries `at`, `pixel` (stimulus pixels) and `normalized`
  (stimulus-relative 0..1).
- The controller loop in `crates/yet-zero/src/console.rs` polls the camera at
  60 Hz; a new prediction is produced only when the camera capture counter
  advances. The demo camera negotiates 25.6 fps.
- The CSV sink writes `Exp,Part,Stim,time,x,y,x_pro,y_pro`; times are epoch
  seconds derived from a session-start epoch plus `SessionTime`.

What is missing for a scientific stream:

- **Capture time.** `Mailbox` stores only `GrayFrame`; `Camera::latest_frame()`
  returns only the frame. `CameraFrame.captured_at` exists in the camera types
  but is not propagated to the predictor or the CSV. The sample is stamped at
  the controller tick (`FrameRecord.at`).
- **Presentation time.** Trial-start is an engine effect at the tick. The
  presenter knows `FrameStamp.presented_at` (observed or estimated), but the
  event path does not use it.
- **Validity/confidence.** The model either has a fitted prediction or not.
  There is no gaze confidence, blink flag, pupil size or fixation state.
- **Stimulus context in the sample.** Block/trial/stimulus are in the event
  path, not in the gaze sample.
- **Monitor-space output.** The predictor produces a monitor-space point and
  `gaze_sample()` maps it to stimulus pixels. The monitor point is currently
  not exposed outside the closure.

### 2.5 Proposed LSL stream contract (v1)

Two streams are enough for offline merging. A third status stream is optional.

#### Stream 1: Gaze

| Property | Value |
|---|---|
| Name | `Yet Gaze <participant>` |
| Type | `Gaze` |
| Format | `float32` |
| Rate | Camera nominal fps (e.g. 25.6), or irregular if the camera is variable |
| Source id | `yet-zero/<host>/<camera>/<model-format-version>` |
| Channels | `x_screen`, `y_screen`, `x_norm`, `y_norm`, `valid` |
| Optional context | `block`, `trial` (float32 integer-valued) |

- `x_screen`/`y_screen` are monitor pixels. They are the only portable
  coordinates when an external program (OpenSesame/PsychoPy) presents stimuli
  and Yet does not know the stimulus rectangle.
- `x_norm`/`y_norm` are stimulus-relative 0..1 and are meaningful only during a
  Yet-presented trial. Outside a trial they can be `NaN`.
- `valid` is `1.0` for a fresh prediction. Reserve `0.0` for future blink or
  model-invalid samples; do not publish repeated stale values.
- Publish only actual predictions. If the model is not fitted or frames stop,
  emit `gaze_unavailable`/`gaze_resumed` markers rather than holding the last
  value.
- If block/trial context is included, consumers can label samples without
  joining the event stream. This is convenient but duplicates state; keep it
  optional until a consumer needs it.

#### Stream 2: Events

| Property | Value |
|---|---|
| Name | `Yet Events <participant>` |
| Type | `Markers` |
| Format | `string` |
| Rate | irregular |
| Source id | `yet-zero-events/<host>/<session>` |
| Channel | one string channel |

Suggested vocabulary, versioned as `yet.lsl.v1`:

| Marker | Payload |
|---|---|
| `session_start` | JSON: schema, experiment, participant (pseudonymous), camera, monitor geometry, model name/version, `t0_unix` |
| `calibration_start` | method (`full`/`quick`) |
| `calibration_end` | RMSE in pixels, target count |
| `trial_start` | block, trial, stimulus id, planned duration |
| `trial_onset` | presenter-observed/estimated onset (see §2.6) |
| `trial_end` | completed flag, lateness |
| `pause` / `resume` | session time |
| `gaze_unavailable` / `gaze_resumed` | reason (`no_frame`, `not_fitted`, `camera_error`) |
| `session_end` | reason (`finished`, `aborted`, `error`) |

String markers are the most portable choice. If a particular EEG amplifier or
trigger box requires numeric codes, add a separate `int32` trigger stream rather
than overloading the string stream. Do not make 8-bit numeric triggers the
primary contract; they lose context and collide across experiments.

#### Stream 3 (optional): Status

At 1 Hz, `float32`: `capture_fps`, `prediction_rate_hz`, `dropped_frames`,
`calibration_rmse_px`, `consumer_count`. Useful for QC and for the console, but
not required for analysis.

### 2.6 Timestamps and latency

This is the part that determines whether the integration is scientifically
useful.

**Clock domain.** `SessionTime` is monotonic since session start. LSL timestamps
must be in the LSL clock domain (`labstream::clock()`), which is monotonic and
**not guaranteed to be Unix epoch**. Do not derive LSL time from `epoch_start`.
Sample a single anchor `(instant, lsl_clock)` at session start and map:

```text
lsl_time = anchor_lsl + (sample_session_time - anchor_session_time)
```

Both clocks are monotonic, so the offset is constant unless the machine sleeps
or the clock source changes. For cross-machine recordings, LSL's inlet-side
clock correction handles the remaining offset; verify it with a common event.
Use the same `SessionTime` value that is written to the CSV, not a
pause-subtracted experiment time; otherwise the two files will diverge.

**Capture vs delivery.** A sample stamped at the controller tick can be up to
one tick (16.7 ms) plus one frame interval (39.1 ms at 25.6 fps) stale, before
USB/decode/predict time. For offline analysis, only the timestamp accuracy
matters; for online gaze-contingent use, the delivery latency matters too. The
current pipeline cannot distinguish the two. The first implementation step must
therefore be:

1. Carry a capture timestamp (or at least a capture `Instant`) through the
   camera mailbox, `Tracker::poll`, and the gaze path.
2. Use that timestamp for LSL, and optionally for the CSV.
3. Measure end-to-end latency (frame arrival to prediction, prediction to LSL
   push) and publish it as status, not as a guess.

**Presentation time.** `trial_start` at the engine tick leads the actual
stimulus. For stimulus-locked EEG/ERP work, emit `trial_onset` from the
presenter's `FrameStamp.presented_at` (or the best available estimate), not from
the tick. If the display's flip latency is unknown, measure it once with a
photodiode or a high-speed camera.

**Audio.** The README records `get_pos()` about 128 ms behind the wall clock on
Linux, and the audio path has an `audio.output_latency_ms` setting. If audio is
the alignment event, the marker must be placed at the estimated acoustic onset,
not at the `play` call; otherwise the marker leads the sound.

**Budget.** At 25.6 fps, the sampling interval alone is 39 ms. A rough
worst-case age at the consumer is 55–80 ms before network and recorder
overheads. That is adequate for offline covariate analysis and slow
gaze-contingent designs; it is not adequate for saccade-contingent or
ERP-locked triggering. State that explicitly in the study documentation.

### 2.7 Metadata, privacy and failure modes

- **No descriptor XML** in the documented high-level API. Put schema, model,
  experiment, participant and monitor geometry into the `session_start`
  marker. If `labstream-net` exposes a description tree, revisit this.
- **Participant identity.** LSL streams are discoverable on the local network.
  Stream names and `session_start` payloads can leak participant ids. Use
  pseudonymous codes by default, make LSL opt-in, and document the network
  scope. LSL has no authentication or encryption; treat it as a lab-LAN
  protocol. Never publish camera frames or the raw eye ROI over LSL by default.
- **No consumer.** Publishing with zero consumers is harmless but wastes work
  and can hide a misconfiguration. Show `consumer_count` in the console; do not
  stop the experiment if it is zero unless the protocol requires a recorder.
- **Outlet failure.** If the outlet cannot be created or the publisher thread
  dies, log a warning and continue with CSV. Never abort a session because the
  network is down.
- **Backpressure.** Use a bounded queue to a dedicated publisher thread. LSL
  push is local and cheap at 25 Hz, but the publisher must not add jitter to the
  60 Hz controller. On overflow, drop and count; emit a diagnostic event.
- **Recorder ordering.** LabRecorder should be recording before the session
  starts for a no-loss recording, or the session should wait for a consumer.
  Document the procedure; do not assume the recorder started first.
- **License.** `labstream` is MIT, compatible with Yet's GPL-3.0-only. Add the
  notice to `THIRD_PARTY_LICENSES.md`.

### 2.8 Feasibility verdict

**Go, with a spike.** The crate API fits, the license is compatible, and the
pure-Rust implementation preserves Yet's no-C++ build goal. The spike must
prove four things before this becomes a product feature:

1. `pylsl` can discover and read the gaze and event streams on the lab machine.
2. LabRecorder records both streams to XDF without loss for at least 10 minutes
   at 25.6 Hz.
3. Timestamps remain monotonic and align with a common flash/click event within
   one frame.
4. Outlet creation and publishing do not perturb controller tick timing.

If any of these fail, fall back to the official `lsl` bindings or to a
`labstream-capi` + `pylsl` bridge, not to a bespoke socket protocol.

### 2.9 Validation plan

| Level | What to prove |
|---|---|
| Unit | `GazeSink` contract with a fake clock and fake sink; no network |
| Loopback | `labstream` outlet → `pylsl` inlet on one machine; count, labels, timestamps, no loss |
| Recorder | LabRecorder → XDF → `pyxdf`; stream metadata, sample counts, event alignment |
| Cross-machine | Clock offset and jitter between two lab machines; LSL timestamp correction |
| Hardware | Common flash/click/photodiode event; offset target ≤ one frame |
| Failure | No network, no consumer, recorder restart, camera stop; session continues |
| Soak | 10–60 minutes; memory, queue depth, drops, controller lateness |

Keep the loopback test in CI only if a Python environment with `pylsl` is
available; otherwise keep it as a documented manual gate like the existing
camera hardware tests.

---

## 3. OpenSesame

**Current stable:** 4.1 (checked 2026-10-31). Python/Qt experiment builder with
a graphical item list and a Python API.

**What exists for eye tracking:** OpenSesame's eye-tracking support is PyGaze,
with backends for EyeLink, EyeLogic, EyeTribe, GazePoint/OpenGaze, SMI, Tobii and
WebGazer. There is no Yet backend, and the built-in item list does not include an
LSL item. The practical integration is `pylsl` in an `inline_script`, or a
community LSL plugin if one is present in the installation. Do not make the
latter a dependency.

**Two topologies:**

| Topology | Presenter | Recorder | Yet's role | What Yet needs |
|---|---|---|---|---|
| A | Yet | LabRecorder | presenter + tracker | LSL outlet only |
| B | OpenSesame | LabRecorder | tracker only | LSL outlet; optional inlet for trial ids |
| C | Yet | Yet CSV + LabRecorder | presenter + tracker | LSL outlet; keep CSV |
| D | OpenSesame | OpenSesame + Yet CSV | tracker only | Offline merge; LSL strongly preferred |

Topology B is the reason LSL is attractive. OpenSesame sends `trial_start` and
`trial_end` markers with `pylsl`; Yet publishes gaze; LabRecorder records both.
The merge is done by timestamp, not by a custom protocol. Yet does not need to
import Python or know OpenSesame's internals.

If Yet must label gaze with OpenSesame's trial ids online, it needs an **LSL
inlet** and a small state machine that maps the latest marker to the current
trial. That is a separate feature (C4 below), not part of the first outlet
increment.

**Caveats:**

- OpenSesame canvas coordinates are not Yet monitor pixels. Publish monitor
  pixels from Yet, and let OpenSesame convert using its canvas rect. If Yet is
  to publish stimulus-normalized coordinates for an external presenter, the
  marker must carry the canvas rect at trial start.
- Confirm whether the canvas rect is in logical or physical pixels. Yet works
  in physical pixels; mixing the two on a HiDPI display shifts every gaze
  coordinate.
- Display timing belongs to OpenSesame. Yet cannot claim stimulus onset; it can
  only record its own gaze. The marker timestamp should come from OpenSesame's
  flip/onset path.
- `pylsl` calls in the run phase can block. Push markers from a small queue or
  accept the short push; do not pull gaze synchronously in a display-critical
  section.

OSWeb (browser) experiments cannot use `pylsl`; LSL integration is desktop-only.

**Verdict:** high feasibility, low direct-integration value. Use LSL/`pylsl`;
do not build an OpenSesame plugin or a PyGaze backend.

---

## 4. PsychoPy

**Current:** PsychoPy is a Python experiment package with Builder and Coder.
I found no LSL device in the documented `psychopy/hardware` or
`psychopy/iohub/devices` listings on the `dev` tree; the standard route is
`pylsl` in a Code Component. PsychoPy's iohub has eye-tracker device classes for
vendor SDKs, not for Yet.

The topology, caveats and verdict are the same as OpenSesame:

- PsychoPy presents stimuli and sends `pylsl` markers; Yet publishes gaze;
  LabRecorder records both.
- Send markers from `win.callOnFlip()` or the equivalent flip callback, not
  before the flip. The marker timestamp should be the flip time, not the time
  the Python line executed.
- For gaze-contingent work, pull gaze in a background thread into a
  latest-value buffer. Do not call `pull_sample` in the frame loop with a long
  timeout.
- PsychoPy's `core.getTime()` is not the LSL clock. Use `pylsl.local_clock()`
  for markers, or map once and keep the anchor.
- Online/Pavlovia experiments run in the browser and cannot use `pylsl`; LSL
  integration is desktop-only.

**Verdict:** high feasibility via LSL; do not build a PsychoPy component or an
iohub device for Yet.

---

## 5. Cross-cutting architecture

### 5.1 Keep the core pure

`yet-core` is `no_std` and must not grow networking. `yet-stim` is UI-free and
contains the engine and data types. LSL belongs in the application layer or in a
small adapter crate, behind a transport-neutral trait:

```rust
// Sketch only.
pub trait GazeSink {
    fn session_start(&mut self, info: &SessionInfo);
    fn gaze(&mut self, sample: &GazeSample, context: &TrialContext);
    fn event(&mut self, event: &SessionEvent);
    fn session_end(&mut self, reason: SessionEnd);
}
```

The trait uses `SessionTime`, `GazePoint`, `StimulusId`, `BlockId` — no LSL
types. `DataSink` remains the CSV implementation; an `LslSink` and a
`MultiSink` (CSV + LSL) are additional implementations. Do not change the CSV
schema.

### 5.2 Threading

```text
camera thread ──► mailbox (frame + capture time)
                       │
controller thread (60 Hz) ──► predictor ──► bounded queue ──► LSL publisher thread
        │                                                └──► CSV sink
        └──► UI / presenter
```

- One publisher owns the outlets and preserves gaze/event order.
- The controller never performs network I/O.
- Create the outlets off the controller thread; surface readiness and
  `consumer_count` in the console.
- On shutdown, enqueue `session_end`, join the publisher, then close the CSV.

### 5.3 Feature and configuration

- Put LSL behind a Cargo feature (for example `lsl`) on the `yet-zero`
  application, not on `yet-core`/`yet-stim`.
- Default it **off** to avoid accidental network broadcast of gaze data.
- Proposed config:

```toml
[lsl]
enabled = false
name_prefix = "Yet"
participant_label = "P001"   # pseudonymous; never a real name
publish_gaze = true
publish_events = true
publish_status = false
```

- If `labstream` does not expose interface binding, document `LSL_INTERFACE`
  and verify it on the lab machines. Do not invent a config key that the
  library ignores.

### 5.4 Relation to Yex/Ystudio

The event vocabulary (`session_start`, `trial_start`, `trial_onset`, ...) is a
transport-neutral precursor to Yex's `YexRecord`. Keep the names and payloads in
one document (`CONNECTIVITY.md`) so Yex and Ystudio can reuse them. Do not couple
Yet to Yex's actor model in this increment.

---

## 6. Alternatives considered

| Alternative | Why not as the primary path |
|---|---|
| Native OpenSesame plugin | Python/Rust boundary, packaging, maintenance; no clock-sync benefit |
| Native PsychoPy component / iohub device | Same; PsychoPy already has `pylsl` |
| PyO3 bindings to Yet | Couples Python ABI and packaging; does not solve recording or clock sync |
| Python bridge process (Yet → socket → `pylsl` shim) | Extra process, second clock and extra failure mode; a native outlet is simpler |
| ZeroMQ / TCP / WebSocket JSON | No discovery or shared clock; every consumer needs a custom adapter |
| OSC | Fine for markers, poor for timestamped sample streams; no clock sync |
| Shared memory | Same-machine only; no standard recorder; premature optimization |
| Direct XDF writing | Duplicates LabRecorder; XDF is a recording format, not a live protocol |
| CSV-only handoff | No live sync; trial alignment requires post-hoc clock guessing |

LSL does not prevent a future lightweight transport for a specific lab. It does
provide the standard, testable baseline.

---

## 7. Recommendation and staged plan

| Stage | Work | Rough effort |
|---|---|---|
| C0 | This document; decide the stream contract and the open questions below | done |
| C1 | Plumb capture timestamps and measure end-to-end latency; add tests | 2–4 days |
| C2 | Feature-gated LSL outlet for gaze + events; loopback test with `pylsl` | 3–5 days |
| C3 | Publisher thread, bounded queue, config, console status, privacy defaults | 2–3 days |
| C4 | Optional LSL inlet for external trial markers (OpenSesame/PsychoPy) | 3–5 days |
| C5 | Recipes: OpenSesame `inline_script`, PsychoPy Code Component, LabRecorder/XDF | 1–2 days |
| C6 | Linux lab validation, then macOS/Windows matrix | hardware-dependent |

These are rough estimates for one developer and exclude the C1 timing work if
the camera backend cannot provide a capture timestamp. Do not start C2 before
C1; otherwise the stream will be technically functional and scientifically
misleading.

**Definition of done for a first usable release:**

- Gaze and event streams are discoverable by `pylsl`.
- LabRecorder produces an XDF with both streams and no loss for 10 minutes.
- `pyxdf` loads it and trial markers align with gaze gaps within one frame.
- The console shows LSL status and consumer count.
- LSL off by default; pseudonymous labels; failure does not abort a session.
- The CSV remains unchanged and remains the primary R pipeline input.

---

## 8. Open questions (underspecified)

These determine whether this is an outlet-only feature or a closed-loop
integration. They should be answered before implementation.

1. **Who presents stimuli in the intended setup?** Yet, OpenSesame, PsychoPy, or
   a mix? This decides whether we need only an outlet, or an outlet plus an
   inlet and trial-state mapping.
2. **Offline merge or online closed loop?** Offline alignment tolerates
   50–100 ms latency; gaze-contingent/ERP-locked work does not. Closed loop
   changes the timing and threading requirements.
3. **Which recording stack and hardware?** LabRecorder/XDF, an EEG amplifier
   with its own LSL outlet, a TTL trigger box, fMRI scanner triggers, or a
   vendor recorder? Marker format and expected codes depend on this.
4. **What timestamp accuracy is required?** Sub-millisecond, within one frame
   (≈39 ms), or "best effort"? The answer determines how much C1 work is
   mandatory.
5. **Is network broadcast acceptable?** LSL is unauthenticated LAN discovery.
   Confirm the lab network policy and whether participant ids may appear in
   stream names and metadata.
6. **Which platforms are in scope for the first release?** Linux-only is
   defensible for the lab machine; Windows/macOS LSL validation is extra work.
7. **Do consumers expect raw gaze only, or fixation/saccade events?** Yet has no
   fixation detector today. If the lab expects events, that is a separate
   layer on top of the gaze stream.

---

## 9. Sources checked

- crates.io API: `labstream-core`, `labstream`, `labstream-net`,
  `labstream-wire`, `labstream-proto`, `labstream-time` (0.1.2, Sep 2026);
  `lsl`, `lsl-sys` (0.1.1, 2020); `lamquant-lsl` (0.1.2, 2026).
- docs.rs: `labstream` 0.1.2 `Outlet` and `StreamInfoBuilder` API.
- GitHub `rednayan/labstream` README: conformance claims, platform notes,
  license, crate layout.
- PyPI: `pylsl` 1.18.6 metadata and README.
- OpenSesame manual 4.1: item list, PyGaze eye-tracking backends.
- PsychoPy `dev` tree: `psychopy/hardware` and `psychopy/iohub/devices`
  listings; no LSL hardware/iohub device found.
- Yet source: `crates/yet-stim/src/engine.rs`, `data.rs`,
  `crates/yet-zero/src/console.rs`, `tracker.rs`, `camera/mod.rs`,
  `driver.rs`; `README.md`, `ROADMAP.md`, `DEVELOPMENT_PLAN.md`.
