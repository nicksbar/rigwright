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
| Kenwood | TS-590SG, TS-890S, TS-2000 | Framework | [Kenwood command-reference downloads](https://www.kenwood.com/i/products/info/amateur/software_download.html), [TS-2000 manual](https://www.kenwood.com/usa/Support/pdf/TS-2000-Owner-Manual.PDF) |

The shared Yaesu and Kenwood codec only guarantees safe framing. Each model
still needs captured-response fixtures, mode mapping, capability gating, serial
integration, and physical-radio validation before promotion to supported.
