# Rigwright

Rigwright is a reusable Rust radio-control HAL with native radio drivers. It was
extracted from [QSONaut](https://github.com/nicksbar/QSONaut) so radio support can
evolve independently and be embedded in other amateur-radio applications.

## What works today

- One async, protocol-neutral `Radio` interface for frequency, mode, PTT,
  typed controls, raw-protocol access, and capability discovery.
- Native Icom CI-V over serial, developed and exercised with the IC-7300.
- Serial-port discovery, CI-V framing and parsing, spectrum-scope data, and raw
  protocol access.
- Strict IC-7300 USB scope assembly: ordered 11-division input produces one
  complete 475-bin sweep, with documented center-span and fixed-edge controls.
- Captured-frame unit tests and a direct CI-V probe example.

Only the IC-7300 is regularly hardware-tested. Other Icom models may respond to
parts of the driver but are not yet claimed as validated. Common frequency,
mode, and PTT drivers exist for the cataloged Yaesu and Kenwood radios and
remain experimental pending physical-radio testing. Hamlib `rigctld`, DX Lab
Commander, and an in-memory mock backend are also available.

The source tree follows the public API: protocol-neutral types live in
`hal.rs`, `controls.rs`, and `models.rs`; shared framing lives under `protocol/`;
and vendor/model profiles live under `icom/`, `yaesu/`, and `kenwood/`. See
[`docs/architecture.md`](docs/architecture.md) for the profile-driven design,
override rules, and validation policy. See
[`docs/supported-radios.md`](docs/supported-radios.md) for maturity labels and
official command-manual sources.

## Use

```toml
[dependencies]
rigwright = { git = "https://github.com/nicksbar/rigwright" }
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

See [`docs/adding-icom-model.md`](docs/adding-icom-model.md) before adding an
Icom profile. It lists every catalog, profile, test, documentation, and live
validation surface that must move together.

## License

MIT
