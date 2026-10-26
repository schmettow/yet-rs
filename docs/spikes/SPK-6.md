# SPK-6 — ESP32-C glance predictor and Quadbright compression

**Status:** design note, 2026-10-25. No MCU hardware has been measured.
**Question:** can the Quadbright feature compression and the linear glance
predictor fit an ESP32-C class device, and which seams are required?

## Target

- **Board class:** ESP32-C (C3/C6 family). The exact variant, FPU, SRAM/flash and
  camera interface are **to be confirmed**; the notes below deliberately avoid
  assuming an FPU.
- **Why it is attractive:** low-power operation, an ultra-low-power monitor core
  on some variants, Bluetooth/BLE for a wireless gaze link, and a small RISC-V
  core that is more than fast enough for a four-feature linear map.
- **Not yet verified:** camera transport (SPI/I2S/parallel), frame rate, BLE
  throughput and latency, power budget, toolchain (`no_std` target).

## Pipeline

```text
camera ROI
  -> 2x2 spatial reduction (four quadrant means)      <- the "Quadbright compression"
  -> optional quantization (u8/u16)
  -> FeatureVector(4)
  -> LinearPredictor: 2 outputs, 4 multiply-adds each
  -> pixel (x, y) over BLE/UART
```

**Quadbright compression is a 2×2 image.** The current `quad_brightness`
function computes exactly these four means from an arbitrary ROI; the MCU path
treats them as the compressed frame. A sensor that performs spatial oversampling
and already outputs a 2×2 tile would be ideal, because the device would not need
a full frame buffer. That is a future sensor choice, not a requirement for the
seam.

The device does **not** decode PNG/JPEG, run `nalgebra`, allocate, or train.
Training stays on the host; only `ModelParameters` is deployed.

## Estimated resource use (not measured)

These are order-of-magnitude estimates to guide E1, not a budget.

| Item | Estimate | Notes |
|---|---:|---|
| Parameter block (in memory) | ~140 B | 2 × 16 × `f32` coefficients + 2 intercepts + version/count; the struct reserves `MAX_FEATURES` so Octabright fits without a new type |
| Deployed Quadbright payload | ~40 B | 2 × 4 × `f32` + 2 intercepts + version/count if serialized compactly; a compact wire format is not implemented yet |
| Prediction work | 8 multiply-adds | Four features × two coordinates |
| Code size | < 2 KB | Predictor + serialization + a small state machine |
| RAM | < 100 B | No frame buffer if the sensor provides the 2×2 tile |
| Compute latency | < 1 ms at 160 MHz | Camera and BLE dominate |
| Feature payload | 4 × u8 = 4 B | If quantized; f32 is 16 B |

## Risks

1. **Camera interface.** The chosen ESP32-C variant must expose a usable camera
   transport. This is the main unknown and must be checked before E1.
2. **No FPU on some variants.** `f32` may be software-emulated. The
   `GazePredictor` trait allows a fixed-point or integer implementation without
   changing callers; quantization error then becomes a measured budget.
3. **BLE latency and power.** A wireless link adds latency and battery cost; it
   is outside the prediction path.
4. **Compression error.** A 2×2 tile throws away spatial detail. That is the
   intended Quadbright representation, but the error against a full-frame
   feature extractor must be measured on real eye images.
5. **Synchronization.** A wireless gaze packet has its own timestamp; the host
   must align it with the stimulus clock. This is the same `SessionTime`
   discipline as the host cameras.

## Seam already provided by S8.5

`crates/yet-gaze` is `#![no_std]`, allocation-free and free of `nalgebra`,
`image`, UI and `std::time`. It contains `FeatureLayout`/`FeatureVector`,
`ModelParameters`, `LinearPredictor`, `GazeInput` and `GazePredictor`. The host
converts `LinearParameters` to `ModelParameters` after fitting.

## Decision

Proceed with E1 only after the exact ESP32-C variant, camera interface and
fixed-point error budget are chosen. The S8.5 seam is sufficient to prototype the
predictor on the host and to add a fixed-point implementation later without
touching the host trainer.

## Not measured here

- MCU runtime, flash/RAM, power, BLE throughput.
- Camera transport and frame rate.
- Fixed-point vs `f32` error on real eye images.
