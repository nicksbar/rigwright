# Changelog

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
