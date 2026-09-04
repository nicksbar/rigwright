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

## Support and evidence contract

The catalog and vendor profiles are the implementation source of truth. The
matrix is the maintained, human-readable projection of that source; it must
not introduce a capability that is absent from the selected profile or driver.

These dimensions are intentionally separate:

| Dimension | Meaning | Evidence location |
|---|---|---|
| Cataloged | The model is selectable in `POPULAR_RADIOS` with protocol and maturity metadata. | `src/models.rs` |
| Profiled | The vendor profile declares ranges, modes, controls, meters, and root capabilities. | Vendor profile modules under `src/` |
| Software-tested | Deterministic parser, profile, boundary, or fake-transport tests pass. | Vendor tests and the locked test suite |
| Hardware-validated | A physical radio was exercised and reviewed with model, firmware, transport, baud, and operating-state context. | Matrix row plus retained probe/capture evidence |
| Consumed | A consumer actually uses the typed surface in a workflow. | Consumer integration and this matrix |

`M`, `H`, and `P` describe documentation, HAL exposure, and profile gating;
they do not mean that a physical radio has been tested. `V` is reserved for
reviewed physical evidence. A passing unit test is software evidence, not
hardware evidence. Unsupported and read-only behavior must be represented
explicitly rather than inferred from a missing row.

Probe reports use the shared `ProbeLog` record shape: model, connection
parameters, timestamp, named `pass`/`fail`/`skip` records, and transport
metrics. Reports are diagnostic evidence, not automatic promotion to `V`;
review must confirm the selected model, firmware, transport, baud, and
operating state and remove unnecessary private details before sharing.

`M` alone is not a support claim. Vendor manuals describe many functions that
are not yet safe to expose generically because command selectors, payloads,
units, or model behavior differ.

### Profile completeness standard

Every selectable model profile must declare the same dimensions, including
explicit unsupported values: documented baud rates and preferred rate;
frequency ranges and modes; control commands, read/write direction, maxima and
discrete legal values; meter selectors, raw ranges, widths, polling and
presentation; and native scope/waterfall metadata when implemented. If a
manual documents a surface that is not implemented, the profile and its
documentation must say so and identify any accessory-owned boundary.

Drivers must delegate discovery and validation to these profile facts. Clients
must use `supports_control_read()`, `supports_control_write()`,
`supported_control_values()`, `control_max()`, `meter_metadata()`, and
`meter_poll_spec()` rather than recreating vendor rules. Parser tests are
software evidence; they do not constitute hardware validation.

## Issue #20 session execution

| Area | Rigwright status | Client contract |
|---|---|---|
| Driver-owned worker/state machine | H | `RadioSession` owns execution and lifecycle status |
| Bounded queue/backpressure | H | `QueueFull` is returned before admission |
| Duplicate/coalesced commands | H | Pending same-key intent supersedes older waiters |
| Safety priority | H | PTT writes are scheduled ahead of ordinary state work |
| Desired/observed/pending state | H | Every ticket resolves to a `RadioSnapshot` |
| Polling and driver events | H | Worker refreshes and consumes vendor event routers |
| Recovery | H | Backend errors produce `Degraded`; later success returns `Ready` |
| Profile baud choices | H | Model catalog exposes documented choices and fastest selection |
| Automatic baud probing | M / future transport | Requires transport-level negotiation before CAT traffic |

This is a Rigwright HAL capability and does not imply QSONaut or QSONoid
integration. No qsonaut-modems or qsonaut-third-party change is required.

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
| Spectrum waveform | backend-specific scope API | H/P; model geometry differs | — | — | — | — for transceivers; P3/PX3 are separate components |
| I/Q stream | model/backend-specific | Shared I/Q sample block decoder only; IC-7610 documents USB I/Q output, but Rigwright does not yet own/open that transport | — | — | — | Not currently consumed |

### Universal HAL caveat

The method names are universal; the hardware support is not. Applications must
check `Radio::capabilities()`, `supports_control()`, and `supports_meter()` as
appropriate. A generic vendor driver deliberately reports no optional typed
controls or meters until a concrete model profile is selected.

### Icom CI-V transport hardening

