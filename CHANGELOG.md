# Changelog

## Unreleased — Structured repeater and channel controls

- Add protocol-neutral typed values for CTCSS tone mode/index, repeater shift,
  memory channels, and validated DTMF sequences.
- Add capability-gated HAL operations for repeater settings, memory/channel
  access, and DTMF transmission, including Android and configured-driver
  delegation.
- Implement the documented Yaesu FTDX10 `CN`, `CT`, and `OS` repeater
  operations; other models remain unavailable until their command manuals are
  individually profiled.

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
