# Adding a modern Yaesu model

Use this checklist for semicolon-terminated ASCII CAT radios. Do not route an
older five-byte binary CAT radio through this driver merely because both carry
the Yaesu name.

## 1. Audit the official CAT manual

Record the exact filename and edition in `supported-radios.md`, then verify:

- the four-character `ID;` response;
- `FA` field width and documented tuning range;
- whether `MD` requires a receiver selector and every accepted mode code;
- `TX` set/read meanings, especially front-panel TX versus CAT-asserted TX;
- supported CAT baud rates and stop-bit configuration;
- the range and unit of `PC` (watts, not percent);
- whether `ST` exists, and the meaning of values beyond simple on/off;
- any receiver, tuner, or meter command the profile will claim.

Treat receive/tuning ranges as driver guardrails, not transmit authorization.
Region, license, band-plan, power, and mode policy remains an application
responsibility.

## 2. Add the declarative surfaces

1. Add a `YaesuCatModel` variant and its spelling aliases.
2. Add the catalog row in `POPULAR_RADIOS` with `Framework` maturity.
3. Add a `YaesuCatProfile` in `yaesu/profile.rs` with the manual-derived ID,
   ranges, baud rates, mode table, power range, and optional command groups.
4. Route it from `profile_for_model` and export the constant from the model
   module.
5. Add factory and profile consistency tests.

The transport in `yaesu/cat_radio.rs` must remain free of model-name branches.
If behavior varies, express it as profile data or a narrowly scoped optional
specification. Put commands unique to one radio in that radio's module and
guard execution by the selected profile. Do not duplicate `FA`, `MD`, `TX`,
`PC`, `ST`, framing, or raw-query helpers in a model module; those are owned by
the generic driver. Prebuilt model-specific set commands can be passed to
`YaesuCatRadio::send_raw`.

## 3. Test protocol facts without inventing captures

Add tests for official manual examples, exact field widths, mode round trips,
range boundaries, ID values, and rejected controls. Label hand-authored manual
examples as such; reserve “captured” for bytes recorded from physical hardware.

Test interleaved auto-information frames and partial serial reads. A query must
match the requested two-letter response rather than accepting the first frame
available on the port.

## 4. Keep maturity honest

New manual-derived profiles remain `Framework`. Promote one to
`HardwareValidated` only after exercising frequency read/write, every exposed
mode, PTT read/write and emergency de-key, identification, profile controls,
timeouts, malformed/error replies, reconnect behavior, and sustained polling
against that exact radio.
