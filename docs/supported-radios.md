# Radio support and manual sources

For the complete control, meter, manual-surface, and QSONaut-consumption
inventory, see [`radio-capability-matrix.md`](radio-capability-matrix.md).

Support labels are intentionally conservative. **Hardware validated** means the
driver is exercised against a physical radio. **Framework** means a model
profile and protocol primitives exist, but Rigwright does not yet claim a
working end-to-end driver for that radio.

| Vendor | Models | Status | Official command documentation |
|---|---|---|---|
| Icom | IC-7300 | Hardware validated | [Icom IC-7300 support](https://www.icomjapan.com/lineup/products/IC-7300/) |
| Icom | IC-705, IC-7610, IC-9700 | Framework | [IC-705](https://www.icomjapan.com/lineup/products/IC-705/), [IC-7610 CI-V guide](https://www.icomjapan.com/support/manual/1745/), [IC-9700](https://www.icomjapan.com/lineup/products/143/) |
| Yaesu | FTDX10 | Hardware validated | [FTDX10 downloads](https://www.yaesu.com/indexVS.cfm?cmd=DisplayProducts&ProdCatID=102&encProdID=1ABBC23C7EC57175A35CB0FDE7A639A0) |
| Yaesu | FT-710, FTDX101D, FTDX101MP, FT-991A | Framework | [FT-710 CAT manual](https://yaesu.com/product-detail.aspx?CatName=HF+Transceivers%2FAmplifiers&Model=FT-710), [FTDX101MP/D CAT manual](https://www.yaesu.com/product-detail.aspx?CatName=HF+Transceivers%2FAmplifiers&Model=FTDX101D), [FT-991A CAT manual](https://www.yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FT-991A_CAT_OM_ENG_1711-D.pdf) |
| Yaesu legacy binary CAT | FT-817ND, FT-818, FT-857D, FT-897D | Framework | [FT-817ND manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-817ND), [FT-818 manual](https://public2024.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-818), [FT-857D manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-857D), [FT-897D manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-897D) |
| Kenwood | TS-590SG, TS-890S, TS-2000 | Framework | [Kenwood command-reference downloads](https://www.kenwood.com/i/products/info/amateur/software_download.html), [TS-2000 manual](https://www.kenwood.com/usa/Support/pdf/TS-2000-Owner-Manual.PDF) |

## Elecraft component audit

Elecraft is intentionally not listed as a supported radio until the
transceiver backend and station-component boundary are implemented. The local
references currently available under `_manuals/elecraft` are:

| Surface | Reference | Initial boundary |
|---|---|---|
| K2 | `KIO2 Pgmrs Ref rev E.pdf` | Transceiver profile |
| KX2/KX3/K3/K3S | `K3S&K3&KX3&KX2 Pgmrs Ref, G5.pdf` | Shared transceiver family with profile differences |
| K4 | `K4 Programmer's Reference, rev. D5.pdf`; `K4 Command Index Rev3.pdf` | Transceiver plus optional Ethernet/streaming capabilities |
| KH1 | `Elecraft KH1 Programmer's Ref, rev B2.pdf` | Separate limited transceiver profile |
| P3/PX3 | `P3_Pgmrs_Ref_Rev_A7.pdf`; `PX3_Pgmrs_Ref_A6.pdf` | Separate spectrum components |
| KAT500 | `KAT500 Automatic Antenna Tuner Serial Command Reference.pdf` | Separate tuner component |
| KPA500/KXPA100 | `KPA500 Programmers Ref.pdf`; `KXPA100 Amplifier Command Reference.pdf` | Separate amplifier components |

These documents establish command syntax and documented capabilities only.
They do not constitute physical-radio or accessory validation. The proposed
component boundary and implementation order are recorded in
[`adding-elecraft.md`](adding-elecraft.md).

## Icom manual audit

The Icom profiles were checked against the official manuals available in the
development workspace. These filenames and command sections make later audits
reproducible without treating a product-page compatibility claim as evidence:

| Model | Manual edition used | Profile details checked |
|---|---|---|
| IC-705 | `IC-705_ENG_CI-V_6.pdf`, Jan. 2023 | address `A4`; commands `0F`, `11`, `14`, `16`, `26`, `27`; WFM; 11/475 scope; 0.03–200 and 400–470 MHz scope ranges |
| IC-7300 | `IC-7300_Full_English v6.pdf` / `.md` | address `94`; commands `0F`, `11`, `14`, `16`, `26`, `27`; 20 dB attenuator only; FM; 11/475 scope |
| IC-7610 | `IC-7610_ENG_CI-V_4.pdf`, Sep. 2025 | address `98`; commands `07 D0/D1/D2`, `0F`, `11`, `14`, `16`, `26`, `27`; main/sub; 15/689 scope; 0.03–60 MHz scope range |
| IC-9700 | `IC-9700_ENG_CI-V_4.pdf`, Mar. 2023 | address `A2`; commands `07 D0/D1/D2`, `0F`, `11`, `16 02`, `26`, `27`; combined internal/external preamp; 144/430/1240 MHz bands; 11/475 scope |

The shared Icom profile also exposes IP+ (`1A 07`), auto notch (`16 41`), and
manual notch enable (`16 48`) as typed controls. SWR is read-only telemetry via
`MeterId::Swr`, using `15 12`; the driver exposes the result on the HAL's
normalized 0..255 meter-deflection scale. The IC-7300 manual documents raw
values of 0 = 1.0:1, 48 = 1.5:1, 80 = 2.0:1, and 120 = 3.0:1, but those ratio
anchors are Icom-specific and are not applied globally by the HAL.

The Icom tuner surface is separate from the meter: `ControlId::Tuner` uses the
documented tuner enable/status operation (`1C 01`), `start_tuner()` requests
tuning (`1C 01 02`), and `get_tuner_status()` reports disabled, enabled, or
tuning. Tuning can transmit, so applications must require an explicit operator
action and should not start it from background SWR polling.

The profile frequency ranges are conservative tune guards derived from the
documented command/scope ranges. They are not a promise that every frequency is
transmittable, nor a substitute for region, band-plan, license, or radio
configuration checks.

## Modern Yaesu manual audit

Modern Yaesu profiles were checked against the official CAT manuals in the
workspace. FTDX10 has also been exercised on physical hardware through its
Enhanced CAT port; the other modern Yaesu profiles remain framework-level.

| Model | Manual edition used | Profile details checked |
|---|---|---|
| FT-710 | `FT-710_CAT_OM_ENG_2306-C.pdf`, Jun. 2023 | ID `0800`; `FA` 9-digit 0.03-75 MHz range; receiver-qualified `MD`; `TX`; `PC` 5-100 W; `ST`; CAT-1/CAT-2 rates through 115200 |
| FTDX10 | `FTDX10_CAT_OM_ENG_2308-F.pdf`, Aug. 2023 | ID `0761`; `FA` 9-digit 0.03-75 MHz range; `MD0`; `TX0/1/2`; `PC` 5-100 W; `ST0/1/2`; 4800-38400 baud |
| FTDX101D | `FTDX101MP_D_CAT_OM_ENG_2308-L.pdf`, Aug. 2023 | ID `0681`; `FA` 9-digit 0.03-75 MHz range; `MD`; `TX`; `PC` 5-100 W; `ST`; 4800-38400 baud |
| FTDX101MP | `FTDX101MP_D_CAT_OM_ENG_2308-L.pdf`, Aug. 2023 | ID `0682`; common modern commands; distinct `PC` 5-200 W maximum |
| FT-991A | `FT-991A_CAT_OM_ENG_1711-D.pdf`, Nov. 2017 | ID `0670`; `FA` 9-digit 0.03-470 MHz range; model-specific `MD` table including C4FM; `TX`; `PC` 5-100 W; no `ST` profile; 4800-38400 baud |

The shared modern driver implements persistent serial transport, response
matching in the presence of auto-information frames, frequency, mode, readable
PTT, raw queries, RF power, receiver controls, clarifiers, VFO selection,
tuner control, memory records, repeater settings, and profile-gated event
subscriptions. The profile mode table chooses DATA-U for the protocol-neutral
`Mode::Data`; other DATA variants still
decode as data because the root HAL intentionally has a coarser mode type.

## Classic Yaesu manual audit

Classic Yaesu models use a separate five-byte binary protocol. The profiled
driver implements persistent 8N2 serial transport, documented frequency ranges
and writable modes, frequency/mode status, readable and writable PTT, split,
RX/TX meters and flags, raw commands, PTT state verification, and reconnect
behavior.

| Model | Manual used | Profile details checked |
|---|---|---|
| FT-817ND | `FT-817ND_OM_ENG_E13771011.pdf` | 17-opcode table; 8N2; 4800/9600/38400 baud; model receive ranges; mode/status codes; `E7`, `F7`, `03`; active-low PTT/split; power commands intentionally not exposed |
| FT-818 | `FT-818ND_OM_ENG_E13772004_2003u-ES-1.pdf` | Same five-byte family; distinct 0.1-56 MHz low receive range; status layouts; baud rates; power commands intentionally not exposed |
| FT-857D | `FT-857D_OM_ENG_EH007M108.pdf` | CAT/LINEAR jack; 8N2; segmented receive ranges; FM-N write code; WFM/CW-N status codes; RX/TX bit layouts; clarifier and repeater opcodes |
| FT-897D | `FT-897_OpMan.pdf` | Available FT-897 family manual; CAT/LINEAR menu; segmented receive ranges; FM-N and WFM codes; RX/TX bit layouts; shared classic opcode family |

Classic CAT has no model ID query with which to detect an incorrect operator
selection. The driver also exposes the documented VFO toggle and CAT-lock
commands as explicit classic-driver helpers. The protocol has no memory read/write or unsolicited event opcodes;
AF/RF gain, squelch, IPO/ATT, DSP filters, and local memory channels therefore
remain untyped. Repeater and clarifier writes have no documented readback and
are intentionally advertised as write-only at the control level.

## Kenwood manual audit

Kenwood profiles share semicolon framing and persistent serial transport, not
one assumed command set. Official instruction manuals in the workspace were
cross-referenced with Kenwood's separate official command references for the
newer radios.

| Model | Manual/reference used | Profile details checked |
|---|---|---|
| TS-590SG | `B5A-0180-20.pdf`; `ts590_g_pc_command_en_rev3.pdf`, Jan. 2019 | ID `023`; `FA`/`FB` 11-digit frequency; `FR`/`FT`; `MD` plus `DA`; `IF` RX/TX/RIT/XIT fields; `PC` 5-100 W broad range (AM max 25 W); `SM0` 0-30; `RM` SWR/COMP/ALC; `MC`/`MR`/`MW` memory records; 4800-115200 baud |
| TS-890S | `B5A-4695-00.pdf`; `ts890_pc_command_en_rev1.pdf`, Jan. 2019 | ID `024`; `FA`/`FB`; `OM` with PSK and data variants; direct `TB` split; `PC`; `SM` 0-70; `RM` SWR/ALC/COMP/ID/VD/TEMP; `RF`/`RT`/`XT` RIT/XIT; no pollable PTT query; COM/USB baud differences |
| TS-2000 | `B62-1221-70.pdf`, PC Control Command Tables | ID `019`; `FA`/`FB`; `FR`/`FT`; `MD`; `IF` RX/TX field; HF/VHF/UHF/1.2 GHz receive segments; 4800 baud requires two stop bits |

The shared driver verifies `ID`, follows the selected receiver VFO for
frequency reads/writes, exposes exact watts and normalized HAL power, handles
model-specific modes and split commands, reads the documented meter layout,
implements profiled receiver controls, RIT/XIT, VFO selection, tuner, filters,
and memory records, and routes interleaved Auto Information frames to the
shared event router. PTT writes are verified on the two models with pollable
`IF` status. All three remain framework-level until exercised against physical
radios.
All normalized meters use the HAL's 0..255 meter-deflection scale. Yaesu CAT
profiles expose signal, power, SWR, ALC, compression, current, and voltage
through the documented `RM1` and `RM3`..`RM8` selectors; FTDX101D/MP also
expose temperature through `RM9`. Other profiled modern Yaesu models do not
advertise temperature. Profiles also expose typed AGC,
noise-reduction, and noise-reduction-level controls. Kenwood profiles expose
normalized signal, TX power, and profile-correct SWR; TS-590SG additionally
exposes ALC and compression, while TS-890S additionally exposes ALC,
compression, current, voltage, and temperature. SWR telemetry uses
the HAL's normalized 0..255 meter-deflection scale. The
Kenwood profiles query the documented `RM` SWR meter and normalize their
model-specific 0..30 or 0..70 dot ranges. Modern Yaesu CAT profiles query the
documented `RM` selectors and already return 0..255 values. This is normalized meter
deflection, not a universal physical SWR-ratio conversion; the manuals do not
define enough cross-vendor ratio calibration to infer one safely.

The model-aware `ConfiguredRadio` wrapper delegates optional controls, meters,
clarifiers, tuner, memory, repeater, and event-router behavior to the selected
vendor backend. It does not own vendor command semantics. Generic vendor
drivers intentionally report no profile-only typed meters or controls until a
concrete model profile is selected.
