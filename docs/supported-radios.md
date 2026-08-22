# Radio support and manual sources

Support labels are intentionally conservative. **Hardware validated** means the
driver is exercised against a physical radio. **Framework** means a model
profile and protocol primitives exist, but Rigwright does not yet claim a
working end-to-end driver for that radio.

| Vendor | Models | Status | Official command documentation |
|---|---|---|---|
| Icom | IC-7300 | Hardware validated | [Icom IC-7300 support](https://www.icomjapan.com/lineup/products/IC-7300/) |
| Icom | IC-705, IC-7610, IC-9700 | Framework | [IC-705](https://www.icomjapan.com/lineup/products/IC-705/), [IC-7610 CI-V guide](https://www.icomjapan.com/support/manual/1745/), [IC-9700](https://www.icomjapan.com/lineup/products/143/) |
| Yaesu | FT-710, FTDX10, FTDX101D, FTDX101MP, FT-991A | Framework | [FT-710 CAT manual](https://yaesu.com/product-detail.aspx?CatName=HF+Transceivers%2FAmplifiers&Model=FT-710), [FTDX10 downloads](https://www.yaesu.com/indexVS.cfm?cmd=DisplayProducts&ProdCatID=102&encProdID=1ABBC23C7EC57175A35CB0FDE7A639A0), [FTDX101MP/D CAT manual](https://www.yaesu.com/product-detail.aspx?CatName=HF+Transceivers%2FAmplifiers&Model=FTDX101D), [FT-991A CAT manual](https://www.yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FT-991A_CAT_OM_ENG_1711-D.pdf) |
| Yaesu legacy binary CAT | FT-817ND, FT-818, FT-857D, FT-897D | Framework | [FT-817ND manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-817ND), [FT-818 manual](https://public2024.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-818), [FT-857D manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-857D), [FT-897D manual](https://www.yaesu.com/product-detail.aspx?CatName=Legacy&Model=FT-897D) |
| Kenwood | TS-590SG, TS-890S, TS-2000 | Framework | [Kenwood command-reference downloads](https://www.kenwood.com/i/products/info/amateur/software_download.html), [TS-2000 manual](https://www.kenwood.com/usa/Support/pdf/TS-2000-Owner-Manual.PDF) |

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

The profile frequency ranges are conservative tune guards derived from the
documented command/scope ranges. They are not a promise that every frequency is
transmittable, nor a substitute for region, band-plan, license, or radio
configuration checks.

## Modern Yaesu manual audit

Modern Yaesu profiles were checked against the official CAT manuals in the
workspace. All remain framework-level until exercised against physical radios.

| Model | Manual edition used | Profile details checked |
|---|---|---|
| FT-710 | `FT-710_CAT_OM_ENG_2306-C.pdf`, Jun. 2023 | ID `0800`; `FA` 9-digit 0.03-75 MHz range; receiver-qualified `MD`; `TX`; `PC` 5-100 W; `ST`; CAT-1/CAT-2 rates through 115200 |
| FTDX10 | `FTDX10_CAT_OM_ENG_2308-F.pdf`, Aug. 2023 | ID `0761`; `FA` 9-digit 0.03-75 MHz range; `MD0`; `TX0/1/2`; `PC` 5-100 W; `ST0/1/2`; 4800-38400 baud |
| FTDX101D | `FTDX101MP_D_CAT_OM_ENG_2308-L.pdf`, Aug. 2023 | ID `0681`; `FA` 9-digit 0.03-75 MHz range; `MD`; `TX`; `PC` 5-100 W; `ST`; 4800-38400 baud |
| FTDX101MP | `FTDX101MP_D_CAT_OM_ENG_2308-L.pdf`, Aug. 2023 | ID `0682`; common modern commands; distinct `PC` 5-200 W maximum |
| FT-991A | `FT-991A_CAT_OM_ENG_1711-D.pdf`, Nov. 2017 | ID `0670`; `FA` 9-digit 0.03-470 MHz range; model-specific `MD` table including C4FM; `TX`; `PC` 5-100 W; no `ST` profile; 4800-38400 baud |

The shared modern driver implements persistent serial transport, response
matching in the presence of auto-information frames, frequency, mode, readable
PTT, raw queries, RF power, and profile-gated split. The profile mode table
chooses DATA-U for the protocol-neutral `Mode::Data`; other DATA variants still
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
| FT-857D | `FT-857D_OM_ENG_EH007M108.pdf` | CAT/LINEAR jack; 8N2; segmented receive ranges; FM-N write code; WFM/CW-N status codes; RX/TX bit layouts |
| FT-897D | `FT-897_OpMan.pdf` | Available FT-897 family manual; CAT/LINEAR menu; segmented receive ranges; FM-N and WFM codes; RX/TX bit layouts |

These remain framework-level. In particular, the available FT-897 manual is a
family reference rather than proof of FT-897D hardware behavior, and classic
CAT has no model ID query with which to detect an incorrect operator selection.
