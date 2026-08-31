# Radio capability matrix

This is the canonical detailed support inventory for the radios in
[`supported-radios.md`](supported-radios.md). It separates documentation,
implementation, profile, application, and validation claims:

| Code | Meaning |
|---|---|
| **M** | Documented by the radio manual or vendor command reference. |
| **H** | Exposed as a typed Rigwright HAL operation or value. |
| **P** | Implemented and gated by the selected model profile. |
| **Q** | Currently consumed by native QSONaut UI/workflows. |
| **V** | Hardware-validated in the project. Current validated radios are the IC-7300 and FTDX10. |
| **—** | Not available, not applicable, or intentionally not claimed. |
| **R** | Read-only telemetry. **W** means writable control. **RW** means both. |

`M` alone is not a support claim. Vendor manuals describe many functions that
are not yet safe to expose generically because command selectors, payloads,
units, or model behavior differ.

## Root HAL operations

| Operation | HAL surface | Icom CI-V | Modern Yaesu CAT | Classic Yaesu CAT | Kenwood PC control | QSONaut native use |
|---|---|---:|---:|---:|---:|---|
| Frequency read/write | `get/set_frequency_hz` | H/P/V for IC-7300 | H/P | H/P | H/P | Q |
| Operating mode read/write | `get/set_mode` | H/P/V for IC-7300 | H/P | H/P | H/P | Q |
| PTT write | `set_ptt` | H/P/V | H/P | H/P | H/P | Q |
| PTT read | `get_ptt` | H/P/V | H/P | H/P | P for TS-590SG/TS-2000; unavailable for TS-890S | Q |
| Radio power write | `set_power` | H/P/V behavior validated on IC-7300 | H/P | — | H/P | Q |
| Radio power read | `get_power` | —; CI-V power is write-only | H/P | — | H/P | Q/pending-state handling |
| Raw protocol | `protocol_write_read` | H/P | H/P | H/P | H/P | Not a normal UI control |
| Tuner start/status | `start_tuner`, `get_tuner_status` | H/P/V for profiled Icoms | H/P | H/P | H/P | Q: tuner and SWR sweep workflow |
| Spectrum waveform | backend-specific scope API | H/P; model geometry differs | — | — | — | Q where native scope is enabled |
| I/Q stream | model/backend-specific | Shared I/Q sample block decoder only; IC-7610 documents USB I/Q output, but Rigwright does not yet own/open that transport | — | — | — | Not currently consumed |

### Universal HAL caveat

The method names are universal; the hardware support is not. Applications must
check `Radio::capabilities()`, `supports_control()`, and `supports_meter()` as
appropriate. A generic vendor driver deliberately reports no optional typed
controls or meters until a concrete model profile is selected.

## Elecraft first implementation slice

| Model profiles | Support level | Profile module | Manual evidence |
|---|---|---|---|
| K2 | Framework | `src/elecraft/k2.rs` | `KIO2 Pgmrs Ref rev E.pdf` |
| KX2 | Framework | `src/elecraft/kx2.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| KX3 | Framework | `src/elecraft/kx3.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K3 | Framework | `src/elecraft/k3.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K3S | Framework | `src/elecraft/k3s.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K4 | Framework | `src/elecraft/k4.rs` | `K4 Programmer's Reference, rev. D5.pdf` |

| Operation | HAL surface | Elecraft status |
|---|---|---:|
| Frequency read/write | `get/set_frequency_hz` | H/P |
| Operating mode read/write | `get/set_mode` | H/P for selected profiles |
| PTT write/read | `set_ptt`, `get_ptt` | H/P via `TX`/`RX` and `TQ` |
| RF power | `ControlId::RfPower` | H/P via profile-scaled `PC` |
| VFO selection / split | `ControlId::Vfo`, `ControlId::Split` | H/P via `FR`/`FT`; K3/K3S VFO-B selection remains profile-limited |
| RIT/XIT enable and offset | `ControlId::{Rit,Xit}`, offset methods | H/P via `RT`/`XT`/`RO`/`IF` |
| Signal meter | `MeterId::Signal` | H/P via `SM` |
| AF gain | `ControlId::AfGain` | H/P via `AG` |
| RF gain | `ControlId::RfGain` | H/P via profile-scaled `RG` |
| Squelch | `ControlId::Squelch` | H/P via profile-scaled `SQ` |
| Preamp / attenuator | `ControlId::{Preamp,Attenuator}` | H/P via profile-owned `PA`/`RA` ranges |
| Noise blanker | `ControlId::NoiseBlanker` | H/P via `NB` enable state |
| AGC | `ControlId::Agc` | H/P via `GT` fast/slow mapping |
| Raw protocol | `protocol_write_read` | H/P |

