# Rigwright

Rigwright is a reusable Rust radio-control HAL with native radio drivers. It was
extracted from [QSONaut](https://github.com/nicksbar/QSONaut) so radio support can
evolve independently and be embedded in other amateur-radio applications.

## What works today

- A small async `Radio` interface for frequency, mode, and PTT.
- An extensible `RadioHal` interface with typed controls and capability discovery.
- Native Icom CI-V over serial, developed and exercised with the IC-7300.
- Serial-port discovery, CI-V framing and parsing, spectrum-scope data, and raw
  protocol access.
- Captured-frame unit tests and a direct CI-V probe example.

Only the IC-7300 is regularly hardware-tested. Other Icom models may respond to
parts of the driver, but are not yet claimed as supported. Yaesu, Kenwood,
Hamlib, network transports, and a mock transport are future work.

## Use

```toml
[dependencies]
rigwright = { git = "https://github.com/nicksbar/rigwright" }
```

```rust,no_run
use rigwright::{IcomCiVRadio, Radio};

# async fn example() -> anyhow::Result<()> {
let radio = IcomCiVRadio::new("/dev/ttyUSB0", 115_200, 0xE0);
radio.set_frequency(14_074_000).await?;
radio.ptt(false).await?;
# Ok(())
# }
```

Run the hardware probe with `cargo run --example ci_v_probe`. It currently uses
`/dev/ttyUSB0` at 115200 baud; inspect the example before transmitting commands.

## Design rules

- Keep the app-facing HAL protocol-neutral.
- Offer typed common controls plus explicit vendor-protocol escape hatches.
- Back claimed protocol behavior with captured-frame tests.
- Never infer broad radio compatibility from one tested model.

## License

MIT

