# pdf-impose

Hot-folder imposition in Rust. Drop a PDF into `PDFIn/` and it is imposed in a
**single row** (first bleed to last bleed) that is centred on a 12.375" wide sheet
(configurable). The eye mark is ignored while centring. The sheet height is the
PDF's bleed height. Then a 0.25" × 0.25" eye mark is placed just outside the left
bleed of the first copy, with its top edge aligned to the trim top.

Pages are reused as vector Form XObjects (via `lopdf`), with no rasterising and no
re-encoding, so jobs take milliseconds and output quality matches the input.

## Build & run (macOS)

```bash
# one-time: install Rust (needs 1.89 or newer; `rustup update` to upgrade)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cd ~/Projects/PDFTooling
cargo build --release
./target/release/pdf-impose PDFIn          # start watching
```

On first start it creates `PDFIn/impose.toml` with every setting and its default.

```
PDFTooling/
├── PDFIn/              ← drop PDFs here
│   ├── impose.toml     ← settings (re-read for every job, no restart needed)
│   ├── Processed/      ← originals after success
│   └── Error/          ← failed PDFs + <name>.error.txt with the reason
└── PDFOut/             ← imposed files: <name>_<N>up.pdf
```

Other commands:

```bash
pdf-impose init PDFIn                                   # just create folder + config
pdf-impose impose in.pdf out.pdf -c PDFIn/impose.toml   # one-off, no watching
RUST_LOG=debug pdf-impose PDFIn                         # more logging
```

## Build & run (Windows)

### Option A — build on the Windows PC (recommended)

1. Install the **Visual Studio Build Tools** (the free C++ build tools Rust uses for linking):
   https://visualstudio.microsoft.com/visual-cpp-build-tools/ → tick **"Desktop development with C++"**.
   Or in PowerShell: `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
2. Install Rust: download and run **rustup-init.exe** from https://rustup.rs (accept the defaults),
   or `winget install Rustlang.Rustup`. Open a **new** terminal and check `cargo --version` (1.89+).
3. Copy the project folder over (without `target\`), then in PowerShell:

```powershell
cd C:\PDFTooling
cargo build --release
.\target\release\pdf-impose.exe PDFIn        # start watching
```

The program is a single `pdf-impose.exe`. `.cargo/config.toml` builds the C runtime into it, so you
can copy just that file to other 64-bit Windows 10/11 PCs without installing anything else.
(On an ARM Windows PC, the same steps produce a native ARM build.)

### Option B — build the .exe on the Mac (cross-compile)

```bash
brew install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/pdf-impose.exe
```

Copy that `.exe` to the Windows PC. It runs exactly like the Option A build.

### Windows notes

- Paths in `impose.toml`: use forward slashes or single quotes, because a backslash inside
  double quotes is an escape in TOML:
  `output = "D:/Jobs/PDFOut"` or `output = 'D:\Jobs\PDFOut'`. Network shares work too:
  `output = '\\server\print\PDFOut'`.
- Log level in PowerShell: `$env:RUST_LOG="debug"; .\pdf-impose.exe PDFIn`
  (in cmd: `set RUST_LOG=debug && pdf-impose.exe PDFIn`).
- Watching a folder on a network share works, but Windows doesn't always send change events for
  shares; the safety rescan (`folders.poll_ms`) still picks files up.
- If a PDF is still open in another program (e.g. Acrobat) when the job finishes, Windows won't let it
  be moved to `Processed`. The output is still written; close the file and move it yourself.

## How the layout is decided

1. **Boxes**: the BleedBox is used as the slot (falls back to CropBox, or TrimBox + `bleed.default_bleed`).
   Anything outside the bleed (slug, printer marks) is clipped.
2. **Orientation**: `auto` keeps each page exactly as the PDF shows it (including `/Rotate`).
   `portrait` / `landscape` turn pages that don't match.
3. **Across**: `floor((sheet.width − margins) / bleed width)`, optionally capped by `layout.max_across`.
   The mark does not take space from `sheet.width` (unless `mark.reserve_space = true`).
4. **Filling the row**

| Pages | Slots | Result (grouped) |
|------:|------:|------------------|
| 1 | 4 | 1 1 1 1 |
| 2 | 4 | 1 1 2 2 |
| 3 | 4 | 1 1 2 3 (`remainder = "fill"`) or 1 2 3 _ (`"blank"`) |
| 4 | 4 | 1 2 3 4 |
| 10 | 4 | sheet 1: 1 2 3 4 · sheet 2: 5 6 7 8 · sheet 3: 9 9 10 10 |

   `order = "collated"` gives 1 2 1 2 instead of 1 1 2 2.
5. **Sheet width**: by default every sheet is exactly `sheet.width` wide and the row is centred
   (`align = "center"`, mark ignored); e.g. 4 × 3" bleed → 12" row with 0.1875" each side.
   With `shrink_width_to_content = true` the page is cut to `mark + row` instead.
   If the row fills the whole width there is no room left for the mark, and a warning is logged.
6. **Eye mark**: placed after the row; right edge touches the first copy's left bleed edge (+`offset_x`),
   top on the trim top (or `anchor = "bleed_top"`), colour set by `cmyk`.

All lengths in `impose.toml` use `units` (`in`, `mm` or `pt`).

## Run at login (optional)

### macOS

Save as `~/Library/LaunchAgents/com.leom1.pdf-impose.plist`, then
`launchctl load ~/Library/LaunchAgents/com.leom1.pdf-impose.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.leom1.pdf-impose</string>
  <key>ProgramArguments</key><array>
    <string>/Users/leom1/Projects/PDFTooling/target/release/pdf-impose</string>
    <string>/Users/leom1/Projects/PDFTooling/PDFIn</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>/Users/leom1/Projects/PDFTooling/pdf-impose.log</string>
</dict></plist>
```

### Windows: start automatically at login

Simplest: press **Win+R**, type `shell:startup`, and put a shortcut there with
Target `C:\PDFTooling\target\release\pdf-impose.exe C:\PDFTooling\PDFIn` and
Start in `C:\PDFTooling`. Set **Run** to *Minimized* so the log window stays out of the way.

To run it without anyone logged in, create a Task Scheduler task instead
(Trigger: *At startup*, Action: the same program and argument,
*Run whether user is logged on or not*).

## Notes

- Files are processed only after their size has been stable for `folders.stable_ms`, so slow copies
  and network drops are safe. Output is written to a temp name and renamed, so downstream hot folders
  never see half-written files.
- Interactive features (bookmarks, form fields, links) are dropped; the output is meant for print.
- Password-protected PDFs (with an open password) go to `Error/`.