Elecraft profile differences currently cover K2 versus K3-family mode tables,
model-specific baud lists, conservative HF frequency ranges, and normalized
S-meter limits. The model-specific declarations live in the six modules listed
above; shared profile validation remains in `src/elecraft/profile.rs`.
Auto-Info event routing is now available through the shared event router. K4
Ethernet/streaming, precise VFO-B semantics, and the wider K3-family control
surface remain open.
KH1 and P3/PX3/KAT/KPA/KXPA equipment are separate future profiles or station
components and are not included in this row.

### Direct CAT control backlog and evidence gate

The following controls are known from the local Elecraft programmer references
and are intentionally tracked even though they are not yet exposed by the
Elecraft `Radio` implementation. “Manual” means the command family is known
enough to design against; it is not proof of correct code or physical behavior.

| Direct CAT surface | Current Rigwright status | Required implementation evidence | Physical tester evidence |
|---|---|---|---|
| RF power (`PC`) | Implemented in driver; framework-level | Profile-native limits, normalized `RfPower`, read/write fixtures, TX-safety tests | Readback and safe min/max power on each model |
| VFO-A/B and selected-VFO routing (`FA`/`FB` plus selection) | Implemented in driver; K3/K3S profile-limited | Explicit VFO state and command-routing tests | Both VFOs, switching, and unsolicited updates |
| Split | Implemented in driver; framework-level | Profile-gated `ControlId::Split` and selected-VFO tests | RX/TX VFO behavior and split transitions |
| RIT/XIT | Implemented in driver; framework-level | Signed offset/enable contract and boundary tests | Sign, range, zero, and independent operation |
| Tuning step | Manual; not implemented | HAL shape and model-specific value tests | Every supported step on a physical dial/navigation workflow |
| Filters/bandwidth | Manual; not implemented | Named/value profile tables and readback fixtures | Accepted values and mode-dependent behavior |
| AGC | Implemented in driver; framework-level | Model-specific control encoding and capability tests | AGC choices and readback |
| Noise blanker/reduction | NB enable implemented; levels/NR open | Separate enable/level controls and range tests | Level behavior and mode interaction |
| Preamp/attenuator | Implemented in driver; framework-level | Distinct profile controls and mutual-exclusion tests | RF-path state and combinations |
| Internal tuner | Manual; not implemented | Tuner state model, explicit-start path, and failure tests | Tune start/completion/failure and TX interlock |
| Memory/channel operations | Manual; not implemented | Lossless `MemoryChannel` mapping or explicit unsupported fields | Empty/read/write/name/mode/frequency round trips |
| Repeater/tone | Manual; not implemented | Profile-gated `RepeaterSettings` and unsupported-field tests | Tone, offset, and model-specific repeater behavior |
| TX status and additional meters | Manual; not implemented | Typed status/events, normalized meter fixtures, read-only semantics | RX/TX/tune captures for power/SWR/ALC/etc. |
| Identification and capability probing (`ID`/status) | `ID` query implemented; probing remains | Bounded probe, unknown-model, timeout, and malformed-frame tests | Known model and firmware identification |

The tester gate is deliberate: all rows may reach `Implemented` with manual
review and deterministic fixtures, but no Elecraft model should move from
`Framework` to `Hardware validated` until a tester exercises the relevant CAT
surface on physical equipment. Tester captures should be retained as fixtures
with model, firmware, baud, transport, and operating-state metadata.

