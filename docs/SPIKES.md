# Spike results

A spike is throwaway code that answers one question about how Studio can be built, with evidence: CI runs, measurements and tests. Nothing is built on spike code. The real code starts again from the plan and from the rules these pages found.

Each spike has its own page in [`docs/spikes`](spikes/). This page is the index. Every statement here comes from those pages. Where they differ, the spike page is right.

## The seven spikes

| Spike | Question | Answer | The fact that carries it |
|---|---|---|---|
| [K1](spikes/K1.md) | Does our own QEMU 10.1.2 build pass RobCo-Core's canary on three systems? | Yes | 78 of 78 on each system in the latest run, one pass, no retry. Earlier first passes: 77 or 78 of 78 |
| [K2](spikes/K2.md) | Does xPack gcc 15.2.1 build Culvert to the same bytes everywhere? | The sealed tape, yes. Intermediate files need cures | `CULVERT.HTP` has one SHA-256 on Ubuntu, macOS and Windows, and through Make and the direct build |
| [K3](spikes/K3.md) | Does RobCo-Core's C build through Cargo, and can Rust write a card? | Yes, with `CC=clang` on Windows | 11 C files, 6 Rust tests pass on three systems. The card is the same bytes on each and boots in QEMU |
| [K4](spikes/K4.md) | egui or GPUI for the window? | egui | `gpui` 0.2.2 does not build on Linux today. egui passes its headless tests on three systems |
| [K5](spikes/K5.md) | Is QMP `screendump` fast enough for the simulator? | Yes | 145 to 260 frames a second at 640 by 480 flat out. At 30 a second the guest runs about 10 % slower |
| [K6](spikes/K6.md) | Can a Rust card writer write and verify a disk, and refuse a system disk? | Yes, on virtual disks in CI | 60 MiB written and read back equal on three runners. The system disk refused on each. No real card reader |
| [K7](spikes/K7.md) | Does a plain text editor hold up in egui? | Yes at 5,000 lines, on one Mac | 113 to 117 fps while typing at 5,000 lines. 57 to 60 fps at 20,000 lines |

## Each spike in short

### K1: our own QEMU on three systems

- Proven in CI on GitHub's `ubuntu-latest`, `macos-latest` arm64 and `windows-latest`: QEMU 10.1.2 builds with the same configure line as RobCo-Core's CI, and the macOS and Windows packages run on a fresh runner with no build libraries.
- Runs A to D passed 77 or 78 of 78 canary cases on a first pass on each system. Every failure was one racy case in RobCo-Core's own test. Run E pins RobCo-Core `c6b6482`, which fixes it, drops the retry, and passes 78 of 78 on each system. That is one run.
- Windows runs the same test file through a small plugin that swaps Unix sockets for TCP loopback. QEMU on Windows serves QMP, serial and qtest over TCP.
- The spike fixed a Windows serial socket that kept a timeout. A review found two holes in the first verdict that read a broken setup as a pass. Both are fixed, and a CI job proves the verdict fails on a broken setup.
- From `c6b6482` the library's harness starts QEMU through `/bin/sh` and process groups. The Windows plugin bypasses that, so on Windows nothing guarantees that QEMU dies with the test process. Studio must give that guarantee itself on Windows.
- Not proven: Linux portability, macOS on Intel, the oldest macOS and Windows that run the package, a user's machine with Gatekeeper or an antivirus, the GPG signature of the QEMU archive, and timing cases over many runs.

### K2: Culvert bytes across systems

- Proven in CI on Ubuntu, macOS and Windows: the sealed `.HTP` is equal on the three systems, raw and cured, and Make and the direct build give the same ELF and `.HTP` on Ubuntu.
- Objects, `libpip.a` and the ELF are equal only with cures: two `-ffile-prefix-map` flags, `-fno-ident`, `ar rcsD` and a checkout without CRLF. The full Windows ELF still differs in its debug sections, through xPack's own `libgcc.a`.
- Ubuntu's own `gcc-arm-none-eabi` 14.2.1 gives a different `.HTP`, as expected from another compiler version.
- Not proven: whether CRLF sources change any object, the SDK's sample tape, Windows on arm64 and macOS on x64.

### K3: RobCo-Core's C through Cargo, and a card from Rust

- Proven in CI on Ubuntu, macOS and Windows: 11 C files build and link through the `cc` crate, and 6 Rust tests pass. Windows needs `CC=clang`. MSVC fails on `__attribute__`. `clang-cl` builds but drops `-std=gnu11` without an error.
- The card written from Rust on each system is the same bytes. mtools reads all four files, and QEMU boots it on raspi2b in the suite's container and reads three files with the right size and CRC.
- The weak symbol `fs_time_hook` links on all three. The plan's fallback for weak symbols is not needed today.
- Not proven: FAT32 cards, a card larger than RAM, ARM64 runners, the sanitizer flags through `cc`. The MSVC and clang-cl rows come from one run each.

### K4: egui or GPUI

