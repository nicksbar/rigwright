# Rigwright

[![Version](https://img.shields.io/badge/version-v0.1.17-2ea44f)](Cargo.toml)
[![CI](https://github.com/nicksbar/rigwright/actions/workflows/ci.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/ci.yml)
[![Release workflow](https://github.com/nicksbar/rigwright/actions/workflows/release.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/nicksbar/rigwright?display_name=tag&sort=semver)](https://github.com/nicksbar/rigwright/releases)
[![Coverage gate](https://github.com/nicksbar/rigwright/actions/workflows/coverage.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/coverage.yml)
[![Icom 86.58%](https://img.shields.io/badge/Icom-86.58%25-brightgreen)](docs/radio-capability-matrix.md)
[![HAL 94.50%](https://img.shields.io/badge/HAL-94.50%25-brightgreen)](docs/radio-capability-matrix.md)
[![Android 83.52%](https://img.shields.io/badge/Android-83.52%25-brightgreen)](docs/radio-capability-matrix.md)
[![Transport 91.78%](https://img.shields.io/badge/Transport-91.78%25-brightgreen)](docs/radio-capability-matrix.md)
[![Drivers 90.08%](https://img.shields.io/badge/Drivers-90.08%25-brightgreen)](docs/radio-capability-matrix.md)
[![IQ 100%](https://img.shields.io/badge/IQ-100%25-brightgreen)](docs/radio-capability-matrix.md)
[![rigctld 94.76%](https://img.shields.io/badge/rigctld-94.76%25-brightgreen)](docs/radio-capability-matrix.md)
[![DX Lab 95.27%](https://img.shields.io/badge/DX%20Lab-95.27%25-brightgreen)](docs/radio-capability-matrix.md)
[![Kenwood CAT 86.28%](https://img.shields.io/badge/Kenwood%20CAT-86.28%25-brightgreen)](docs/radio-capability-matrix.md)
[![Kenwood profile 93.28%](https://img.shields.io/badge/Kenwood%20profile-93.28%25-brightgreen)](docs/radio-capability-matrix.md)
[![Yaesu profile 85%](https://img.shields.io/badge/Yaesu%20profile-85%25-yellow)](docs/radio-capability-matrix.md)
[![Classic Yaesu profile 100%](https://img.shields.io/badge/Classic%20Yaesu%20profile-100%25-brightgreen)](docs/radio-capability-matrix.md)
[![Elecraft 85.92%](https://img.shields.io/badge/Elecraft-85.92%25-brightgreen)](docs/radio-capability-matrix.md)
[![CodeQL](https://github.com/nicksbar/rigwright/actions/workflows/codeql.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/codeql.yml)

Rigwright is a reusable Rust radio-control HAL with native radio drivers. It was
extracted from [QSONaut](https://github.com/nicksbar/QSONaut) so radio support can
evolve independently and be embedded in other amateur-radio applications.

CodeQL scans the Rust workspace on pull requests and weekly. The repository
configuration enables GitHub's `security-extended` query suite; findings are
reported through the CodeQL check and GitHub code-scanning alerts.

## What works today

- One async, protocol-neutral `Radio` interface for frequency, mode, PTT,
  typed controls, raw-protocol access, and capability discovery.
- Driver-owned `RadioSession` execution with bounded, coalescing command
  admission, desired/observed snapshots, worker refresh, events, and recovery.
- A normalized `0..=255` HAL scale for radio controls and meter deflection,
  with vendor-specific physical units kept in the driver/profile layer.
- Native Icom CI-V over serial, developed and exercised with the IC-7300.
- Low-latency Icom CI-V response demultiplexing, bounded interleaved-frame
  retention, USB echo filtering, and transport health metrics.
- All native ASCII vendor transports own a persistent serialized session,
  bounded response/event demultiplexing, adaptive bounded recovery timing,
  explicit serial-line policy, and transport health metrics. Classic binary
  Yaesu exposes its fixed line policy and transaction metrics. Vendor
  identification and option probes are opt-in because probing can change radio
  state or interrupt a live CAT link.
- Additive `RadioAndroid` entry point for Icom CI-V, modern Yaesu CAT, classic
  Yaesu CAT, and Kenwood CAT over an externally supplied byte transport.
- Serial-port discovery, CI-V framing and parsing, spectrum-scope data, and raw
  protocol access.
- Strict IC-7300 USB scope assembly: ordered 11-division input produces one
  complete 475-bin sweep, with documented center-span and fixed-edge controls.
- Captured-frame unit tests and a direct CI-V probe example.
- Profile-driven Elecraft CAT support for K2, KX2, KX3, K3, K3S, K4, and KH1,
  including direct controls, model-specific option probing, normalized meters,
  and explicit accessory/protocol boundaries.
- Profile-gated receiver controls, RF power, split, clarifiers, VFO, tuner,
  memory, repeater, event, and normalized-meter support for modern Yaesu;
  profile-gated receiver controls, RF power, split, clarifiers, VFO, tuner,
  memory, repeater, event, and model-specific meters for Kenwood; and
  model-specific Icom controls including IP+, notch, tuner, memory, repeater,
  main/sub, external preamp, and normalized meters where documented.

Only the IC-7300 is regularly hardware-tested. Other profiles are not yet
claimed as hardware validated. Modern Yaesu models use a profile-driven ASCII
CAT engine with model IDs, ranges, mode maps, readable PTT, RF power, and split
gating. Classic Yaesu models use a separate profile-driven five-byte 8N2 engine
with readable PTT, split, and status. Kenwood models use a
profile-driven persistent PC-control engine with exact IDs, command families,
ranges, modes, power, split, and meter layouts. Hamlib `rigctld`, DX Lab
Commander, and an in-memory mock backend are also available.

The source tree follows the public API: protocol-neutral types live in
`hal.rs`, `controls.rs`, and `models.rs`; shared framing lives under `protocol/`;
and vendor/model profiles live under `icom/`, `yaesu/`, and `kenwood/`.

## Support and documentation

The [radio capability matrix](docs/radio-capability-matrix.md) is the canonical
status report. It tracks every HAL operation, typed control, normalized meter,
manual-only surface, exact model profile, and current QSONaut consumption. It
distinguishes documented behavior from implemented, profile-gated, consumed,
and hardware-validated behavior.

- [Radio capability matrix](docs/radio-capability-matrix.md) — detailed status,
  normalization, model exceptions, and QSONaut coverage.
- [Supported radios and manual sources](docs/supported-radios.md) — supported
  models, maturity labels, and workspace manual editions.
- [Driver architecture](docs/architecture.md) — HAL boundaries, transport
  rules, profiles, and validation policy.
- [Driver-owned sessions](docs/session.md) — issue #20 queue, state, event, and
  baud-selection behavior.

Only the capability matrix should be updated for ordinary support-status
changes; the architecture and model-addition guides are design/maintenance
references.

## Use

```toml
[dependencies]
rigwright = "0.1.17"
```

```rust,no_run
use rigwright::{IcomCiVRadio, Radio};

# async fn example() -> anyhow::Result<()> {
let radio = IcomCiVRadio::new_generic("/dev/ttyUSB0", 115_200, 0xE0, 0x94);
radio.set_frequency_hz(14_074_000).await?;
radio.set_ptt(false).await?;
# Ok(())
# }
```

Run the hardware probe with `cargo run --example ci_v_probe`. It currently uses
`/dev/ttyUSB0` at 115200 baud; inspect the example before transmitting commands.

When the model is known, prefer a profile-backed constructor so Rigwright can
validate documented ranges, modes, controls, and scope geometry:

```rust,no_run
use rigwright::{models::IcomCivModel, IcomCiVRadio};

let radio = IcomCiVRadio::new_for_model_default_address(
    IcomCivModel::Ic7300,
    "/dev/ttyUSB0",
    115_200,
    0xE0,
);
assert_eq!(radio.radio_address(), 0x94);
```

Use `new_for_model` when the operator changed the radio's CI-V address. The
model-neutral constructor intentionally cannot expose profile-only controls or
decode a model-specific spectrum stream.

Modern Yaesu radios follow the same profile-backed pattern:

```rust,no_run
use rigwright::{Radio, YaesuCatModel, YaesuCatRadio};

# async fn example() -> anyhow::Result<()> {
let radio = YaesuCatRadio::new_for_model(
    YaesuCatModel::Ftdx10,
    "/dev/ttyUSB0",
    38_400,
)?;
radio.verify_model()?;
radio.set_frequency_hz(14_074_000).await?;
radio.set_ptt(false).await?;
# Ok(())
# }
```

Use the Enhanced virtual COM port for FTDX10 CAT. The Standard port is for
PTT/keying/digital-mode signals, not frequency and mode CAT commands.

For a read-only identity/frequency/mode/PTT check, run
`cargo run --example yaesu_probe -- FTDX10 /dev/ttyUSB0 38400`. Match the baud
rate and one-stop-bit setting in the radio's CAT menu. No example command keys
the transmitter. FTDX10 automatically reads its CAT RTS setting and adapts the
serial flow-control mode; `--hardware-flow` remains available for adapters that
need an explicit override.

For an older FT-817ND, FT-818, FT-857D, or FT-897D, set the radio's CAT menu to
4800, 9600, or 38400 baud and use the documented CT-62-compatible serial
interface. The driver configures 8N2 automatically. A read-only check is:

```text
cargo run --example classic_yaesu_probe -- FT-857D /dev/ttyUSB0 4800
```

The classic protocol has no identification command, so this probe can confirm
responses but cannot prove that the configured model name is correct.

Kenwood TS-590SG, TS-890S, and TS-2000 use the same model-backed factory path.
Match the radio's PC-control baud menu; the driver automatically uses two stop
bits at 4800 baud and one stop bit at higher rates. A read-only identity and
status probe is:

```text
cargo run --example kenwood_probe -- TS-590SG /dev/ttyUSB0 115200
```

The probe never sends `TX`. TS-590SG and TS-2000 PTT state is read from `IF`;
TS-890S does not advertise readable PTT because its documented `TX` command is
set/auto-information only.

## Tests and coverage

Run the full locked test suite locally with:

```text
cargo test --locked --all-targets --all-features
```

Rigwright uses LLVM source-based coverage through `cargo-llvm-cov`. Install the
LLVM tools and the Cargo subcommand once, then generate an HTML report:

```text
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo llvm-cov --locked --all-features --workspace --html
```

The report is written to `target/llvm-cov/html/index.html`; open that file in a
browser to inspect line and branch coverage. For a terminal summary instead,
run `cargo llvm-cov --locked --all-features --workspace`.

Pull requests run formatting, locked checks, strict Clippy, and the complete
test suite in `ci.yml`. LLVM coverage runs as a separate `coverage.yml`
workflow; it prints the test and coverage summary in the pull request's check
details and uploads the complete HTML report as a workflow artifact.

The README coverage labels are the latest measured line-coverage snapshot; the
workflow badge is the authoritative pass/fail result. The coverage gate is
enforced by `scripts/check-icom-coverage.sh` and
`scripts/check-elecraft-coverage.sh`. The Elecraft gate requires at least 80%
aggregate line coverage; the current measured Elecraft snapshot is 85.92%.
The existing gates currently
requires at least 85% Icom, 90% HAL, 80% Android, 90% transport, 85% driver,
95% IQ, 90% rigctld, 90% DX Lab, 80% Kenwood CAT, 90% Kenwood profile, 85%
modern Yaesu profile, and 85% classic Yaesu profile line coverage. The latest
local run reached 85.13% overall line coverage, including 84.98% Icom CI-V,
86.29% Kenwood CAT, 76.74% modern Yaesu CAT, 91.79% transport, and 90.09%
configured-driver dispatch coverage. The workflow badge reports whether these
tests and gates pass; the uploaded LLVM report provides the detailed source,
function, and line view.

## Design rules

- Keep the app-facing HAL protocol-neutral.
- Offer typed common controls plus explicit vendor-protocol escape hatches.
- Back claimed protocol behavior with captured-frame tests.
- Never infer broad radio compatibility from one tested model.
- Keep protocol-neutral HAL types independent of vendor drivers.
- Put model defaults and documented differences in model profiles; generic
  protocol drivers execute those profiles.
- Treat CI-V addresses as configurable values with model defaults, never as
  immutable application constants.
- Keep scope, waveform, I/Q, satellite, and dual-receiver features optional
  until their wire formats are implemented and tested.

See [`docs/adding-icom-model.md`](docs/adding-icom-model.md) or
[`docs/adding-yaesu-model.md`](docs/adding-yaesu-model.md) before adding a modern
profile. Classic five-byte models use
[`docs/adding-classic-yaesu-model.md`](docs/adding-classic-yaesu-model.md).
Kenwood profiles use
[`docs/adding-kenwood-model.md`](docs/adding-kenwood-model.md).

## License

MIT