## Typed controls

The following table lists every current `ControlId`. “Icom” means the selected
Icom profile exposes the operation; “Yaesu” and “Kenwood” are the current
profile-wide implementation claims. A model-specific exception is listed in
the final column.

| HAL control | Value | Icom CI-V | Modern Yaesu | Classic Yaesu | Kenwood | QSONaut native use |
|---|---|---:|---:|---:|---:|---|
| `AfGain` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q slider |
| `RfGain` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q slider |
| `Squelch` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q slider |
| `RfPower` | normalized `U8` 0–255 | RW/P all four profiles | RW/P modern profiles; exact watts also available | M, not typed; power write intentionally absent | RW/P all three profiles; exact watts also available | Q slider and SWR sweep power |
| `Preamp` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q compact control |
| `ExternalPreamp` | model-specific `U8` | RW/P IC-9700 only | M, not typed | M, not typed | M, not typed | Not currently used |
| `Attenuator` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q compact control |
| `NoiseBlanker` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q toggle |
| `NoiseReduction` | `Bool` | RW/P all four profiles | RW/P modern profiles | M, not typed | RW/P all three profiles | Q toggle |
| `NoiseReductionLevel` | `U8` 1–15 | M, not typed | RW/P modern profiles | M, not typed | M, not typed | Q level control where advertised |
| `IpPlus` | `Bool` | RW/P all four profiles | M, not typed | — | M, not typed | Q toggle |
| `Notch` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q toggle |
| `ManualNotch` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q toggle; position not typed |
| `DataMode` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | TS-590SG/TS-890S model behavior exists but not typed as this control | Q mode/status support |
| `Filter` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | RW/P TS-590SG and TS-890S | Q filter control |
| `Agc` | model-specific `U8` | RW/P IC-705, IC-7300, and IC-9700; manual-only on IC-7610 | RW/P modern profiles | M, not typed | RW/P TS-890S | Q compact control |
| `Rit` | Icom `21 01`; model-specific elsewhere | R/W on all profiled Icom models | M | M | RW/P all three profiles | Icom profile implementation |
| `Xit` | Icom `21 02` where documented | R/W on IC-7300/IC-7610 profiles | M | M | RW/P all three profiles | Model-specific; not exposed on IC-705/IC-9700 |
| `Split` | `Bool` | RW/P all four profiles | RW/P profiles with documented split | RW/P all four profiles | RW/P all three profiles | Q profile/control path, limited banner use |
| `Tuner` | `Bool` enable/status | RW/P all four profiles | M, not typed | M, not typed | RW/P all three profiles | Q enable/disable and sweep safety |
| `Vfo` | model-specific selector | W/P Icom profiles; A/B readback is not documented | M, not typed | M, not typed | RW/P all three profiles | Q write-only selector with local state |
| `MainSub` | receiver selector | RW/P IC-7610 and IC-9700 | M, not typed | — | M, not typed | Not currently used |
| `RawCiV` | raw bytes | H/P Icom only | — | — | — | Not a normal UI control |

Icom control-boundary tests cover both sides of the profile boundary. A
model-neutral CI-V instance advertises neither profile controls nor meters and
rejects typed model controls before transport. Each profiled model is checked
for inherited common controls, model-only controls, read/write direction, and
meter availability. Fake-transport tests additionally verify exact CI-V
frames for inherited RF power and the IC-7300 XIT override. Decoder tests
cover both HF and VHF/UHF memory layouts, signed RIT offsets, repeater flags,
meter BCD values, and all four scope geometries. These are protocol and
regression tests; they do not promote the framework-level IC-705, IC-7610, or
IC-9700 profiles to physical-hardware validation.

### Control interpretation notes

Read/write direction is part of the driver contract. A control may be exposed
for writing without having reliable readback; Icom VFO A/B selection is the
current example. Applications should use `supports_control_read()` and
`supports_control_write()` independently rather than inferring both from
`supports_control()`.

