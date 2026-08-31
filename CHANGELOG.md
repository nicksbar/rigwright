# Changelog

## 0.1.17 — Elecraft transceiver foundation

- Document the Elecraft boundary between K2/KX/K3/K4/KH1 transceivers and
  separate P3/PX3, KAT, and KPA/KXPA station components.
- Add a profile-driven semicolon CAT transceiver backend for K2, KX2, KX3, K3,
  K3S, and K4 core operations.
- Route Elecraft Auto-Info frequency, mode, PTT, AF-gain, signal-meter, and raw
  frames through the shared Rigwright event router.
- Add profile-scaled Elecraft RF-gain and squelch controls through the common
  normalized HAL value range.
- Split the K2/KX2/KX3/K3/K3S/K4 declarations into focused per-model profile
  modules, leaving shared contracts and driver behavior separate.
- Record the direct-CAT backlog, command-family audit, implementation evidence,
  and physical-tester gate in the Elecraft matrix and contributor guidance.
- Implement the first direct-CAT control slice: identification query, RF power,
  VFO/split routing, and RIT/XIT enable and offsets.
- Implement profile-owned preamp, attenuator, noise-blanker, and AGC controls
  using the documented `PA`, `RA`, `NB`, and `GT` command families.
- Implement model-owned filter bandwidth and power/ALC/SWR meter decoding via
  the documented `BW`/`FW`, `BG`, and `SW` command families.
- Implement K4 internal-tuner mode/status and explicit `TU3` start handling.
- Implement K4 repeater shift/offset read/write through the documented `RP`
  command while rejecting unsupported tone fields.
- Add the K4 mode-qualified `VT$` tuning-step control and response matching;
  legacy-family step writes remain explicitly profile-gated.
- Add K4-native level-bearing noise blanker and noise reduction controls via
  `NB$`/`NR$`, preserving configured levels when toggling either feature.
- Add a bounded raw `OM` option probe alongside `ID`, retaining family-specific
  option interpretation for later model capability negotiation.
- Add KX2/KX3/K3/K3S memory-channel selection through the documented three-digit
  `MC` command while keeping undocumented record persistence unsupported.
- Decode K4 unsolicited `TM` TX-meter frames into typed ALC, power, and SWR
  events and expose the documented `TM1`/`TM0` reporting toggle.
- Add explicit independent Elecraft VFO-A/VFO-B frequency read/write methods
  using `FA`/`FB`, while keeping receive/transmit selection semantics separate.
- Add K3/K3S transmit meter-source selection through documented `TM0`/`TM1`
  commands, keeping it distinct from K4's streaming `TM` reports.
- Preserve the K4 `TM` compressor field as a typed normalized compression-meter
  event alongside ALC, forward power, and SWR.
- Keep KH1, K4 streaming/Ethernet, and tuner/amplifier/panadapter components
  outside this initial radio slice.

## 0.1.16 — model-correct meter capabilities

