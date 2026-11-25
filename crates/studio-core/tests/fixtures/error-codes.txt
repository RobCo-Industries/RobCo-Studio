| Code | Exit | Defined in | When |
|---|---|---|---|
| `usage` | 2 | [Options](#options) | The command line is wrong |
| `project: none` | 3 | [The project folder](#the-project-folder) | The current folder holds no `robco.toml` |
| `manifest: syntax`, `manifest: format`, `manifest: unknown key`, `manifest: type`, `manifest: missing`, `manifest: name`, `manifest: version`, `manifest: title`, `manifest: kind`, `manifest: subtype`, `manifest: platform`, `manifest: abi`, `manifest: path`, `manifest: icon`, `manifest: sources` | 3 | [Errors](./STUDIO_PROJECT.md#errors) | `robco.toml` is refused |
| `lock: syntax`, `lock: format`, `lock: unknown key`, `lock: entry`, `lock: stale`, `lock: hash`, `lock: missing`, `lock: tool` | 3 | [Reading the lock](./STUDIO_PROJECT.md#reading-the-lock), [A stale lock](./STUDIO_PROJECT.md#a-stale-lock), [What the lock pins](./STUDIO_PROJECT.md#what-the-lock-pins) | `robco.lock` is refused, or no longer matches what it pins |
| `new: name`, `new: template`, `new: exists` | 3 | [robco new](#robco-new) | The name, the template or the folder is refused |
| `run: board` | 3 | [Starting QEMU](#starting-qemu) | The package has no such board. `robco test` uses the same code. |
| `test: none`, `test: name`, `test: syntax`, `test: format`, `test: unknown key`, `test: type`, `test: missing`, `test: step`, `test: value`, `test: golden` | 3 | [Scenario files](#scenario-files) | A scenario file or a golden image is refused |
| `package: archive`, `package: format`, `package: manifest`, `package: hash` | 4 | [The archive](./SDK_PACKAGE.md#the-archive), [Reading sdk.json](./SDK_PACKAGE.md#reading-sdkjson) | A package is refused: its archive, a format it uses, one of its files `sdk.json`, `flags.json` and `boot.json`, or a hash |
| `index: location`, `index: read`, `index: syntax`, `index: format`, `index: entry`, `index: duplicate`, `index: host` | 4 | [Reading the index](./SDK_PACKAGE.md#reading-the-index) | The index has no location, could not be fetched or opened, is not valid, or has no archive for this host |
| `sdk: no package`, `sdk: no tool` | 4 | [Version and ABI rules](./SDK_PACKAGE.md#version-and-abi-rules) | Nothing in the index fits |
| `tool: hash` | 4 | [Hashes](./SDK_PACKAGE.md#hashes) | A tool archive fails its check, or the cache holds another archive of that tool |
| `tool: archive` | 4 | [Tool archives](./SDK_PACKAGE.md#tool-archives) | A tool archive cannot be extracted, or does not hold what the index says |
| `tool: program` | 4 | [robco build](#robco-build), [Starting QEMU](#starting-qemu) | An installed tool folder lacks a program the command must start |
| `firmware: hash` | 4 | [Firmware pins](./SDK_PACKAGE.md#firmware-pins) | A firmware file fails its check |
| `cache: path` | 4 | [The cache folder](./SDK_PACKAGE.md#the-cache-folder) | `ROBCO_CACHE` is not an absolute path |
| `templates: none`, `templates: format`, `templates: manifest`, `templates: token` | 4 | [robco new](#robco-new), [templates.json](./SDK_PACKAGE.md#templatesjson) | No `templates` package is installed, or the installed one is refused |
| `build: compile`, `build: link`, `build: objcopy`, `build: seal` | 5 | [robco build](#robco-build) | A build step failed |
| `run: unmatched`, `run: tape` | 1 | [What ends a run](#what-ends-a-run) | The tape ended without the awaited result |
| `run: timeout` | 6 | [What ends a run](#what-ends-a-run) | The time limit passed |
| `sim: start`, `sim: died`, `sim: qmp` | 7 | [Starting QEMU](#starting-qemu) | QEMU did not start, died or did not answer |
| `guest: fault`, `guest: runner` | 8 | [What ends a run](#what-ends-a-run) | The machine failed outside the tape |
| `test: failed` | 1 | [robco test](#robco-test) | At least one run of a scenario failed |
| `io: read`, `io: write` | 9 | This table | The host refused to open, read, write, create or rename a file that exists or should be made. The detail names the path. It covers `robco.toml`, `robco.lock`, the sources, the outputs and the cache. A file of the project that is simply absent has its own code, such as `project: none`. A local index that cannot be read is `index: read`. |
| `internal` | 10 | This table | A fault of the tool itself |