- Icom controls are declarative CI-V profile entries. The exact command
  prefixes and allowed values remain model-specific even when the HAL name is
  shared.
- Modern Yaesu typed controls cover AF/RF gain, squelch, RF power, preamp/IPO,
  attenuator, NB, auto/manual notch, filter width, AGC, NR, RIT/XIT, tuner, VFO
  selection, and split across the current modern profiles. Repeater (`CN`/`CT`/`OS`)
  and memory (`MC`/`MR`/`MT`) operations are also profile-gated.
- Classic Yaesu has a separate five-byte binary protocol. It supports split,
  write-only RIT/clarifier and repeater shift/tone/offset operations, explicit
  VFO toggle and CAT-lock helpers, frequency, mode, PTT, and normalized RX/TX
  status meters in Rigwright; it does not inherit modern Yaesu CAT controls.
- Kenwood has verified RF power, split, frequency, mode, PTT where pollable,
  signal meter, and SWR meter. Its manual surfaces are broader than the
  current typed profile.

### Exact typed-control profile sets

These lists are the concrete profile result behind the grouped table above.
They include the shared Icom special operations `DataMode`, `Filter`, `Vfo`,
and `RawCiV` where the selected profile permits them.

| Model/profile | Typed controls currently advertised by Rigwright |
|---|---|
| IC-705 | AF gain, RF gain, squelch, RF power, preamp, attenuator, AGC, NB, NR, IP+, auto notch, manual notch, tuner, split, data mode, filter, VFO, raw CI-V |
| IC-7300 | Same as IC-705; this is the hardware-validated Icom profile |
| IC-7610 | AF gain, RF gain, squelch, RF power, preamp, attenuator, NB, NR, IP+, auto notch, manual notch, tuner, split, data mode, filter, VFO, main/sub, raw CI-V; AGC is manual-only and not typed in this profile |
| IC-9700 | IC-705 set plus external preamp and main/sub |
| FTDX10 | Full modern typed-control set, repeater controls, full memory records; hardware-validated CAT path |
| FT-710, FTDX101D, FTDX101MP | Full modern typed-control set, repeater controls, full memory records |
| FT-991A | Full modern typed-control set, repeater controls, full memory records; split remains manual-only in the current profile |
| FT-817ND, FT-818, FT-857D, FT-897D | Split (read/write), RIT/clarifier and repeater shift/tone/offset (write-only), VFO toggle, CAT lock |
| TS-590SG | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, filter A/B, RIT/XIT, VFO, split, tuner, signal/power/SWR/ALC/COMP meters, AI, and memory records |
| TS-890S | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, filter A/B/C, AGC, RIT/XIT, VFO, split, tuner, signal/power/SWR/ALC/COMP/current/voltage/temperature meters, AI, and memory records |
| TS-2000 | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, RIT/XIT, VFO, split, tuner, signal/power/SWR meters, and AI; filter and memory records remain unprofiled |

## Normalized meters

All typed meter values are normalized to a HAL deflection level of 0–255. This
is not a universal physical-unit conversion. For example, Icom SWR values have
documented ratio anchors, but those anchors are not shared by Yaesu or Kenwood.