The IC-7200 is now cataloged at framework maturity from the local
`IC-7200_ENG_CD_0b.pdf` manual. Its profile covers documented HF/50 MHz CI-V
frequency and mode operations, RIT/split/VFO/memory/tuner paths, receiver
controls, tuning-step selection, and signal/power/SWR/ALC meters. It has no
native scope profile and remains untested on physical hardware.

The IC-718 is also cataloged at framework maturity from the local
`IC-718 ADVANCED MANUAL 2024.pdf`. Its profile uses CI-V address `5E` and
covers the manual's HF/50 MHz frequency and mode, VFO, split, tuning-step,
AGC, preamp, noise, attenuator, RF power, memory, and S-meter surfaces. It
does not advertise scope, repeater, or I/Q support, and remains untested on
physical hardware.

| Transport behavior | Status |
|---|---|
| One in-flight CI-V transaction | H; deliberately retained because CI-V frames have no transaction ID |
| Immediate completion on decoded response | H; host-side inter-frame delay removed |
| Persistent bounded inbox for unmatched radio frames | H; preserves interleaved replies for later commands |
| USB echo-back filtering | H; echoed outbound frames are discarded and counted |
| Scope-frame separation | H; waveform frames remain in the scope assembler and do not fill CAT inbox state |
| Link-health metrics | H; counters and cumulative response time exposed by `IcomCiVRadio::transport_metrics()` |
| Adaptive timeout and opt-in baud/address probing | H; adaptive deadlines are bounded and probing only tries caller-supplied candidates |

### Remaining vendor transport hardening

| Vendor | Low-latency reads | Bounded retained frames | Link metrics | Remaining |
|---|---:|---:|---:|---|
| Modern Yaesu CAT | H | H | H via `YaesuTransportMetrics` | Model verification is opt-in via `verify_model`; FTDX10 RTS probing remains advisory |
| Kenwood PC control | H | H | H via `KenwoodTransportMetrics` | Model verification is opt-in via `verify_model` |
| Classic Yaesu CAT | H for fixed binary transaction metrics | — | H via `LegacyYaesuTransportMetrics` | Fixed 8N2/no-flow policy is exposed; binary probing is intentionally not automatic |
| Elecraft CAT | H | H | H via `ElecraftTransportMetrics` | `identify`/`probe_options` remain explicit and model-scoped |

## Elecraft first implementation slice

| Model profiles | Support level | Profile module | Manual evidence |
|---|---|---|---|
| K2 | Framework | `src/elecraft/k2.rs` | `KIO2 Pgmrs Ref rev E.pdf` |
| KX2 | Framework | `src/elecraft/kx2.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| KX3 | Framework | `src/elecraft/kx3.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K3 | Framework | `src/elecraft/k3.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K3S | Framework | `src/elecraft/k3s.rs` | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` |
| K4 | Framework | `src/elecraft/k4.rs` | `K4 Programmer's Reference, rev. D5.pdf` |
| KH1 | Framework | `src/elecraft/kh1.rs` | `Elecraft KH1 Programmer's Ref, rev B2.pdf` |

