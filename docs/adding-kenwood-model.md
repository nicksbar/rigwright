# Adding a Kenwood model

Kenwood PC control is an ASCII, semicolon-terminated command family, but the
commands are not identical across generations. Add model differences to a
profile; do not branch on model names in the transport or copy common commands
into a model module.

## Required evidence

Use the official instruction manual and PC-control command reference for the
exact model. Record the editions in `supported-radios.md`. Verify at least:

- `ID;` response and whether it uniquely identifies the selected model;
- serial baud rates, stop-bit behavior, parity, and flow-control requirements;
- `FA`/`FB` frequency width and tunable ranges;
- mode command and complete mode table (`MD`, `OM`, or another family);
- whether a separate data-mode flag such as `DA` exists;
- PTT set commands and whether current RX/TX state can be queried;
- split representation (`FR`/`FT`, `TB`, or model-specific commands);
- `PC` power units and band/mode-dependent limits; and
- `SM` query parameters, response width, and maximum value.

Do not infer a command from another Kenwood model merely because the framing
looks the same. For example, TS-590SG uses `MD`, while TS-890S uses `OM` and
dedicated `TB` split control.

## Implementation checklist

1. Add a `KenwoodCatModel` variant and model-name aliases.
2. Add the catalog entry in `models.rs` at `Framework` maturity.
3. Define one `KenwoodCatProfile` in `kenwood/profile.rs`.
4. Re-export that constant as `CAT_PROFILE` from the individual model module.
5. Add profile tests for identification, range, baud, mode, split, meter, and
   status differences.
6. Add manual-derived command examples or captured frames to parser tests.
7. Update the support matrix and run the read-only `kenwood_probe` first.
8. Promote the model beyond `Framework` only after physical-radio validation.

The tuning ranges are guardrails, not transmit authorization. Region, license,
band plan, antenna, power, and mode checks remain application/operator policy.
Power commands deserve special care: AM and some VHF/UHF bands can have lower
maximums than the broad profile range.

At 4800 baud, all three current profiles use two stop bits. Higher documented
rates use one stop bit. TS-890S permits 4800 only on its physical COM port, not
the USB virtual COM port.

Kenwood `TX` is not a read command. TS-590SG and TS-2000 expose RX/TX state in
`IF`, so Rigwright verifies PTT changes there. TS-890S has no profiled polling
command for current PTT state; its `can_get_ptt` capability must remain false
until an official, pollable command is implemented.