| HAL meter | Icom CI-V | Modern Yaesu CAT | Classic Yaesu CAT | Kenwood | QSONaut native use |
|---|---:|---:|---:|---:|---|
| `Signal` | R/P via `15 02`; IC-7300 V | R/P via `RM1` | R/P via `E7`, 0-15 | R/P via `SM`, profile max 30 or 70 | Q normalized meter panel where advertised |
| `Power` | R/P via `15 11`; IC-7300 V | R/P via `RM5` | R/P via `F7`, 0-15 | R/P via `SM` (TX), profile max 30 or 70 | Q normalized meter panel where advertised |
| `Swr` | R/P via `15 12`; IC-7300 V | R/P via `RM6` | M, not typed | R/P via `RM`; selector and range profile-specific | Q live meter and stepped SWR chart |
| `Alc` | R/P via `15 13`; IC-7300 V | R/P via `RM4` | M, not typed | R/P TS-890S via `RM1` | Q normalized meter panel where advertised |
| `Compression` | R/P via `15 14`; IC-7300 V | R/P via `RM3` | M, not typed | R/P TS-890S via `RM3` | Q normalized meter panel where advertised |
| `Current` | R/P via `15 16`; IC-7300 V | R/P via `RM7` | M, not typed | R/P TS-890S via `RM4` | Q normalized meter panel where advertised |
| `Voltage` | R/P via `15 15`; IC-7300 V | R/P via `RM8` | M, not typed | R/P TS-890S via `RM5` | Q normalized meter panel where advertised |
| `Temperature` | Manual/protocol surface varies; intentionally not profiled | R/P FTDX101D/MP via `RM9`; not exposed by FT-710, FTDX10, or FT-991A | M, not typed | R/P TS-890S via `RM6` | Q normalized meter panel where advertised |

Yaesu `RM` selector meanings are documented by the modern CAT manuals: `1`
signal, `3` compression, `4` ALC, `5` power, `6` SWR, `7` current, and `8`
voltage. The FTDX101D/MP additionally document `9` for temperature; the
other profiled modern Yaesu models do not. Kenwood RM selector meanings differ by model; TS-590SG and TS-2000
use one SWR selector while TS-890S uses another, so the profile owns the
selector and native meter maximum.

## Manual surfaces not yet fully typed into the HAL

These are intentionally tracked so they do not disappear from the roadmap.
They are documented radio capabilities, but currently have no safe common
`ControlId`/`MeterId` implementation across the supported model set.

