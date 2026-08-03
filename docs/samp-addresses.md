# SA-MP layout references

Wardrobe identifies `samp.dll` by PE entry point, which remains valid if Windows
relocates the module. Player-pool layouts still differ by client build.

| Client build | PE entry point | `CNetGame*` global | `m_pPools` | Player pool |
| --- | ---: | ---: | ---: | ---: |
| 0.3.7-R1 | `0x31DF13` | `0x21A0F8` | `0x3CD` | `0x18` |
| 0.3.7-R3-1 | `0x0CC4D0` | `0x26E8DC` | `0x3DE` | `0x08` |
| 0.3.7-R4 | `0x0CBCB0` | `0x26EA0C` | `0x3DE` | `0x08` |
| 0.3.DL-R1 | `0x0FDB60` | `0x2ACA24` | `0x3DE` | `0x08` |

R4 shares R3's player-pool layout. R1, R3/R4, and DL use distinct remote-player
and local-ped offsets; unknown revisions are rejected rather than guessed.

Sources: [SAMP-API multiversion headers](https://github.com/blasthacknet/samp-api/tree/multiver)
and [RakHook](https://github.com/imring/RakHook).
