# Radio capability matrix

This is the canonical detailed support inventory for the radios in
[`supported-radios.md`](supported-radios.md). It separates documentation,
implementation, profile, application, and validation claims:

| Code | Meaning |
|---|---|
| **M** | Documented by the radio manual or vendor command reference. |
| **H** | Exposed as a typed Rigwright HAL operation or value. |
| **P** | Implemented and gated by the selected model profile. |
| **Q** | Currently consumed by native QSONaut UI/workflows. |
| **V** | Hardware-validated in the project. At present this is the IC-7300 baseline. |
| **—** | Not available, not applicable, or intentionally not claimed. |
| **R** | Read-only telemetry. **W** means writable control. **RW** means both. |

`M` alone is not a support claim. Vendor manuals describe many functions that
are not yet safe to expose generically because command selectors, payloads,
units, or model behavior differ.

## Root HAL operations

| Operation | HAL surface | Icom CI-V | Modern Yaesu CAT | Classic Yaesu CAT | Kenwood PC control | QSONaut native use |
|---|---|---:|---:|---:|---:|---|
| Frequency read/write | `get/set_frequency_hz` | H/P/V for IC-7300 | H/P | H/P | H/P | Q |
| Operating mode read/write | `get/set_mode` | H/P/V for IC-7300 | H/P | H/P | H/P | Q |
| PTT write | `set_ptt` | H/P/V | H/P | H/P | H/P | Q |
| PTT read | `get_ptt` | H/P/V | H/P | H/P | P for TS-590SG/TS-2000; unavailable for TS-890S | Q |
| Radio power write | `set_power` | H/P/V behavior validated on IC-7300 | H/P | — | H/P | Q |
| Radio power read | `get_power` | —; CI-V power is write-only | H/P | — | H/P | Q/pending-state handling |
| Raw protocol | `protocol_write_read` | H/P | H/P | H/P | H/P | Not a normal UI control |
| Tuner start/status | `start_tuner`, `get_tuner_status` | H/P/V for profiled Icoms | — | — | — | Q: tuner and SWR sweep workflow |
| Spectrum waveform | backend-specific scope API | H/P; model geometry differs | — | — | — | Q where native scope is enabled |
| I/Q stream | model/backend-specific | Profile metadata for IC-7610; not a root HAL stream | — | — | — | Not currently consumed |

### Universal HAL caveat

The method names are universal; the hardware support is not. Applications must
check `Radio::capabilities()`, `supports_control()`, and `supports_meter()` as
appropriate. A generic vendor driver deliberately reports no optional typed
controls or meters until a concrete model profile is selected.

## Typed controls

The following table lists every current `ControlId`. “Icom” means the selected
Icom profile exposes the operation; “Yaesu” and “Kenwood” are the current
profile-wide implementation claims. A model-specific exception is listed in
the final column.

| HAL control | Value | Icom CI-V | Modern Yaesu | Classic Yaesu | Kenwood | QSONaut native use |
|---|---|---:|---:|---:|---:|---|
| `AfGain` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q slider |
| `RfGain` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q slider |
| `Squelch` | normalized `U8` 0–255 | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q slider |
| `RfPower` | normalized `U8` 0–255 | RW/P all four profiles | RW/P modern profiles; exact watts also available | M, not typed; power write intentionally absent | RW/P all three profiles; exact watts also available | Q slider and SWR sweep power |
| `Preamp` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q compact control |
| `ExternalPreamp` | model-specific `U8` | RW/P IC-9700 only | M, not typed | M, not typed | M, not typed | Not currently used |
| `Attenuator` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q compact control |
| `NoiseBlanker` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q toggle |
| `NoiseReduction` | `Bool` | RW/P all four profiles | RW/P modern profiles | M, not typed | M, not typed | Q toggle |
| `NoiseReductionLevel` | `U8` 1–15 | M, not typed | RW/P modern profiles | M, not typed | M, not typed | Q level control where advertised |
| `IpPlus` | `Bool` | RW/P all four profiles | M, not typed | — | M, not typed | Q toggle |
| `Notch` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q toggle |
| `ManualNotch` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q toggle; position not typed |
| `DataMode` | `Bool` | RW/P all four profiles | M, not typed | M, not typed | TS-590SG/TS-890S model behavior exists but not typed as this control | Q mode/status support |
| `Filter` | model-specific `U8` | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q filter control |
| `Agc` | model-specific `U8` | RW/P IC-705, IC-7300, and IC-9700; manual-only on IC-7610 | RW/P modern profiles | M, not typed | M, not typed | Q compact control |
| `Rit` | model-specific value | M | M | M | M | Not implemented |
| `Xit` | model-specific value | M | M | M | M | Not implemented |
| `Split` | `Bool` | RW/P all four profiles | RW/P profiles with documented split | RW/P all four profiles | RW/P all three profiles | Q profile/control path, limited banner use |
| `Tuner` | `Bool` enable/status | RW/P all four profiles | M, not typed | M, not typed | M, not typed | Q enable/disable and sweep safety |
| `Vfo` | model-specific selector | H/P Icom profiles | M, not typed | M, not typed | M, not typed | Not currently used as a typed banner control |
| `MainSub` | receiver selector | RW/P IC-7610 and IC-9700 | M, not typed | — | M, not typed | Not currently used |
| `RawCiV` | raw bytes | H/P Icom only | — | — | — | Not a normal UI control |

