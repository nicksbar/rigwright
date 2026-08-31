# Elecraft component architecture

Elecraft should be added as a family of composable protocol components, not as
one large radio driver. The local programmer references show several related
ASCII command dialects and several kinds of equipment that are not radios.

## What the local references establish

| Component family | Local reference | Role | Protocol boundary |
|---|---|---|---|
| K2 | `KIO2 Pgmrs Ref rev E.pdf` | Transceiver | K2 command set, semicolon framed |
| KX2/KX3/K3/K3S | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` | Transceivers | Shared K3-family commands, with model applicability markers |
| K4 | `K4 Programmer's Reference, rev. D5.pdf` | Transceiver | K3-compatible command lineage plus Ethernet, multi-client, and streaming data |
| KH1 | `Elecraft KH1 Programmer's Ref, rev B2.pdf` | Transceiver | Small semicolon-framed subset; many operations are UI emulation or SET-only |
| P3/PX3 | `P3_Pgmrs_Ref_Rev_A7.pdf`, `PX3_Pgmrs_Ref_A6.pdf` | Panadapter/spectrum instruments | `#`-prefixed commands; optional downstream transceiver pass-through |
| KAT500 | `KAT500 Automatic Antenna Tuner Serial Command Reference.pdf` | Antenna tuner | Own semicolon-framed command set; frequency tracking and tuning state |
| KPA500 | `KPA500 Programmers Ref.pdf` | Amplifier | `^`-prefixed command set; power, fault, and operate/standby state |
| KXPA100 | `KXPA100 Amplifier Command Reference.pdf` | Amplifier and optional tuner host | `^`-prefixed command set; can forward non-`^` transceiver commands |
| KIO2 | `KIO2 Pgmrs Ref rev E.pdf` | K2 serial interface context | Interface documentation, not a separate radio model |

The references consistently use semicolon termination, but that does not make
their command semantics interchangeable. Some devices have fixed baud rates,
some permit baud changes, and K4 additionally exposes Ethernet and streaming
data. Responses, command widths, readback, and optional hardware must remain
profile-owned.

## Proposed Rigwright shape

1. Add a shared Elecraft ASCII transport for serial and, later, a TCP byte
   stream. It should own semicolon framing, bounded response matching,
   unsolicited-response handling, and transaction serialization.
2. Add an `elecraft::transceiver` backend implementing `Radio`. Its model
   profile should describe command forms, frequency/mode fields, readback,
   baud options, VFO/subreceiver targeting, meters, and optional controls.
3. Keep each transceiver profile in its own focused module:
   `src/elecraft/k2.rs`, `kx2.rs`, `kx3.rs`, `k3.rs`, `k3s.rs`, `k4.rs`, and
   `kh1.rs`.
   `profile.rs` contains only shared contracts and lookup; `transceiver.rs`
   contains shared commands. Model applicability, widths, ranges, and
   exceptions belong in the model files. K4 streaming and Ethernet should be
   optional transport capabilities, not requirements of `Radio`.
4. Treat KH1 as a separate profile only where the command semantics genuinely
   match. Its documented SET-only `FA` and `MD`, display-mediated status, and
   `FO`/`HK` FT8 keying must not be represented as ordinary readable frequency,
   mode, or generic digital-transmit operations without explicit capability
   metadata.
5. Do not put P3/PX3, KAT500, KPA500, or KXPA100 in `ConfiguredRadio`. They are
   station components. Add protocol-neutral accessory traits only when a
   consumer needs them, for example `AntennaTuner`, `Amplifier`, and
   `SpectrumSource`; each Elecraft device can then implement the relevant
   trait while retaining its own profile and command namespace.
6. Model composition outside the root `Radio`: a station may contain one
   transceiver plus zero or more tuner, amplifier, and spectrum components.
   Accessory discovery and association should be explicit, especially for
   KXPA100 forwarding and P3/PX3 downstream transceiver connections.

## First implementation slice

The first slice is the shared K3-family transceiver path for KX2/KX3, K3/K3S,
and K4 over the existing `RadioTransport`. Its current contract coverage is
`ID`, `FA`/`FB`, `FR`/`FT`, `MD`, `TX`/`RX`, `TQ`, `RO`, `RT`, `XT`, `PC`, `SM`,
and `AI`. K4 Ethernet and streaming should follow as a separate
capability-tested transport layer.

