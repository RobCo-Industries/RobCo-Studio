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

The file system's state is global in the C. The card functions hold one lock, so one caller at a time uses it.

## What is tested and what is not

- Tested on Ubuntu, macOS and Windows in CI: the build, the 65 layout entries, CRC-32, INI, `rimg_validate` on an image sealed by `tools/rimg.py`, and a FAT16 card of 16 MiB whose SHA-256 starts with `ee100f0f93696cff` on each system. A Linux job reads that card with mtools: `scripts/check_card.sh`.
- Not tested: FAT32 cards, from 64 MiB, cards larger than memory, and ARM64 Windows.

## Licences

| Part | Licence |
|---|---|
| This crate's Rust and `csrc/layout.c` | GPL-3.0-or-later |
| RobCo-Core's C, compiled from the submodule | MIT, see `external/robco-core/LICENSE` |
| FatFs, `third_party/fatfs` in RobCo-Core | The FatFs licence, a one-clause BSD-style licence by ChaN, see `external/robco-core/third_party/fatfs/LICENSE.txt` |
| The `cc` crate and its dependencies `find-msvc-tools` and `shlex`, used only at build time | MIT OR Apache-2.0 |

Both C licences let a GPL-3.0-or-later program link the code.