### Control interpretation notes

- Icom controls are declarative CI-V profile entries. The exact command
  prefixes and allowed values remain model-specific even when the HAL name is
  shared.
- Modern Yaesu typed controls currently cover RF power, split, AGC, NR enable,
  and NR level. The CAT manuals contain more functions, but they are not yet
  in the root HAL for Yaesu.
- Classic Yaesu has a separate five-byte binary protocol. It supports split,
  frequency, mode, PTT, and status operations in Rigwright; it does not inherit
  modern Yaesu CAT controls.
- Kenwood has verified RF power, split, frequency, mode, PTT where pollable,
  signal meter, and SWR meter. Its manual surfaces are broader than the
  current typed profile.

### Exact typed-control profile sets

These lists are the concrete profile result behind the grouped table above.
They include the shared Icom special operations `DataMode`, `Filter`, `Vfo`,
and `RawCiV` where the selected profile permits them.

| Model/profile | Typed controls currently advertised by Rigwright |
|---|---|
| IC-705 | AF gain, RF gain, squelch, RF power, preamp, attenuator, AGC, NB, NR, IP+, auto notch, manual notch, tuner, split, data mode, filter, VFO, raw CI-V |
| IC-7300 | Same as IC-705; this is the hardware-validated Icom profile |
| IC-7610 | AF gain, RF gain, squelch, RF power, preamp, attenuator, NB, NR, IP+, auto notch, manual notch, tuner, split, data mode, filter, VFO, main/sub, raw CI-V; AGC is manual-only and not typed in this profile |
| IC-9700 | IC-705 set plus external preamp and main/sub |
| FT-710, FTDX10, FTDX101D, FTDX101MP | RF power, split, AGC, NR enable, NR level |
| FT-991A | RF power, AGC, NR enable, NR level; split is manual-only in the current profile |
| FT-817ND, FT-818, FT-857D, FT-897D | Split only; other documented CAT surfaces remain untyped |
| TS-590SG, TS-890S, TS-2000 | RF power and split |

## Normalized meters

All typed meter values are normalized to a HAL deflection level of 0–255. This
is not a universal physical-unit conversion. For example, Icom SWR values have
documented ratio anchors, but those anchors are not shared by Yaesu or Kenwood.

| HAL meter | Icom CI-V | Modern Yaesu CAT | Classic Yaesu CAT | Kenwood | QSONaut native use |
|---|---:|---:|---:|---:|---|
| `Signal` | R/P via `15 01`; IC-7300 V | R/P via `RM1` | M, not typed | R/P via `SM`, profile max 30 or 70 | Q normalized meter panel where advertised |
| `Power` | R/P via `15 02`; IC-7300 V | R/P via `RM5` | M, not typed | M, not typed | Q normalized meter panel where advertised |
| `Swr` | R/P via `15 12`; IC-7300 V | R/P via `RM6` | M, not typed | R/P via `RM`; selector and range profile-specific | Q live meter and stepped SWR chart |
| `Alc` | R/P via `15 11`; IC-7300 V | R/P via `RM4` | M, not typed | M, not typed | Q normalized meter panel where advertised |
| `Compression` | R/P via `15 13`; IC-7300 V | R/P via `RM3` | M, not typed | M, not typed | Q normalized meter panel where advertised |
| `Current` | R/P via `15 15`; IC-7300 V | R/P via `RM7` | M, not typed | M, not typed | Q normalized meter panel where advertised |
| `Voltage` | R/P via `15 14`; IC-7300 V | R/P via `RM8` | M, not typed | M, not typed | Q normalized meter panel where advertised |
| `Temperature` | R/P via `15 16`; IC-7300 V | Manual/protocol surface varies; intentionally not profiled | M, not typed | TS-890S manual surface exists; not typed | Q only if a future profile advertises it |