| Operation | HAL surface | Elecraft status |
|---|---|---:|
| Frequency read/write | `get/set_frequency_hz` | H/P for K2/KX2/KX3/K3/K3S/K4; KH1 write-only |
| Operating mode read/write | `get/set_mode` | H/P for K2/KX2/KX3/K3/K3S/K4; KH1 write-only |
| PTT write/read | `set_ptt`, `get_ptt`, `get_actual_tx_state` | H/P via `TX`/`RX` and `TQ`; K4 actual-RF state also via `TQX` |
| RF power | `ControlId::RfPower` | H/P via profile-scaled `PC`; K4 uses documented `PCnnnH` range framing |
| VFO selection / split | `ControlId::{Vfo,Split}`, `get/set_vfo_frequency_hz` | H/P via `FR`/`FT` and independent `FA`/`FB`; K3/K3S receive-selection semantics remain distinct |
| RIT/XIT enable and offset | `ControlId::{Rit,Xit}`, offset methods | H/P via `RT`/`XT`/`RO`/`IF` |
| Signal meter | `MeterId::Signal` | H/P via `SM` |
| AF gain | `ControlId::AfGain` | H/P via `AG` |
| RF gain | `ControlId::RfGain` | H/P via profile-scaled `RG` |
| Squelch | `ControlId::Squelch` | H/P via profile-scaled `SQ` |
| Preamp / attenuator | `ControlId::{Preamp,Attenuator}` | H/P via profile-owned `PA`/`RA` ranges |
| Antenna selection | `ControlId::Antenna` | H/P via profile-owned `AN` connector limits for K2/KX2/KX3/K3/K3S/K4 |
| Auto/manual notch | `ControlId::{Notch,ManualNotch,ManualNotchPosition}` | H/P for K4 via `NA$`/`NM$`; manual position is normalized from documented 150–5000 Hz |
| Noise blanker | `ControlId::NoiseBlanker` | H/P via `NB` enable state |
| AGC | `ControlId::Agc` | H/P via `GT` fast/slow mapping |
| Filter bandwidth | `ControlId::Filter` | H/P via model-owned `BW`/`FW` bandwidth mapping |
| Tuning step / VFO movement | `ControlId::TuningStep`, `move_vfo` | H/P for K4 via `VT$`; K2/K4 current-step `UP`/`DN` and K3-family indexed `UP`/`DN` movement implemented |
| Internal tuner mode/start | `ControlId::Tuner`, `start_tuner` | H/P for K4 via `AT`/`TU3`; other models remain profile-gated |
| Repeater shift/offset | `RepeaterSettings` | H/P for K4 via `RP`; tone fields remain unsupported |
| Memory/channel selection | `select_memory_channel` | H/P for KX2/KX3/K3/K3S via `MC`; full record read/write remains open |
| Raw protocol | `protocol_write_read` | H/P |
| TX meters | `MeterId::{Power,Alc,Swr,Compression}` | K4 signal/power queries use documented `SM$`/`PO`; K3/K3S queried `BG` plus `TM0`/`TM1`; K4 ALC/compression/SWR are typed unsolicited `TM` events, not polled `SW` |

Elecraft profile differences currently cover K2 versus K3-family mode tables,
model-specific baud lists, conservative HF frequency ranges, and normalized
S-meter limits. The model-specific declarations live in the seven modules listed
above; shared profile validation remains in `src/elecraft/profile.rs`.
Auto-Info event routing is now available through the shared event router. K4
Ethernet/streaming and precise VFO-B semantic differences remain outside this
direct transceiver scope. KH1 is intentionally limited to fixed-baud, write-only
frequency/mode control; its display-mediated status and `FO`/`HK` FT8/CW
operations require a separate capability surface.
KH1 and P3/PX3/KAT/KPA/KXPA equipment are separate future profiles or station
components and are not included in this row.

### Direct CAT control backlog and evidence gate

The following controls are known from the local Elecraft programmer references
and are tracked with implementation and tester evidence separately. “Manual”
means the command family is known enough to design against; it is not proof of
correct code or physical behavior.

