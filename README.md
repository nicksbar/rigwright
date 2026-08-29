# Rigwright

[![CI](https://github.com/nicksbar/rigwright/actions/workflows/ci.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/ci.yml)
[![Coverage](https://github.com/nicksbar/rigwright/actions/workflows/coverage.yml/badge.svg)](https://github.com/nicksbar/rigwright/actions/workflows/coverage.yml)

Rigwright is a reusable Rust radio-control HAL with native radio drivers. It was
extracted from [QSONaut](https://github.com/nicksbar/QSONaut) so radio support can
evolve independently and be embedded in other amateur-radio applications.

## What works today

- One async, protocol-neutral `Radio` interface for frequency, mode, PTT,
  typed controls, raw-protocol access, and capability discovery.
- A normalized `0..=255` HAL scale for radio controls and meter deflection,
  with vendor-specific physical units kept in the driver/profile layer.
- Native Icom CI-V over serial, developed and exercised with the IC-7300.
- Additive `RadioAndroid` entry point for Icom CI-V, modern Yaesu CAT, classic
  Yaesu CAT, and Kenwood CAT over an externally supplied byte transport.
- Serial-port discovery, CI-V framing and parsing, spectrum-scope data, and raw
  protocol access.
- Strict IC-7300 USB scope assembly: ordered 11-division input produces one
  complete 475-bin sweep, with documented center-span and fixed-edge controls.
- Captured-frame unit tests and a direct CI-V probe example.
- Profile-gated RF power, split, AGC, noise reduction, and normalized meter
  support for modern Yaesu; profile-gated RF power, split, signal, and SWR for
  Kenwood; and model-specific Icom controls including IP+, notch, tuner,
  main/sub, and external preamp where documented.

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

Only the capability matrix should be updated for ordinary support-status
changes; the architecture and model-addition guides are design/maintenance
references.

## Use

```toml
[dependencies]
rigwright = "0.1.12"
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

Pull requests run formatting, checks, and tests in `ci.yml`. LLVM coverage runs
as a separate `coverage.yml` workflow; it prints the test and coverage summary
in the pull request's check details and uploads the complete HTML report as a
workflow artifact.

The badges above intentionally report workflow status and the availability of
the LLVM report rather than a hard-coded percentage. Coverage can currently be
reviewed by source file in the uploaded report, including the Icom, Yaesu, and
Kenwood brand modules and their supported model profiles. A meaningful
per-brand or per-model percentage badge requires separate coverage test
targets—or stable source filters and badge publication—which is not yet part
of this project.

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
