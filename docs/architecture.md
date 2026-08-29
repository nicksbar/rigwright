# Rigwright driver architecture

Rigwright has one protocol-neutral root HAL and separate protocol backends.

## Root HAL

The root API lives in `src/hal.rs` and `src/controls.rs`:

- `Radio` provides frequency, mode, PTT state, controls, raw access, and
  capabilities through the `Radio` trait.
- `Mode`, `ControlId`, and `ControlValue` are application-facing types.
- Drivers translate these types into their protocol-specific command formats.

A generic control identifier does not imply universal hardware support. Each
selected driver validates support, value type, range, and command encoding.
Gain-like `ControlValue::U8` controls, including `RfPower`, use the normalized
0-255 HAL range. A vendor API may additionally expose exact native units; for
example, modern Yaesu `get_power_watts`/`set_power_watts` map the manual's `PC`
field without pretending that watts are percent.
The older `frequency`, `set_frequency`, `mode`, and `ptt` method spellings are
compatibility wrappers; new code should use the explicit `*_hz`, `get_*`, and
`set_*` methods.

The trait is asynchronous, but the native serial implementation currently does
blocking serial reads behind that boundary. Do not call it while holding a GUI
mutex or another latency-sensitive application lock.

## Backends

- `icom/` contains the shared Icom CI-V implementation and model profiles.
- `yaesu/cat_radio.rs` is the model-neutral modern Yaesu ASCII CAT transport;
  `yaesu/profile.rs` and model modules contain documented differences.
- `yaesu/legacy_radio.rs` and `yaesu/legacy_profile.rs` implement classic Yaesu
  five-byte binary CAT as a separate profiled backend.
- `kenwood/cat_radio.rs` is the persistent, model-neutral Kenwood PC-control
  transport; `kenwood/profile.rs` owns per-model command differences.
- `rigctld.rs` contains the Hamlib TCP backend.
- `dxlab.rs` contains the DX Lab Suite Commander TCP backend.
- `NullRadio` is an in-memory backend for tests and offline UI work.

`ConfiguredRadio` is the factory/dispatch enum that forwards the root HAL to
the selected backend.

## Application-facing model catalog

`models::POPULAR_RADIOS` is the source of truth applications should use for
native model selection. Each `RadioModelProfile` supplies the canonical model
name, manufacturer, protocol, validation maturity, broad band/spectrum
metadata, and helpers for the selected driver's behavior:

- `preferred_baud_rate()` derives a safe starting choice from the vendor
  profile. It does not override the baud configured on the physical radio.
- `driver_capabilities()` reports the root frequency/mode/PTT/raw operations.
- `supports_control(ControlId)` reports only typed controls implemented for
  that exact profile.

Applications should group and label models from this catalog instead of
maintaining vendor lists or inferring a manufacturer from a model-name prefix.
Connection backends such as `rigctld`, DX Lab, and an offline mock remain
application choices rather than radio manufacturers. When a model is added,
updating its vendor profile and catalog row makes it available to consumers
without adding model branches to their UI or business logic.

## Profiles and overrides

Model files define defaults and documented differences:

- CI-V address defaults
- Frequency ranges
- Mode mappings
- Control mappings and ranges
- Optional scope/IQ/receiver capabilities

Modern Yaesu profiles additionally own the `ID;` value, accepted CAT baud
rates, receiver-qualified `MD` mappings, `PC` range in watts, and optional
command families. `YaesuCatRadio` reuses one serial connection and matches
semicolon-terminated replies by command so unsolicited auto-information frames
cannot be mistaken for a requested reply. Modern ASCII CAT and legacy binary
CAT are intentionally separate engines.

Yaesu `TX;` distinguishes idle (`0`), CAT-requested transmit (`1`), and
radio/front-panel transmit (`2`). The protocol-neutral PTT state treats both
non-zero values as transmitting; `set_ptt(false)` sends `TX0;`.

Classic Yaesu CAT is not an earlier framing option for the modern engine. It
uses exactly five binary bytes, no terminator, 8N2 serial framing, and no set
acknowledgements. `E7`, `F7`, and `03` return RX status, TX status, and
frequency/mode respectively. Several status bits use active-low polarity:
split and PTT are on when their bits are zero. Classic radios provide no model
identification command, so model selection is operator-supplied profile data.
Because set commands have no acknowledgement, the HAL follows every PTT change
with an `F7` status read and rejects a state mismatch.

Kenwood framing is shared, but command semantics vary by generation. The
TS-590SG and TS-2000 profiles use `MD` and represent split through differing
`FR`/`FT` VFO selections. TS-590SG adds the separate `DA` data-mode flag. The
TS-890S uses `OM` modes (including explicit data variants) and direct `TB`
split control. The driver queries the selected receive VFO before using
`FA`/`FB`, instead of assuming VFO A is always active.

Kenwood set commands generally do not acknowledge success. TS-590SG and
TS-2000 expose RX/TX state in the fixed-layout `IF` response, so PTT writes are
followed by a status verification. The TS-890S `TX` response is available only
through Auto Information and is not a pollable status command; the profile
therefore reports `can_get_ptt = false`. Unsolicited Auto Information frames
are tolerated and cannot satisfy an unrelated query.

At 4800 baud the profiled Kenwood serial transport uses 8N2. At higher rates it
uses 8N1. TS-890S 4800-baud operation is COM-only; its USB virtual COM port
starts at 9600 baud.

`IcomCivProfile` is declarative. A control stores an arbitrary-length command
prefix followed by a typed value. This matters because CI-V does not have one
uniform command/subcommand grammar: split (`0F`) and attenuator (`11`) take
their value directly, while gain (`14 01`) and preamp (`16 02`) use a
subcommand. Do not add a placeholder `00` to command-only operations.

Frequency ranges are driver guardrails for tuning, not transmit authorization
or regulatory band plans. Applications remain responsible for TX band, license,
power, and mode policy. The radio may also apply region-specific limits.

Software configuration always wins over a model default. In particular, an
Icom CI-V radio is constructed with the model default address only when no
address override is supplied.

CI-V frame directions are:

- controller to radio: `FE FE <radio> <controller> <payload> FD`
- radio to controller: `FE FE <controller> <radio> <payload> FD`

The controller address is normally `E0`; factory radio addresses are only
defaults and are always overrideable. A NAK (`FA`) and a missing ACK are errors,
not successful writes.

## Scope and streams

Spectrum scope and I/Q data are optional capabilities, not requirements of the
root HAL. A driver must implement and test the actual waveform transport before
advertising a scope stream. Geometry, metadata layout, and receiver selection
are model-specific profile data.

All four currently profiled USB waveform formats place a scope selector first
(fixed `00`, or main/sub), then current division and maximum division. The
sample geometry still differs: IC-705/IC-7300/IC-9700 use 11 divisions and 475
bins; IC-7610 uses 15 divisions and 689 bins. The first division carries only
waveform metadata.

## Validation policy

IC-7300 and FTDX10 behavior are hardware-validated for the exercised CAT
paths. Other model profiles are based on the available official command
references until tested against physical hardware.
Captured protocol fixtures and parser tests are preferred over compatibility
claims.

See [`adding-icom-model.md`](adding-icom-model.md) and
[`adding-yaesu-model.md`](adding-yaesu-model.md) or
[`adding-classic-yaesu-model.md`](adding-classic-yaesu-model.md), plus
[`adding-kenwood-model.md`](adding-kenwood-model.md) for extension checklists,
and
[`supported-radios.md`](supported-radios.md) for exact manual editions and the
boundary between implementation and validation.