| Direct CAT surface | Current Rigwright status | Required implementation evidence | Physical tester evidence |
|---|---|---|---|
| RF power (`PC`) | Implemented in driver; framework-level | Profile-native limits, normalized `RfPower`, read/write fixtures, TX-safety tests | Readback and safe min/max power on each model |
| VFO-A/B and selected-VFO routing (`FA`/`FB` plus selection) | Independent `FA`/`FB` operations and `FR`/`FT` selection implemented; K3/K3S receive-selection semantics remain distinct | Explicit VFO state and command-routing tests | Both VFOs, switching, and unsolicited updates |
| Split | Implemented in driver; framework-level | Profile-gated `ControlId::Split` and selected-VFO tests | RX/TX VFO behavior and split transitions |
| RIT/XIT | Implemented in driver; framework-level | Signed offset/enable contract and boundary tests | Sign, range, zero, and independent operation |
| Tuning step | K4 step selector plus K2/K4 current-step and K3-family indexed VFO movement implemented; no generic legacy step-size readback | HAL shape and model-specific value tests | Every supported step on a physical dial/navigation workflow |
| Filters/bandwidth | Implemented as normalized bandwidth; framework-level | Named/value profile tables and readback fixtures | Accepted values and mode-dependent behavior |
| AGC | Implemented in driver; framework-level | Model-specific control encoding and capability tests | AGC choices and readback |
| Noise blanker/reduction | NB enable implemented; K4 `NB$`/`NR$` levels implemented; reviewed non-K4 references do not provide a lossless typed NR CAT surface | Separate enable/level controls and range tests | Level behavior and mode interaction |
| Preamp/attenuator | Implemented in driver; framework-level | Distinct profile controls and mutual-exclusion tests | RF-path state and combinations |
| Internal tuner | K4 mode/start implemented; KAT/KXAT accessories remain separate components and other transceiver profiles are gated | Tuner state model, explicit-start path, and failure tests | Tune start/completion/failure and TX interlock |
| Memory/channel operations | KX2/KX3/K3/K3S selection implemented via `MC`; reviewed transceiver references do not provide lossless record read/write framing and K4 `MC` is pending | Lossless `MemoryChannel` mapping only if a documented record surface appears; otherwise explicit unsupported fields | Empty/read/write/name/mode/frequency round trips |
| Repeater/tone | K4 shift/offset implemented; reviewed transceiver references do not expose typed tone payloads | Profile-gated `RepeaterSettings` and unsupported-field tests | Tone, offset, and model-specific repeater behavior |
| TX status and additional meters | Power/ALC/SWR implemented where the protocol permits; K4 actual TX state via `TQX`, K3/K3S `TM` source selection, K4 `SM$`/`PO` queries, and K4 `TM` ALC/compression/power/SWR event reports implemented; voltage/current/temperature remain unavailable in the transceiver HAL | Typed status/events, normalized meter fixtures, read-only semantics | RX/TX/tune captures for power/SWR/ALC/etc. |
| Identification and capability probing (`ID`/`OM`/status) | `ID` and decoded model-specific `OM` probes implemented for K3/K3S/KX2/KX3/K4; KH1 has no reviewed `OM` schema | Bounded probe, unknown-model, timeout, malformed-frame, and family-parser tests | Known model, firmware, and installed-option identification |

Control bounds and discrete choices are part of the profile-owned surface in
the rows above. Consumers such as QSONaut must obtain them through
`RadioModelProfile::control_max` and `supported_control_values`; application
code may choose presentation, labels, and safety policy but must not recreate
vendor ranges.

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

| HAL control | Value | Icom CI-V | Modern Yaesu | Classic Yaesu | Kenwood | Elecraft | QSONaut native use |
|---|---|---:|---:|---:|---:|---:|---|
| `AfGain` | normalized `U8` 0–255 | RW/P all profiled models | M, not typed | M, not typed | RW/P all three profiles | RW/P profile-native maximums | Q slider |
| `RfGain` | normalized `U8` 0–255 | RW/P all profiled models | M, not typed | M, not typed | RW/P all three profiles | RW/P profile-native maximums; attenuation direction is profile-owned | Q slider |
| `Squelch` | normalized `U8` 0–255 | RW/P all profiled models | M, not typed | M, not typed | RW/P all three profiles | RW/P profile-native maximums | Q slider |
| `RfPower` | normalized `U8` 0–255 | RW/P all profiled models; exact watts also available | RW/P modern profiles; exact watts also available | M, not typed; power write intentionally absent | RW/P all three profiles; exact watts also available | RW/P profiled models; native watt limits remain model-specific | Q slider and SWR sweep power |
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
| `Rit` | Icom `21 01`; model-specific elsewhere | R/W on all profiled Icom models; IC-7300 live RIT operations are mode-dependent and are rejected by the connected USB-D/Data configuration | M | M | RW/P all three profiles | Icom profile implementation; probe reports the IC-7300 Data-mode restriction explicitly |
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
| FT-991A | Full modern typed-control set, repeater controls, full memory records, and profile-gated split |
| FT-817ND, FT-818, FT-857D, FT-897D | Split (read/write), RIT/clarifier and repeater shift/tone/offset (write-only), VFO toggle, CAT lock |
| TS-590SG | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, filter A/B, RIT/XIT, VFO, split, tuner, signal/power/SWR/ALC/COMP meters, AI, and memory records |
| TS-890S | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, filter A/B/C, AGC, RIT/XIT, VFO, split, tuner, signal/power/SWR/ALC/COMP/current/voltage/temperature meters, AI, and memory records |
| TS-2000 | AF/RF gain, squelch, RF power, preamp, NB, NR, notch, RIT/XIT, VFO, split, tuner, signal/power/SWR meters, and AI; filter and memory records remain unprofiled |