Yaesu `RM` selector meanings are documented by the modern CAT manuals: `1`
signal, `3` compression, `4` ALC, `5` power, `6` SWR, `7` current, and `8`
voltage. Kenwood RM selector meanings differ by model; TS-590SG and TS-2000
use one SWR selector while TS-890S uses another, so the profile owns the
selector and native meter maximum.

## Manual surfaces not yet typed into the HAL

These are intentionally tracked so they do not disappear from the roadmap.
They are documented radio capabilities, but currently have no safe common
`ControlId`/`MeterId` implementation across the supported model set.

| Surface | Manual coverage | Current Rigwright status | QSONaut status | Why it remains open |
|---|---|---|---|---|
| RIT/XIT offset and enable | All modern vendor families | HAL IDs exist, no driver implementation | Not used | Signed ranges, enable state, and command widths differ |
| Yaesu AF/RF gain and squelch | Modern CAT manuals | Not typed | UI cannot safely use them through Yaesu | Query/set field formats need profile commands and ranges |
| Yaesu NB, filter, tuner, antenna selection | Modern CAT manuals | Not typed | Not used for modern Yaesu | Several commands are mode/model dependent |
| Yaesu ALC/compression/current/voltage presentation | Modern CAT manuals | Meter IDs implemented | Not displayed | QSONaut must add polling, labels, and TX safety policy |
| Icom signal, output power, ALC, compression, voltage/current | CI-V/model manuals | Meter IDs exist but Icom mappings are not typed | Not displayed | CI-V subcommands and scaling must be verified per model |
| Icom NR level and notch position | Model manuals | Enable controls exist; level/position not typed | Not used | Value encoding and model ranges differ |
| Kenwood AF/RF gain, squelch, AGC, NB, NR | PC command references | Not typed | Not used | TS-590SG, TS-890S, and TS-2000 command families differ |
| Kenwood ALC/compression/current/voltage/temperature | PC command references | Not typed except signal/SWR | Not displayed | RM selector, range, and response width differ by model |
| Memory/channel operations | Vendor manuals | Not typed | Not used | Requires a larger stateful API than a scalar control |
| Antenna selection | Vendor manuals | Not typed | Not used | Main/sub/VFO and tuner routing differ by model |
| Band-stack and quick-memory operations | Vendor manuals | Not typed | Not used | Requires model-specific state and persistence semantics |
| Scope configuration and I/Q streaming | Icom manuals, some Yaesu manuals | Icom scope profile exists; I/Q remains metadata-only | Scope use is partial; I/Q not consumed | Waveform transport and receiver geometry are model-specific |
| Auto-information subscriptions | Vendor command references | Kenwood transport tolerates frames; no common subscription API | Not used as a general event source | Needs lifecycle, filtering, and backpressure semantics |

## QSONaut consumption summary

QSONaut currently consumes the following Rigwright surfaces in its native
radio worker/UI:

- frequency, mode, data-mode status, filter, PTT, and radio power workflow;
- AF gain, RF gain, squelch, RF power, preamp, attenuator, NB, NR, IP+, notch,
  AGC, and tuner controls where the selected profile advertises them;
- read-only tuner status;
- normalized SWR polling and the stepped SWR sweep workflow;
- native Icom scope data where the selected model/profile provides it.

QSONaut consumes every normalized meter that the selected Rigwright profile
advertises and renders those values in the native radio banner. Physical units
and calibrated SWR ratios remain limited: the HAL provides normalized meter
deflection, while QSONaut applies the documented IC-7300 SWR anchors only for
that model and otherwise displays the normalized level.

QSONaut also consumes modern Yaesu’s typed noise-reduction level control. It
does not yet consume RIT/XIT, Icom external-preamp or main/sub controls,
memory/channel operations, antenna selection, or generic auto-information
surfaces. Icom signal/power/ALC/compression/current/voltage remain manual-only
in Rigwright, so they cannot appear in QSONaut through the capability gate.

## Maintenance rule

When adding a capability, update all four locations when applicable:

1. the root `ControlId`/`MeterId` or backend-specific API;
2. the vendor/model profile and `supports_*` capability report;
3. this matrix, including manual filename and validation level;
4. QSONaut only if the UI/workflow actually consumes the capability.

Do not mark a manual-only feature as Rigwright-supported until its command,
response shape, scaling, model scope, and failure behavior have tests.
