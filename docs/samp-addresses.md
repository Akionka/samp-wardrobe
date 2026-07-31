# SA-MP address research

Wardrobe identifies a loaded `samp.dll` from the PE optional-header
`AddressOfEntryPoint`. Unlike an absolute code address, this value is relative
to the image and is unaffected when Windows relocates the DLL.

| Client build | PE entry point | `CNetGame*` global | `CNetGame::m_pPools` | Player pool in `Pools` |
| --- | ---: | ---: | ---: | ---: |
| 0.3.7-R1 | `0x31DF13` | `0x21A0F8` | `0x3CD` | `0x18` |
| 0.3.7-R3-1 | `0x0CC4D0` | `0x26E8DC` | `0x3DE` | `0x08` |
| 0.3.7-R4 | `0x0CBCB0` | `0x26EA0C` | `0x3DE` | `0x08` |
| 0.3.DL-R1 | `0x0FDB60` | `0x2ACA24` | `0x3DE` | `0x08` |

The player-pool layout changes too, so the scanner keeps a separate layout for
R1, R3/R4, and DL. In particular, the remote-player array is at `0x2E` in R1,
`0x04` in R3/R4, and `0x26` in DL. DL also moves the local player and the
remote ped pointer. Treating all builds as R1 can therefore read unrelated
memory and eventually pass an invalid ped to GTA.

R4 uses the R3 player-pool layout. Its `CNetGame` and pool arrangement match
R3; the distinct PE entry point and global address still identify it as R4.
The scanner performs all SA-MP reads through `ReadProcessMemory`, so a missing
pool or stale pointer aborts the scan instead of dereferencing it directly.

## Sources

- [SAMP-API multiversion headers](https://github.com/blasthacknet/samp-api/tree/multiver)
  document the R1, R3-1, and DL structure layouts and function addresses.
- [RakHook offsets and version detection](https://github.com/imring/RakHook)
  provide the entry-point fingerprints and `CNetGame` globals for R1, R3-1,
  R4, and DL-R1.

No addresses are guessed for other revisions. In particular, 0.3.7-R5 is
rejected until its distinct player layout and hook signatures are separately
verified.
