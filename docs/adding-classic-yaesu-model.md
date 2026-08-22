# Adding a classic Yaesu CAT model

This checklist applies only to Yaesu's fixed five-byte binary CAT family. Do
not mix these radios with the semicolon-terminated modern ASCII driver.

## 1. Confirm the wire protocol

Use the exact model operating manual and record its filename in
`supported-radios.md`. Confirm all of the following before sharing a profile:

- every command is four parameter bytes followed by one opcode byte;
- the serial format is 8 data bits, no parity, and two stop bits;
- supported CAT baud rates and the required radio menu/jack setting;
- response length for each read opcode;
- frequency encoding precision and documented tuning ranges;
- writable mode codes separately from codes that may only appear in status;
- RX/TX status-bit polarity, especially PTT and split, where zero means on.

Classic CAT has no model-identification command. The selected model therefore
comes from operator configuration and cannot be verified over the wire.

## 2. Add the profile surfaces

1. Add a `YaesuLegacyModel` variant and model-name aliases.
2. Add the `POPULAR_RADIOS` catalog entry with `Framework` maturity.
3. Add a `YaesuLegacyProfile` containing frequency ranges, baud rates,
   writable modes, and documented optional command groups.
4. Route it from `profile_for_model` and export it from the model module.
5. Add catalog/profile, boundary, mode, and factory tests.

Keep framing and shared status parsing in `protocol/yaesu_legacy_cat.rs`, serial
execution and HAL translation in `yaesu/legacy_radio.rs`, and genuinely unique
commands in the individual model module.

## 3. Validate conservatively

Manual examples are not hardware captures. A new profile remains `Framework`
until frequency and mode read/write, PTT read/write, RX/TX status, split,
timeouts, reconnects, and emergency de-key have been exercised on that exact
radio. Confirm that set commands really return no bytes and that status reads
return exactly the documented one or five bytes.

Do not expose remote radio power-off through the root HAL. The FT-817ND and
FT-818 manuals document power commands, but an accidental or unacknowledged
power transition is operationally different from ordinary CAT control.
