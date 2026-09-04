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
- Profile completeness is mandatory: record baud choices, preferred baud,
  frequency/mode ranges, control read/write direction, maxima and discrete
  legal values, meter selectors/ranges/widths/polling/presentation, and native
  scope/waterfall metadata when implemented. Document explicit negative
  capability for manual surfaces that are not implemented.
- Never advertise an accessory waterfall such as Elecraft P3/PX3 as a
  transceiver scope. Native `ScopeMetadata` requires model-owned commands,
  stream framing, configuration handling, and tests.
- Run `cargo fmt --all`, `cargo test --locked`, `cargo clippy --locked
  --all-targets --all-features -- -D warnings`, and `git diff --check` before
  declaring a driver change complete.
- Coverage is part of the public documentation contract. Whenever coverage
  changes, run `cargo llvm-cov --locked --all-features --workspace
  --summary-only`, run `bash scripts/check-icom-coverage.sh` against that
  summary, and update every per-area coverage badge and the coverage snapshot
  in `README.md`. Never leave hard-coded badge values or documented coverage
  figures stale after adding tests, changing thresholds, or modifying code.
- Every changed production Rust file must have at least one covered executable
  line. New or changed model/profile files also require a focused contract test;
  aggregate area coverage must not be treated as a substitute for per-file
  coverage. CI enforces this with `scripts/check-changed-coverage.sh`.
- The README's release/version badge must match `Cargo.toml` and the current
  release branch or tag. When a release tag is created, verify the dynamic
  latest-release badge resolves to that tag and update the changelog and
  support documentation as needed.

Use the vendor model modules for model tables: Icom modules under
`src/icom/`, modern and classic Yaesu modules under `src/yaesu/`, and Kenwood
modules under `src/kenwood/`. Keep the shared vendor driver model-neutral.
