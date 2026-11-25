# studio-sys

RobCo-Core's C, compiled into Studio, with a thin Rust binding.

## How the C gets in

RobCo-Core is the submodule at `external/robco-core`. `build.rs` compiles 11 of its files with the `cc` crate, the flags of the library's own host tests: `-std=gnu11 -DROBCO_HOST=1 -Wall -Wextra`, warnings as errors.

| Part | Files |
|---|---|
| Library | `src/lib/rimg.c`, `crc32.c`, `ini.c`, `part.c`, `blk_ram.c`, `k_string.c`, `datetime.c` |
| File system | `src/fs/fs.c`, `fs_glue.c` |
| FatFs | `third_party/fatfs/ff.c`, `ffunicode.c` |

The include paths are `include`, `src/fs` and `third_party/fatfs`. `csrc/layout.c` reports the size, alignment and field offsets of the 9 shared structs, 65 entries. `tests/layout.rs` compares each one with Rust's view.

## The C compiler

- Linux and macOS: the system C compiler, as the `cc` crate picks it.
- Windows, MSVC target: the GNU-style `clang` driver. MSVC rejects `__attribute__`, and `clang-cl` drops `-std=gnu11` without an error. The build script looks first in the Visual Studio clang component, through `vswhere`, then for `clang.exe` on `PATH`, then in `C:\Program Files\LLVM\bin`. It stops with a message that says what to install when it finds none. A compiler you name in `CC` is used as given, and refused when it is MSVC-style.

## What it wraps

- `crc32`, `ini_parse`, `ini_get_int`, `rimg_validate`, `rimg_crc`.
- `build_card` and `read_back`: a card image built in memory on the library's RAM block device. Rust writes the MBR, one partition at LBA 2048. FatFs formats and fills it. A fixed clock, the weak `fs_time_hook` of `fs_glue.c`, gives the same bytes on every system.
- The raw declarations are in `src/ffi.rs`.

## Card sizes

`build_card` follows RobCo-Core's `tools/mksdimage.sh` and its storage guide. `card_layout` gives the table as a pure function.

| Size | File system | Partition type | Cluster |
|---|---|---|---|
| 4, 8, 16 or 32 MiB | FAT16 | `0x0E` | 512 bytes |
| 64 to 32768 MiB, powers of 2 | FAT32 | `0x0C` | FatFs default |

The tool rounds a size up to a power of 2, at least 4 MiB, because QEMU's card model needs that. `build_card` takes only the rounded sizes and refuses every other size before it allocates. One-sector clusters keep each FAT16 card between 4085 and 65524 clusters. After formatting, `build_card` asks the file system for the FAT type with `fs_statvfs` and fails if it is not the type of the partition.

The file system's state is global in the C. The card functions hold one lock, so one caller at a time uses it.

## What is tested and what is not

- Tested on Ubuntu, macOS and Windows in CI: the build, the 65 layout entries, CRC-32, INI, `rimg_validate` on an image sealed by `tools/rimg.py`, the card size table at each edge, FAT16 cards of 4 and 32 MiB, and a FAT16 card of 16 MiB whose SHA-256 starts with `ee100f0f93696cff` on each system. A Linux job reads that card with mtools: `scripts/check_card.sh`.
- Built but not checked by another tool: a FAT32 card of 64 MiB. Its FAT type is checked, nothing reads it with mtools.
- Not tested: FAT32 cards above 64 MiB, cards larger than memory, and ARM64 Windows.

## Licences

| Part | Licence |
|---|---|
| This crate's Rust and `csrc/layout.c` | GPL-3.0-or-later |
| RobCo-Core's C, compiled from the submodule | MIT, see `external/robco-core/LICENSE` |
| FatFs, `third_party/fatfs` in RobCo-Core | The FatFs licence, a one-clause BSD-style licence by ChaN, see `external/robco-core/third_party/fatfs/LICENSE.txt` |
| The `cc` crate and its dependencies `find-msvc-tools` and `shlex`, used only at build time | MIT OR Apache-2.0 |

Both C licences let a GPL-3.0-or-later program link the code.