## Direct CAT control roadmap

The local manuals give us enough information to plan the remaining direct CAT
surface, but a manual citation is not an implementation or a hardware-support
claim. Every row below needs three separate outcomes: a model/profile contract,
deterministic transport tests using captured or authored frames, and a physical
tester before the model can be promoted beyond `Framework`.

| CAT surface | Likely Elecraft command family | Rigwright work needed | Tester requirement |
|---|---|---|---|
| RF power | `PC` | Implemented as normalized profile-scaled control; tester coverage remains open | Confirm readback, minimum/maximum power, and behavior while transmitting |
| VFO-A/B and independent operations | `FA`/`FB`, VFO selection commands | Implemented with independent VFO frequency methods plus profile-gated selection | Exercise both VFOs, switching, persistence, and unsolicited updates |
| Split | VFO selection plus split/status commands | Implemented through profile-gated `ControlId::Split`; preserve selected-VFO context | Verify transmit VFO, receive VFO, and split on/off without changing activity context |
| RIT/XIT | RIT/XIT enable and offset commands | Implemented for signed offsets and enable controls where profiled | Verify sign, range, zeroing, and independent RIT/XIT behavior |
| Tuning step | `VT$`, `UP`/`DN` and `UPB`/`DNB` | K4 typed step control plus model-specific VFO movement implemented; legacy step-size readback remains unavailable | Verify each supported step and its effect on tuning/navigation |
| Filter and bandwidth | Filter/width command family | Implemented with model-owned normalized `BW`/`FW` ranges; named-filter fidelity remains open | Confirm accepted values, readback, and mode-dependent limits |
| AGC | AGC command family | Implemented through profile-owned `GT` mapping | Exercise mode-specific AGC choices and readback |
| Noise blanker/reduction | NB/NR command families | NB enable is implemented broadly; K4 level-bearing `NB$`/`NR$` controls are implemented | Confirm level ranges, interaction with modes, and persistence |
| Preamp/attenuator | Preamp/attenuation command families | Implemented with distinct profile controls and ranges | Verify RF-path state and mutually exclusive combinations |
| Internal tuner | Tuner enable/status/start commands | K4 mode/status/start implemented with explicit command path; other models remain gated | Confirm tuning start, completion, failure, and TX interlock behavior |
| Memory/channel operations | Memory select/read/write command families | KX2/KX3/K3/K3S selection implemented via `MC`; lossless records remain open | Verify empty slots, names, mode, frequency, and write/read round trips |
| Repeater/tone | Tone, offset, and repeater command families | K4 shift/offset implemented; tone fields remain explicitly unsupported | Verify tone modes, CTCSS/DCS, offsets, and VHF/UHF model behavior |
| Transmit status and meters | TX/status plus power/SWR/ALC/voltage/current families | Generic queried meters plus typed K4 `TM` TX events implemented; further status remains open | Capture idle, RX, tune, and TX readings under safe test conditions |
| Identification and probing | `ID` and model/status queries | Raw `ID` and model-specific `OM` probes implemented; interpretation remains profile-specific | Test known models, unknown firmware, timeout, and malformed replies |

The implementation order should be: identification/probing, VFO context, split
and RIT/XIT, RF power, then receiver controls and meters. Tuner start and any
other transmit-capable command must have an explicit operator action and must
not be triggered by background polling. The driver must continue to preserve
the application-selected activity mode while CAT navigation changes radio
state.

Until testers are available, we should still implement the profile contracts,
command encoders/decoders, captured-frame tests, and explicit unsupported
capabilities. We should not mark a model `Hardware validated` based on manual
review or loopback tests alone.

KH1 has a separate limited profile. Its fixed 9,600-baud, SET-only `FA`/`MD`
surface is implemented as write-only frequency/mode support; display-mediated
readback, `HK` CW keying, and `FO` FT8 offset control remain separate APIs to
avoid falsely advertising ordinary `Radio` semantics. Accessories should be
added after the station-component trait shape is settled; otherwise
tuner/amplifier controls risk leaking into the radio HAL.

All Elecraft profiles begin at `Framework` maturity. The local manuals prove
command documentation and parser contracts, not physical-radio behavior.