- Correct the IC-7300 capability profile to omit temperature, which is
  available on the radio's front-panel meter but has no documented CI-V
  remote-meter command (#83).
- Expose the documented `RM9` temperature meter for FTDX101D and FTDX101MP,
  while keeping it unavailable on FT-710, FTDX10, and FT-991A.
- Add regression coverage and refresh the capability documentation for the
  modern Yaesu, classic Yaesu, and Kenwood meter surfaces.

## 0.1.15 — Icom IP+ profiles and resilient Yaesu RTS probing

- Correct Icom IP+ capability ownership: expose the documented `1A 07`
  control on the IC-7300, IC-7610, and IC-9700 profiles while keeping it
  unavailable on the IC-705 profile.
- Make FTDX10 CAT RTS detection advisory when the radio or USB bridge does
  not answer the bounded menu probe, allowing ordinary CAT commands to
  continue after the fallback flow-control attempt.
- Add regression coverage for unanswered RTS probes and refresh the measured
  README coverage snapshot to 85.13% overall, including 76.74% modern Yaesu
  CAT coverage.

## 0.1.14 — Icom and Yaesu capability consolidation

- Add repository ownership, Dependabot updates, an owner-specific MIT license
  notice, and weekly/pull-request CodeQL security analysis.
- Document the local LLVM coverage workflow and add pull-request-only CI for
  locked tests, LLVM coverage summaries, and uploaded HTML coverage reports.
- Add focused contract tests for every previously uncovered model entry-point
  module and raise the suite to 193 tests; all former 0% production files now
  have exercised executable lines.
- Add a changed-production-file coverage gate so a new or modified `src/**/*.rs`
  file with no covered executable lines fails the pull request instead of being
  hidden by aggregate area coverage.
- Keep the README's per-area coverage labels, release/version metadata, and
  coverage snapshot synchronized with the enforced LLVM results.
- Harden the generic/profile boundary across the vendor drivers: shared
  protocol engines own framing and execution, while model modules own command
  exceptions, ranges, selectors, and optional capabilities.
- Complete the Icom profile split for IC-705, IC-7300, IC-7610, and IC-9700;
  remove duplicated common controls and profile the model-specific memory,
  meter, VFO, repeater, and special-control surfaces.
- Add generic-versus-profile capability tests and fake-transport exact-frame
  regression tests for common RF power plus IC-7300 XIT, IC-7610 antenna, and
  IC-9700 AGC controls.
- Reconcile the Icom capability matrix with the manuals, including the `15`
  meter selectors and the `16 48` manual-notch command. Framework profiles
  remain distinct from physical-radio validation.
- Promote the common Icom CI-V control definitions and meter selectors into
  the shared profile/generic driver layer.
- Complete live Icom duplex repeater state: read/write offset frequency via
  `0C`/`0D` and simplex/DUP−/DUP+ state via `0F`.
- Keep IC-7300-specific scope geometry, limits, and exceptional controls in
  its model profile.
- Document the Icom IQ boundary: IC-7300 CI-V scope is supported, while raw
  USB IQ remains unavailable on that model.
- Promote documented modern Yaesu `CN`, `CT`, and `OS` repeater controls to all
  current modern profiles, including the required main-band selector in `OS`.
- Generalize modern Yaesu `MC`, `MR`, and `MT` memory-channel records across
  current modern profiles, and keep live `OS` direction distinct from memory
  offset data.
- Add modern Yaesu typed AF/RF gain, squelch, preamp, attenuator, NB, notch,
  filter-width, RIT/XIT, VFO, and tuner controls, plus AI event routing.
- Expand Kenwood PC control with profile-aware AF/RF gain, squelch, preamp,
  noise blanker, noise reduction, notch, filters, RIT/XIT offsets, VFO A/B,
  tuner control, Auto Information event routing, TX power metering, TS-590SG
  memory/ALC/compression support, and TS-890S
  ALC/compression/current/voltage/temperature meters.
- Complete the classic Yaesu binary CAT surfaces available in the documented
  opcode table: normalized signal/power meters, clarifier enable/offset writes,
  and repeater shift, offset-frequency, CTCSS, and DCS writes. Readback remains
  limited to the documented status operations; classic memory and event
  controls are not exposed because the protocol does not provide them.
- Add explicit classic-driver helpers for the documented CAT lock and VFO-A/B
  toggle commands, whose state cannot be read back through binary CAT.

- Add protocol-neutral typed values for CTCSS tone mode/index, repeater shift,
  memory channels, and validated DTMF sequences.
- Add capability-gated HAL operations for repeater settings, memory/channel
  access, and DTMF transmission, including Android and configured-driver
  delegation.
- Implement the documented modern Yaesu `CN`, `CT`, and `OS` repeater
  operations with model-profile gating and correct main-band `OS` framing.
- Implement Icom CI-V repeater tone state/frequency and documented memory
  channel selection, plus Kenwood `CN`/`CT` CTCSS controls.
- Correct Icom tone mappings to the documented `16 42/43` enable flags and
  `1B 00/01` CTCSS/TSQL frequency records; correct the generic Icom manual
  notch mapping to `16 48`.
- Implement Icom `1A 00` memory record reads/writes for channel, frequency,
  mode/data, tone/TSQL fields, and the documented 10-character memory name.
- Add documented Icom RIT enable control across all profiled models and XIT
  enable control for the IC-7300 and IC-7610 profiles.
- Add protocol-neutral signed RIT offset read/write using the documented Icom
  `21 00` packed-BCD payload.
- Record the Icom CI-V DTMF boundary: the profiled guides expose DTMF speed
  configuration, but no documented CI-V digit-transmit command.
- Extend Icom VHF/UHF memory records for IC-705 and IC-9700 with band,
  duplex, DTCS, offset, and 16-character name fields.
- Add IC-7300/IC-7610 NR-level and manual-notch-position controls, plus the
  IC-7610 antenna selector control.
- Add lifecycle-managed unsolicited CI-V event subscriptions for frequency,
  mode, PTT, receiver, and raw state notifications.
- Add a generic Icom scope configuration API for span, fixed edges, hold,
  reference level, sweep speed, center mode, and VBW.
- Add transport-neutral interleaved I/Q sample blocks and PCM16/PCM24/float
  decoding helpers; model-specific raw I/Q transport negotiation remains
  profile-owned.
- Add an explicit Yaesu hardware-flow-control constructor for CAT RTS-enabled
  serial interfaces while preserving the no-flow-control default.
- Automatically read the FTDX10 CAT RTS menu value and adapt the serial
  adapter's RTS/CTS flow control without requiring a command-line flag.
- Persist the detected RTS/CTS choice and retry the initial FTDX10 `MD;` mode
  query once when the radio rejects or times out on the first request.
- Retry the FTDX10 CAT RTS menu probe itself with RTS/CTS when the initial
  no-flow-control probe cannot receive a response.
- Make the modern Yaesu probe complete identity, frequency, mode, and PTT
  checks independently before returning a combined failure summary.
- Correct FTDX10 mode reads to include the documented Main-band receiver
  selector (`MD0;`) instead of issuing the invalid bare `MD;` query.
- Record the FTDX10 Enhanced CAT path as hardware-validated based on physical
  radio testing.
- Implement Kenwood TS-890 memory selection and structured `MA0` memory
  record read/write support, including channel names and split frequencies.
- Implement FTDX10 `MR` memory reads and `MT` memory writes for documented
  frequency, mode, tone, shift, offset, and tag fields.

## 0.1.13 — Directional control capabilities

- Add independent `supports_control_read()` and
  `supports_control_write()` capability queries.
- Mark Icom VFO A/B selection as write-only because the documented CI-V
  selector has no reliable active-VFO readback command.
- Preserve readable and writable status for the existing Yaesu and Kenwood
  typed controls.
- Expose the directional capability contract through the Android adapter.

## 0.1.12 — IC-7300 telemetry meters

- Add IC-7300 CI-V meter queries for signal, output power, ALC, SWR,
  compression, voltage, current, and temperature.
- Expose the documented meters through the profile-gated `MeterId` capability
  surface using normalized `0..=255` values.
- Add coverage for the documented CI-V meter command selectors.

## 0.1.11 — Android transport plumbing

- Add the shared `RadioTransport` byte-stream contract while preserving the
  existing `Radio` HAL and desktop serial constructors.
- Add `RadioAndroid` with Icom CI-V, modern Yaesu CAT, classic Yaesu CAT, and
  Kenwood CAT family entry points.
- Allow each current driver to operate over an externally configured transport,
  including an Android USB Host or Bluetooth adapter supplied by the caller.
- Preserve protocol-specific framing, response matching, capability profiles,
  stale-input handling, and desktop serial behavior.
- Add injected-transport and fragmented-read coverage for the Android entry
  point; retain the full existing driver test suite.

Android transport implementations and physical radio validation remain the
responsibility of the consuming application. The current hardware-validated
Rigwright baseline remains the Icom IC-7300 desktop serial path.

## 0.1.10

- See the Git history for the prior release changes.