- Proven in CI: egui builds on the three systems and passes both headless tests there, including a log scroll and "Jump to end". GPUI builds on macOS and Windows and passes its one headless test, which presses two buttons.
- `gpui` 0.2.2 does not build on Ubuntu: a dependency chain needs `libc::ENOATTR`, which `libc` 0.2.190 removed. A CI job checks this finding.
- Frame rates come from one Mac only, with a 120 Hz display. Both reach the display rate on the canvas. egui held 29.5 fed frames a second on the screen test. GPUI's screen test never reached a clean run. egui's scrolling log rate, 88.2 fps, is one run. These numbers do not rank the two on speed.
- egui pulls 427 packages against 718 and builds in about half the time.
- Not proven: frame rates on Ubuntu and Windows, image snapshots, GPUI at a fractional display scale, `gpui-pre`.
- The audit of this page narrowed two claims: egui's log rate is one run, and GPUI's log was never tested headless. Neither changes the pick.

### K5: frame rate of QMP screendump

- Measured on one Mac under load, load averages 4 to 7, and on a hosted Linux runner in the suite's container. Each phase lasted 2 s, so each number is one short sample and a lower bound.
- Flat out at 640 by 480, PPM to a file: 145 to 260 frames a second on `raspi0`, `raspi2b` and `raspi3b`. At a fixed 30 a second, about 4 to 7 ms per frame from request to decoded pixels.
- The cost is the guest's: each call stops it for about 4 ms. At 30 a second it runs about 10 % slower, at 15 about 3 to 4 %, flat out at about a quarter of its speed.
- PNG costs twice the time per frame, and the suite's QEMU has no PNG. VNC costs the guest nothing but tops out at about 29 updates a second with about 35 ms of delay, and the suite's QEMU has no VNC. Both ran on the Mac only.
- One of two CI runs that wrote frames to disk had stalls up to 300 ms. `/dev/shm` and a named pipe had none above 14 ms.
- Not proven: macOS and Windows runners, K1's QEMU builds, a calm machine, PNG on a busy screen, real key input.

### K6: the card writer

- Proven in CI on Ubuntu, macOS and Windows, on virtual disks only: a 64 MiB loop device, disk image and VHD. A 60 MiB image is written and read back equal, and a different image is reported different.
- The runner's own system disk is refused on all three before the writer opens it. On Linux `strace` shows no open with write flags.
- The rules are one pure function. 22 tests run it on recorded lists written by hand. Only `decide` can build an `Approved` value, and only that value opens a disk for writing.
- Three review rounds and one sweep found faults of two kinds: a checked property not carried to where it is used, and a system value put in the wrong class. All are fixed with tests.
- Not proven: any real card reader or SD slot, unmounting on macOS and Windows, a real image with a partition table, the macOS drive cache flush, a second byte check on Windows. The elevated helper was not built.

### K7: a plain text editor in egui

- Window rates come from one Mac with a 120 Hz display, one run per row. At 5,000 lines: 113 to 117 fps while typing one character a frame, 117 fps while scrolling, with the gutter in step in every frame. At 20,000 lines typing fell to 57 to 60 fps with one core busy.
- Proven in CI on Ubuntu, macOS and Windows: the headless tests for typing, undo, copy and paste, search, go to line, the gutter, exact bytes on save, refused files, and the review fixes. The editor uses egui's own `TextEdit` and adds no package.
- The cost split ran on the three runners. On each edit our tokenizer runs over the whole text, and egui hashes the whole job and rebuilds the whole galley, so the cost grows with the file. A view of the visible lines only would cost about 0.02 ms of text layout, but it was not built.
- Three reviews found faults of one kind: state that outlives the thing it belongs to. All are fixed with tests.
- Not proven: rates on Ubuntu and Windows, 10,000 lines, a slower machine, mouse selection, colours and font on screen, the system clipboard.

## Choices these results leave open

The owner decides. Each point lists the options as its page gives them. No option is picked here.

### Building tapes, from K2

