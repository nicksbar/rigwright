# Rigwright implementation controls

These rules are part of the driver contract and must be preserved by future
agent changes:

- The root HAL contains protocol-neutral control IDs and values only.
- Generic protocol drivers contain transport, framing, validation, and shared
  execution. They must not grow model-name conditionals for command layout,
  value ranges, meter selectors, or optional capabilities.
- Every vendor follows the same two-layer rule: generic protocol code owns
  framing, transport, shared execution, and genuinely universal controls;
  model-specific modules own command differences, ranges, selectors, optional
  capabilities, and overrides through profile/spec tables. A control or meter
  is not advertised unless its selected profile contains the metadata needed
  to execute it.
- Every new model profile must add an architectural contract test covering its
  optional controls, meters, command widths, and unsupported surfaces.
- Run `cargo fmt --all`, `cargo test --locked`, `cargo clippy --locked
  --all-targets --all-features -- -D warnings`, and `git diff --check` before
  declaring a driver change complete.

Use the vendor model modules for model tables: Icom modules under
`src/icom/`, modern and classic Yaesu modules under `src/yaesu/`, and Kenwood
modules under `src/kenwood/`. Keep the shared vendor driver model-neutral.
