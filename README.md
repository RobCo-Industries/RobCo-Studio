# RobCo Studio

RobCo Studio is the development tool for the RobCo Industries operating systems. It is a `robco` command and a window. It will build holotapes and apps for [Pip-OS](https://github.com/RobCo-Industries/Pip-OS) and [UOS](https://github.com/RobCo-Industries/UOS), run them in an emulator and write cards.

## Status

Nothing is usable yet. What exists:

- A Cargo workspace with six crates. `robco sdk list` and `robco sdk install` list and install SDK packages and tools from a local or an `https://` index, and `robco build` builds a holotape project into `build/NAME.HTP`, by [STUDIO_CLI.md](https://github.com/RobCo-Industries/RobCo-Core/blob/main/docs/spec/STUDIO_CLI.md). No other command exists yet. There is no default index: `--index` is required. `robco --version`, `robco version` and `robco-studio --version` print the version. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- `robco-studio` opens the first window. See [The window](#the-window).
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a link check of the Markdown files.
- Seven spikes, throwaway code that answers one question each, with their results. No RobCo-Core C is linked into the crates yet. See [docs/SPIKES.md](docs/SPIKES.md).

To build and test, see [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## The window

`robco-studio` opens a 1280 by 720 window on no project. `robco-studio PATH` opens the folder PATH as the project, and File, Open folder, Cmd or Ctrl+O, opens another through the system's folder dialog. To try it from a checkout:

```sh
cargo run -p robco-studio -- path/to/a/folder
```

What works today:

- The window's regions: the menus, the toolbar, the page and the status bar. The toolbar starts with one control for the four pages: Project, Card, Device and SDK. On macOS the menus are in the system's menu bar. On Windows and Ubuntu they are a row in the window.
- The Project page: Files, the editor, the simulator, the tape and the bottom pane, at their default sizes. The dividers between them drag, and take focus: the arrow keys move them and Enter puts them back.
- Files lists the real folder. A click opens a file in a tab.
- The editor edits plain text and C, with line numbers and colours for C. It saves with Cmd or Ctrl+S, finds and replaces with Cmd or Ctrl+F, and keeps a file's CRLF line endings. A file it cannot open as text is refused with the reason, and nothing changes.
- A tab with unsaved edits shows a square after its name. Closing it, or quitting, asks "Save ... before closing?" first.
- F6 and Shift+F6 move between the panes. Every control takes keyboard focus and has a name for a screen reader.
- The green, amber, blue and white schemes and high legibility, from the View menu.
- View, Screen: Off, Soft, Still, Live and Arcade, and Adjust with a slider for each effect. Still, Live and Arcade pass the whole window through a CRT shader: bloom, vignette, scanlines, a curved glass, and in Live and Arcade persistence, noise, flicker and a rolling band. Live and Arcade animate only while the window has focus. Studio starts in Live on a graphics adapter, in Still when the system asks for reduced motion, which only macOS tells it, and in Soft on a software adapter. `--screen LEVEL` starts at a level.

What does not work yet, and says so:

- Build, Run, Debug and Write card do nothing. They are drawn with a dashed frame, and their tooltip says why: nothing was built, no tape ran, nothing was written. Nothing in the window pretends a job ran.
- The simulator, the tape pane and the bottom pane show their empty states. The Card, Device and SDK pages say they come in a later release.
- There is no settings file, no recent projects list, and no "Go to line". Studio does not notice a file changed by another program.

The window writes no file except one you save in the editor. It opens no network connection and starts no process.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

The licence covers Studio's own code. It does not cover what you make with Studio: your projects, your art, your tapes and your card images are yours, under any licence you pick. The files that end up inside a tape live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT.

## Names

The names come from the Fallout games. All code, art, sound and text in this repository are original. This project is not affiliated with the owners of Fallout.