- How do the path maps reach the compiler? (a) `flags.json` carries placeholders for the project and toolchain directories, (b) Studio adds the maps itself, or (c) tape builds drop `-g`. [K2](spikes/K2.md#open-points-for-the-owner)
- Where does `-fno-ident` come from? (a) `pipos-app.mk` adds it, (b) Studio adds it, or (c) only the ELF and the `.HTP` are compared. [K2](spikes/K2.md#open-points-for-the-owner)
- How is `libpip.a` made equal? (a) Pip-OS moves to `ar rcsD`, (b) the SDK ships a prebuilt archive, or (c) only the ELF and the `.HTP` must be equal. [K2](spikes/K2.md#open-points-for-the-owner)
- How are ELFs compared? (a) without debug info, or (b) only the `.HTP` is compared. [K2](spikes/K2.md#open-points-for-the-owner)

### Shipping QEMU, from K1

- How is the Linux build made portable? Copy the six libraries into `lib/` and set the run path with `patchelf`, or link glib and pixman statically. Building in an older distribution or the manylinux image would widen the glibc range. [K1](spikes/K1.md#what-stays-unknown)
- How does a downloaded macOS build get past quarantine? The SDK manager clears the attribute, or the release is signed and notarised. [K1](spikes/K1.md#what-stays-unknown)

### The simulator screen, from K5

- Which rate to offer? Fixed 30, fixed 15, a user setting, or a rate that adapts and slows when the guest's markers show it slowed. [K5](spikes/K5.md#what-it-means-for-studio)
- Where does QEMU write the frame? A file in the system's temporary folder, as the harness does; a named pipe on macOS and Linux with a file on Windows; or memory-backed storage where there is some, `/dev/shm` on Linux. [K5](spikes/K5.md#what-it-means-for-studio)
- One QMP connection or two? One is enough for keys. A second cuts key latency and keeps the poller and the input path apart, for one more `-qmp` argument. [K5](spikes/K5.md#what-it-means-for-studio)
- Skip unchanged frames? Studio can hash each frame and skip the upload when nothing changed. The page leaves it to milestone S2. [K5](spikes/K5.md#what-it-means-for-studio)

### The card writer, from K6

- A built-in SD slot on Linux reports RM=0 and is refused today. Accept `mmc` with RM=0, or tell the user to use a USB reader? [K6](spikes/K6.md#what-it-means-for-studio)
- How does the macOS write get its privilege? `authopen` hands a descriptor to the main program after the admin prompt, or a helper runs as root through `SMAppService`, which needs signing. [K6](spikes/K6.md#the-elevated-helper)
- How are disks held by a pool or array member with no child refused? On Linux read FSTYPE and refuse `LVM2_member`, `linux_raid_member` and `zfs_member`. On macOS refuse any APFS physical store or RAID member. On Windows refuse a disk whose `PartitionStyle` is not MBR or GPT, or whose `IsClustered` or pool membership is set. [K6](spikes/K6.md#what-stays-unknown)

### The editor and the window, from K7 and K4

- When files above about 10,000 lines must edit at the display rate, or the editor grows towards analysis, completion, folding, several cursors or gutter diagnostics: write our own visible-lines view on egui, try a crate such as `egui-pop-editors` or `egui_code_editor`, or weigh GPUI with GPUI Kit's editor. [K7](spikes/K7.md#what-it-means-for-studio)
- The old `TextEdit` ids' state stays in egui's memory until exit. S2 removes it, or keeps one id per open tab on purpose. [K7](spikes/K7.md#what-the-review-found)
- If GPUI is ever used, it means one of: pinning a hand-picked set of old crates to keep 0.2.2 building on Linux, the weekly snapshot `gpui-pre`, or a git revision of Zed pinned by hash. [K4](spikes/K4.md#what-it-means-for-studio)

## What the spikes changed in the code that follows

These rules came out of the spikes and their reviews. The real code must keep them.

- **Card writer, from K6.** Carry every checked property to where it is used: write and read back exactly the checked length from the one image handle, pad to the disk's own sector size, list and decide again after opening the disk and require the same path, size, sector size and image length. Put every system value into a class on purpose and send the unknown to a refusal: when two sources describe one fact and differ, the answer is unknown. [K6](spikes/K6.md#what-the-reviews-found)
- **Editor, from K7.** Never drop unsaved edits without asking. Undo, cursor and scroll belong to one document, whose state lives in one struct replaced whole on load. A document has its own path or none. The buffer holds LF only, guarded at every input event. One modal question at a time, carrying its target and its document. A cache key holds everything the value depends on. [K7](spikes/K7.md#what-the-review-found)
- **Canary verdict, from K1.** The verdict reads pytest's exit code and the JUnit report, and fails on any failed case, any error element, any skip, a case count other than the expected one, and a report that is missing, empty or not XML. It is one function that runs nothing, with unit tests and a CI job that proves it fails on a broken setup. [K1](spikes/K1.md#the-verdict)
- **Equal bytes, from K2.** Compile with the flags of `pipos-app.mk` in its order, in the project directory, with relative paths. Keep the link order. Map the project and toolchain directories with `-ffile-prefix-map`, both slash spellings on Windows. Add `-fno-ident`. Build `libpip.a` with `ar rcsD`. Read sources as committed bytes. [K2](spikes/K2.md#what-it-means-for-studio)
- **C build, from K3.** Use the 11 files of the spike plus `rimg_seal.c`, from the submodule at `external/robco-core`. Set `CC=clang` on Windows in the sys crate's own config, not by hand. Turn C warnings into errors. Serialise every use of the file system. Keep the layout test. [K3](spikes/K3.md#what-it-means-for-studio)
- **Screen capture, from K5.** Each `screendump` stops the guest for about 4 ms. Studio must pace its polls and never poll flat out. Use PPM, not PNG. [K5](spikes/K5.md#what-it-means-for-studio)
