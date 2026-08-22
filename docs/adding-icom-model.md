# Adding an Icom CI-V model

An Icom model is ready to merge only when its defaults and differences are in a
profile and the shared CI-V engine remains free of model-name conditionals.

## 1. Establish the evidence

Use the official CI-V reference for the exact firmware/manual edition. Record
the source in `supported-radios.md` and verify at least:

- factory CI-V address and supported baud/transport requirements;
- command `03/05` frequency format and documented tuning guard ranges;
- command `04/06` and, when present, `26` mode/data/filter layout;
- PTT command `1C 00`;
- command-only controls such as `0F` and `11` versus true subcommands;
- allowed values for attenuator, preamp, AGC, and level controls;
- scope enable/output commands plus selector, division, bin, and value layout;
- main/sub, satellite, I/Q, or combined-value behavior unique to the model.

Do not infer a command from a neighboring model. CI-V reuses command numbers,
but data layout, range, and even meaning can differ.

## 2. Add the profile surfaces

Update all of these together:

1. Add the model to `IcomCivModel` and its name conversion.
2. Add `src/icom/<model>.rs` with one `IcomCivProfile`.
3. Export the module and route it from `profile_for_model`.
4. Add the public catalog entry in `models.rs`.
5. Keep the catalog address and CI-V profile address identical; a test enforces
   this for built-in models.

Use `command_prefix: &[0x0F]` for a command-only value and
`command_prefix: &[0x14, 0x01]` for a command plus subcommand. Never add a
synthetic zero byte just to make command shapes look uniform.

If two logical controls share one combined radio value, reads and writes must
preserve the other bits. IC-9700 internal/external preamp handling is the
reference pattern.

## 3. Test wire behavior

Add tests for exact prefixes, allowed/rejected values, mode support, range
boundaries, ACK/NAK handling, and all scope geometry. Prefer captured
radio-to-controller frames. Synthetic frames are useful for malformed and edge
cases but do not count as hardware evidence.

For scope streams, cover division 1 metadata, an ordinary sample division, the
last short division, an entire ordered sweep, missing/out-of-order divisions,
and main/sub selectors when applicable.

Run:

```text
cargo fmt --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo doc --no-deps
```

## 4. State maturity honestly

New manual-derived profiles start at `Framework`. Promote one to
`HardwareValidated` only after exercising frequency read/write, every claimed
mode, PTT read/write, profile controls, error handling, and sustained scope
streaming on a physical radio. Record captures for regressions and note any
firmware or regional variant.