| Surface | Manual coverage | Current Rigwright status | QSONaut status | Why it remains open |
|---|---|---|---|---|
| RIT/XIT offset and enable | All modern vendor families | Icom RIT enable plus signed RIT offset read/write implemented; Icom XIT enable implemented where documented, with no separate XIT offset command documented | Not used | XIT offset and non-Icom payloads remain family-specific |
| Yaesu AF/RF gain and squelch | Modern CAT manuals | Profiled `AG`/`RG`/`SQ` controls | Not yet consumed | Native ranges are normalized by the driver |
| Yaesu NB, filter, tuner, antenna selection | Modern CAT manuals | Profiled `NB`, `SH`, and `AC`; antenna selection remains untyped | Not yet consumed | Several antenna and filter variants remain model dependent |
| Yaesu preamp/attenuator/notch | Modern CAT manuals | Profiled `PA`, `RA`, `BC`, and `BP` controls | Not yet consumed | Exact physical labels remain model/UI concerns |
| Yaesu RIT/XIT and VFO selection | Modern CAT manuals | Profiled `RT`/`XT`, `CF`, and `VS`; VFO readback is supported | Not yet consumed | Clarifier payload is shared, while UI should retain RX/TX enable state separately |
| Yaesu ALC/compression/current/voltage presentation | Modern CAT manuals | Meter IDs implemented | Not displayed | QSONaut must add polling, labels, and TX safety policy |
| Icom signal, output power, ALC, compression, voltage/current/temperature | CI-V/model manuals | Profile-gated CI-V `15` meter queries implemented and normalized to `0..=255` | Consumed by the native meter panel | Physical-unit labels/scales remain model/UI concerns |
| Icom NR level and manual-notch position | IC-7300/IC-7610 CI-V manuals | Profiled `14 06` NR level and `14 0D` manual-notch position controls implemented for IC-7300/IC-7610 | Not yet consumed | IC-705/IC-9700 do not advertise the same HF notch surface |
| Kenwood AF/RF gain, squelch, AGC, NB, NR, filter | PC command references | Typed: gain/squelch/NB/NR for all three; TS-890S AGC; TS-590SG A/B and TS-890S A/B/C filter selection | Not used | Noise-reduction depth and detailed roofing-filter bandwidths remain model-specific |
| Kenwood ALC/compression/current/voltage/temperature | PC command references | TS-590SG ALC/COMP and TS-890S ALC/COMP/current/voltage/temperature via profiled RM selectors | Not displayed | Meters are normalized deflection, not physical units |
| Memory/channel operations | Vendor manuals | Icom `08`/`09` selection and model-specific `1A 00` records: HF mode/data/CTCSS/name plus IC-705/IC-9700 band, duplex, DTCS, offset, and 16-char name fields; modern Yaesu `MC`/`MR`/`MT`; Kenwood TS-590SG `MC`/`MR`/`MW` and TS-890 `FR`/`MN` plus `MA0` records | Not used | Icom program-scan, call, DV routing, and satellite records remain separate surfaces |
| Repeater tone/shift | IC-7300, IC-705, IC-9700 and modern Yaesu CAT manuals | Icom CTCSS flags/frequencies, live duplex shift/offset (`0C`/`0D` and `0F`), VHF/UHF DTCS memory fields, and signed RIT offset; modern Yaesu `CN`/`CT`/`OS` with main-band selector; Kenwood `CN`/`CT` tone | Not used | Icom live DTCS and model-specific auto-repeater settings remain separate surfaces; Yaesu offset frequency is carried by memory/configuration rather than `OS` |
| DTMF transmission | Vendor manuals | Validated HAL type/API; Icom references document DTMF speed but no CI-V digit-transmit payload | Not used | Do not confuse Icom voice-memory command `28` with DTMF transmission |
| Antenna selection | Icom IC-7610 CI-V manual | IC-7610 `12` antenna selector is now profile-exposed as a typed U8 control; antenna-memory bands remain untyped | Not used | Antenna-memory records are frequency-range keyed |
| Band-stack and quick-memory operations | Vendor manuals | Not typed | Not used | Requires model-specific state and persistence semantics |
| Scope configuration and I/Q streaming | Icom manuals, some Yaesu manuals | Generic Icom scope configuration API plus shared I/Q sample decoding; IC-7300 has CI-V scope only; IC-7610 IQ is documented metadata only until a driver transport exists | Scope use is partial; I/Q not consumed | Waveform transport and receiver geometry are model-specific |
| Auto-information subscriptions | Vendor command references | Icom CI-V lifecycle-managed event router with typed frequency/mode/PTT/receiver events and raw fallback; Kenwood `AI` enable with typed frequency/PTT and raw fallback | Yaesu `AI` enable plus typed frequency/mode/PTT events and raw fallback | QSONaut still needs a general UI event consumer |

## QSONaut consumption summary

QSONaut currently consumes the following Rigwright surfaces in its native
radio worker/UI:

- frequency, mode, data-mode status, filter, PTT, and radio power workflow;
- AF gain, RF gain, squelch, RF power, preamp, attenuator, NB, NR, IP+, notch,
  AGC, and tuner controls where the selected profile advertises them;
- read-only tuner status;
- normalized SWR polling and the stepped SWR sweep workflow;
- native Icom scope data where the selected model/profile provides it.

QSONaut consumes every normalized meter that the selected Rigwright profile
advertises and renders those values in the native radio banner. Physical units
and calibrated SWR ratios remain limited: the HAL provides normalized meter
deflection, while QSONaut applies the documented IC-7300 SWR anchors only for
that model and otherwise displays the normalized level.

QSONaut also consumes modern Yaesu’s typed noise-reduction level control. It
does not yet consume RIT/XIT, Icom external-preamp or main/sub controls,
memory/channel operations, antenna selection, or generic auto-information
surfaces. Icom memory and repeater APIs are now available through Rigwright;
QSONaut still needs a dedicated UI consumer for them.

## Maintenance rule

When adding a capability, update all four locations when applicable:

1. the root `ControlId`/`MeterId` or backend-specific API;
2. the vendor/model profile and `supports_*` capability report;
3. this matrix, including manual filename and validation level;
4. QSONaut only if the UI/workflow actually consumes the capability.

Do not mark a manual-only feature as Rigwright-supported until its command,
response shape, scaling, model scope, and failure behavior have tests.