## Normalized meters

All typed meter values are normalized to a HAL deflection level of 0–255. This
is not a universal physical-unit conversion. For example, Icom SWR values have
documented ratio anchors, but those anchors are not shared by Yaesu or Kenwood.

| HAL meter | Icom CI-V | Modern Yaesu CAT | Classic Yaesu CAT | Kenwood | Elecraft | QSONaut native use |
|---|---:|---:|---:|---:|---:|---|
| `Signal` | R/P via `15 02`; IC-7300 V | R/P via `RM1` | R/P via `E7`, 0-15 | R/P via `SM`, profile max 30 or 70 | R/P profile-native maximums | Q normalized meter panel where advertised |
| `Power` | R/P via `15 11`; IC-7300 V | R/P via `RM5` | R/P via `F7`, 0-15 | R/P via `SM` (TX), profile max 30 or 70 | R/P `BG`/`PO`, profile-native maximums | Q normalized meter panel where advertised |
| `Swr` | R/P via `15 12`; IC-7300 V | R/P via `RM6` | M, not typed | R/P via `RM`; selector and range profile-specific | R/P where model profile exposes it | Q live meter and stepped SWR chart |
| `Alc` | R/P via `15 13`; IC-7300 V | R/P via `RM4` | M, not typed | R/P TS-890S via `RM1` | R/P K3/K3S and K4 event surface | Q normalized meter panel where advertised |
| `Compression` | R/P via `15 14`; IC-7300 V | R/P via `RM3` | M, not typed | R/P TS-890S via `RM3` | R/P K4 event surface | Q normalized meter panel where advertised |
| `Current` | R/P via `15 16`; IC-7300 V | R/P via `RM7` | M, not typed | R/P TS-890S via `RM4` | M, not typed | Q normalized meter panel where advertised |
| `Voltage` | R/P via `15 15`; IC-7300 V | R/P via `RM8` | M, not typed | R/P TS-890S via `RM5` | M, not typed | Q normalized meter panel where advertised |
| `Temperature` | Manual/protocol surface varies; intentionally not profiled | R/P FTDX101D/MP via `RM9`; not exposed by FT-710, FTDX10, or FT-991A | M, not typed | R/P TS-890S via `RM6` | M, not typed | Q normalized meter panel where advertised |

Normalization policy: all linear HAL levels use the common 0..255 scale and
half-up rounding in both directions. Vendor-native maxima and model-specific
power ranges remain profile facts. Generic or undocumented meters are not
forced into physical units; they remain normalized or unavailable.

### Model normalization facts

| Vendor/models | Native source | HAL treatment |
|---|---|---|
| Icom IC-705, IC-718, IC-7200, IC-7300, IC-7610, IC-9700 | CI-V level BCD values already encoded as 0–255 | Exact 0–255 decode; model profiles gate availability |
| Modern Yaesu FT-710, FTDX10, FTDX101D, FTDX101MP, FT-991A | `RM` meters 0–255; `PC` power has model watt range | Exact meter values; power maps profile minimum/maximum watts to 0–255 |
| Classic Yaesu FT-817ND, FT-818, FT-857D, FT-897D | `E7`/`F7` meter dots 0–15 | Profile-independent half-up scaling to 0–255 |
| Kenwood TS-590SG, TS-890S, TS-2000 | `SM`/`RM` meter dots, profile maxima 30 or 70 | Half-up scaling by the selected model profile; power maps 5–100 W |
| Elecraft K2, KX2, KX3, K3, K3S, K4 | Model-native `SM$`, `BG`, `SW`, `PO`, and profile control maxima | Half-up scaling using model maxima; K4/K3-family meter availability remains profile-gated |
| Generic Icom/Yaesu/Kenwood adapters and DX Lab/rigctld | No authoritative model range | Expose only protocol-neutral values they can prove; no guessed physical calibration |

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
