# PDFTooling

Two imposition programs built from this project (`cargo build --release` builds both):

| Program | Use it for |
|---|---|
| `pdf-impose` | Hot folders: single-row step-and-repeat on a fixed-width web/roll (eye mark, die, white underlay) |
| `sheet-impose` | Sheet-fed and large-format: work styles, booklets, signatures, gang/cut & stack, marks, colour bars — see [sheet-impose](#sheet-impose) |

## pdf-impose

Hot-folder imposition in Rust. Drop a PDF into `PDFIn/` and it is imposed in a
**single row** (first bleed to last bleed) that is centred on a 12.375" wide sheet
(configurable). The eye mark is ignored while centring. The sheet height is the
PDF's bleed height. Then a 0.25" × 0.25" eye mark is placed just outside the left
bleed of the first copy, with its top edge aligned to the trim top, and duplicated in place as an
overprinting spot colour named "die" (shown as magenta). Behind it sits a "white" spot-colour
underlay (shown as 10% black), as wide as the mark and running from the top edge of the sheet to the
bottom edge.

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

## Watching several folders

List the hot folders in `hotfolders.toml` (next to the program, or anywhere you like):

```toml
[[hotfolder]]
path = "PDFIn"                      # uses PDFIn/impose.toml

[[hotfolder]]
path = "6RAutoImpose"               # uses 6RAutoImpose/impose.toml

[[hotfolder]]
path    = "/Volumes/Jobs/Labels/In" # absolute paths and network shares work
config  = "shared/labels.toml"      # optional: share one settings file between folders
name    = "Labels"                  # optional: label shown in the log
enabled = false                     # optional: skip without deleting the entry
```

```bash
./target/release/pdf-impose                      # uses ./hotfolders.toml if it exists, else ./PDFIn
./target/release/pdf-impose hotfolders.toml      # or name the list explicitly
./target/release/pdf-impose init hotfolders.toml # write a starter list
```

- Relative paths in the list are relative to the list file.
- Each folder uses its own `impose.toml` unless `config` points elsewhere. Missing folders and
  settings files are created with defaults. Paths inside a settings file (`output`, `processed`,
  `error`) stay relative to each hot folder, even when the file is shared.
- The list is re-read when it is saved: added folders start, removed or `enabled = false` folders
  stop, and no restart is needed. If the list has a mistake, the error is logged and the folders
  already running keep going.
- Log lines are tagged with the folder, e.g. `[6RAutoImpose] ✔ card.pdf → card_4up.pdf`.

### Row and sheet folders in one list

A hot folder can run either engine. Add `type` to an entry, or leave it out and it is worked out
from the settings file in the folder:

| `type` | Engine | Settings file in the folder |
|---|---|---|
| `row` | pdf-impose (single row, eye mark, die, white underlay) | `impose.toml` |
| `sheet` | sheet-impose (work styles, signatures, lip, gang, marks, colour bar) | `job.toml` |

```toml
[[hotfolder]]
path = "6RAutoImpose"          # impose.toml inside → row

[[hotfolder]]
path = "Sheet/Booklets"        # job.toml inside → sheet
type = "sheet"                 # (or say so explicitly)

[[hotfolder]]
path   = "Sheet/BooksRush"
config = "jobs/saddle-38x25.toml"   # several sheet folders can share one job file
```

Either program can run the list (`pdf-impose hotfolders.toml` or `sheet-impose hotfolders.toml`),
and one running process handles row and sheet folders together. A new `type = "sheet"` folder gets
a commented `job.toml`. Its `[folders]` section (output, processed, error, after_success, stable_ms,
poll_ms) works the same as in `impose.toml`.

Other commands:

```bash
pdf-impose PDFIn                                        # watch just one folder
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
$env:RUSTFLAGS="-C target-feature=+crt-static"   # build the C runtime into the .exe
cargo build --release
.\target\release\pdf-impose.exe PDFIn        # start watching
```

This builds both `pdf-impose.exe` and `sheet-impose.exe`. Each program is a single `.exe`. Because of the `RUSTFLAGS` line, the C runtime is built
into it, so you can copy just that file to other 64-bit Windows 10/11 PCs without installing anything
else. (Skip that line and the PC running it needs the Microsoft Visual C++ Redistributable.)
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
   A duplicate is placed exactly on top of it in the spot colour **die** (lowercase; spot names are case-sensitive) (a Separation colour,
   shown as 100% magenta), set to **overprint**, so the black mark still prints underneath and the
   die line comes out on its own separation. Change or switch it off under `[mark.spot]`
   (`name`, `tint`, `cmyk` = on-screen colour, `overprint`, `enabled`). In Acrobat, turn on
   *Output Preview* or *Overprint Preview* to see both marks.
7. **White underlay**: drawn first, behind the mark, in the spot colour **white** (shown as 10% black).
   Same x and width as the eye mark; height from the top edge of the sheet to the bottom edge
   (`extent = "page"`), or from the first copy's trim top to trim bottom with `extent = "trim"`.
   The black eye mark (`mark.overprint = true`) and the die mark both overprint it, so the white
   ink still prints under them. Settings are under `[mark.underlay]` (same keys as `[mark.spot]`).

   Paint order on the sheet: pages → white underlay → black eye mark (overprint) → die (overprint).

All lengths in `impose.toml` use `units` (`in`, `mm` or `pt`).

# sheet-impose

Imposition for sheet-fed presses and large-format / roll printers.

```bash
./target/release/sheet-impose init job.toml                 # commented job file with every setting
./target/release/sheet-impose book.pdf -c job.toml          # → book_sheetwise_saddle.pdf next to the input
./target/release/sheet-impose book.pdf out.pdf -c job.toml --set sheet.width=28 --set sheet.height=20
./target/release/sheet-impose book.pdf -c job.toml --plan   # show the layout + page map, write nothing
```

Every setting in `job.toml` can be overridden with `--set section.key=value`, so one job file
can serve many jobs.

**As a hot folder:**

```bash
./target/release/sheet-impose init Sheet/Booklets      # folder + commented job.toml
./target/release/sheet-impose Sheet/Booklets           # watch one folder
./target/release/sheet-impose hotfolders.toml          # watch a list (row and sheet folders)
```

Drop PDFs in. The output goes to `[folders] output` (default `../PDFOut`) as
`{name}_{work}_{binding}.pdf`, and the original moves to `Processed/`. A PDF that can't be imposed
goes to `Error/` with a `.error.txt` giving the reason. `job.toml` is re-read for every PDF.
A mistake in it is logged and the PDFs wait until it's fixed.

### Work styles (`press.work_style`)

| Value | What comes out |
|---|---|
| `single` | Front plates only |
| `sheetwise` | Work & back: a front page and a back page per sheet (sheet turned left-to-right) |
| `perfect` | Perfecting press: front + back pages, back flipped head-to-foot (`press.back_flip` to change) |
| `workturn` | Work & turn: one plate holds the front on one half and the back on the other; turned left-to-right, cut in half |
| `worktumble` | Work & tumble: same, but tumbled head-to-foot (front on the bottom half) |

Back plates are always written the way the press prints them (back view), so the back of every
page lands exactly behind its front.

### Binding styles (`binding.style`)

| Value | Behaviour |
|---|---|
| `flat` | Single pieces (cards, flyers). Fewer pieces than positions are repeated to fill the sheet; 2-sided work pairs pages 1/2, 3/4… as front/back |
| `cutstack` | Pieces ordered so that after cutting, each stack is in sequence (stack 1 = 1-4, stack 2 = 5-8 …) |
| `saddle` | Folded signatures **nested** inside each other (4pp: 16\|1 / 2\|15, 14\|3 / 4\|13 …); blank pages added to reach a multiple of 4; creep compensation (below) |
| `perfect` | Folded signatures **gathered** one after another (1-16, 17-32 …); optional spine `grind` |

`binding.edge` = `left`, `right` (right-to-left books) or `top` (calendars / pads).

**Lip — high folio / low folio** (`binding.lip`, `binding.lip_side`): adds a lip (lap) to every
folded signature, so the stitcher or gatherer can grab one half and open the signature at the centre.

| `lip_side` | Lip goes on |
|---|---|
| `none` | no lip (default) |
| `low` | the **low folio** half: the leaves with the lower page numbers (pages 1 … n/2 of each signature) |
| `high` | the **high folio** half: the leaves with the higher page numbers |

The lip is added beyond the **face** of every page in that half. Face folds between two pages of
the lip half get it on both sides, and an outer face makes the block wider. The pages and fold
positions stay correct, and the extra paper is trimmed off with the face.

```bash
sheet-impose book.pdf --set binding.style=saddle --set binding.lip=0.375 --set binding.lip_side=low
```

**Creep** (`binding.creep`, `paper_caliper`, `creep_method`, `creep_direction`): when folios are
nested, the inner pages push out at the face and get more trimmed off. Creep compensation moves
each page's content toward the spine by an amount that grows linearly from 0 on the outer folio
to the full creep on the innermost folio.

| Setting | Meaning |
|---|---|
| `creep = 0.0625` | creep of the innermost folio (saddle: centre spread of the whole book; perfect: centre of each signature) |
| `paper_caliper = 0.004` | used when `creep = 0`: creep = caliper × (folios − 1) |
| `creep_method = "shift"` | move the content toward the spine (exact size; a sliver at the spine is lost) |
| `creep_method = "scale"` | shrink the content toward the spine edge so nothing is lost at the spine |
| `creep_direction = "in"` | toward the spine (normal); `"out"` reverses it |

The log prints the creep table, e.g.
`creep — shift toward the spine (in): 3-4/13-14 0.0208, 5-6/11-12 0.0417, 7-8/9-10 0.0625`.
Trim positions and marks stay where the sheet is cut.

**Creep marks** (`marks.creep = true`): every crept page gets a short **dashed** mark at its head
and foot, at the position where its face will actually trim (the trim moved in by that page's creep).
These follow the same rules as trim marks: they start beyond the bleed and never cross any trim or bleed.

**Signatures are computed by simulating the folds** (4, 8, 16 and 32 pages, right-angle folds).
The program folds a virtual sheet, reads the folded section like a book and works back to the page
number and the head direction of every position, so the output is correct for any binding edge.
For each it tries every fold direction and keeps the shop-standard layout: page 1 on the front,
heads together at the head folds. Spine folds are butted (no bleed across a fold, `grind` adds space for
milling). Head and face folds get `layout.fold_gap`, which defaults to 2 × bleed.

`binding.signature_pages = 0` (auto) tries 32/16/8/4 and keeps whatever needs the fewest sheets;
a 40-page book on a 40×28 sheet becomes 32pp + 8pp, for example. Set 4/8/16/32 to force a size.

### Filling the sheet

- Blocks (signatures or pieces) are packed as a grid; if turning the whole layout 90° fits more,
  it does (`layout.allow_rotation`).
- **Mixed rotation:** if turning *some* blocks 90° into the leftover strip fits more, it does that
  and logs it (`layout.mixed_rotation`). With mixed rotation off, the log still tells you how many
  more would fit. Example: `mixed rotation: 2 upright + 4 turned 90° = 6 per sheet (best without mixing: 4)`.
- **1 folding sheet per signature** (`layout.one_signature_per_sheet = true`, saddle and perfect):
  every sheet carries one **single-fold folio** (4pp), with every page upright. Copies are
  repeated **head to foot, all facing the same direction**, as many as fit, with no mixed rotation.
  Sheet 1 = 16|1 (back 2|15) × n, sheet 2 = 14|3 × n, and so on. The log tells you to run each sheet
  at 1/n of the quantity.
  Example: 16-page 4×6 on 19×13 with 0 margins/gripper gives 4 sheets × 4 copies (2 across, 2 down).
  Set `binding.signature_pages = 8/16/32` to repeat a bigger signature instead. The copies still all
  face the same way, but inside a multi-fold signature some rows are heads-together, because that's
  how it folds.
- `layout.gang = true` puts different signatures/pieces on one sheet. Spare positions are filled
  with repeats (`layout.fill = "repeat"`) or left empty (`"blank"`). When a sheet carries several copies of
  a signature, the log tells you to run that sheet at 1/n of the quantity.
- `sheet.align = "gripper"` pushes the layout against the gripper instead of centring it.
- **Large format / roll:** `sheet.height = 0` makes the sheet as long as the job needs
  (`sheet.max_length` splits it across several sheets).

### Reassigning pages (`pages.map` or `--map`)

Run with `--plan` to see the automatic page order in map form, then copy it into the job file and edit:

```toml
[pages]
# map = [ signature, ... ]
#   signature = [front, back]                       one sheet
#            or [[front, back], [front, back], ...]  several sheets
#   front/back = page numbers by position, in reading order (top-left first, as the press sees that side)
#   0 = blank,  "3@180" = page 3 turned 180°
map = [ [ [4, 1], [2, 3] ] ]
```

If a sheet in the map has the same number of positions as the automatic layout, the pages are put
into that layout, keeping its signature gaps, rotation and so on. Otherwise a plain grid is used and
the log says so.

### Marks and colour bar

- **Trim marks** at every trim edge and **bleed marks** at every bleed edge. A mark starts
  `marks.offset` beyond the bleed and is cut short wherever it would cross **any** page's trim or
  bleed (including neighbours and the colour bar). Marks squeezed below `marks.min_length` are
  dropped. All marks are in Registration colour (`All`).
- **Bleed** never runs into a neighbour: when two pages are closer than 2 × bleed, the bleed stops
  halfway; at butted spine folds there is none.
- **Colour bar**: `colorbar.position` top/bottom/left/right, with space reserved so pages never
  overlap it. It uses the built-in CMYK patch bar or **imports a page from your own PDF**
  (`colorbar.file`), and **scales with the sheet**: `tile` (repeat at bar height along the full
  length), `stretch`, `fit` or `none`. `colorbar.length = 0` means the full printable length.
- **Slug line** (job name, sheet n/m, work style, signature, front/back) in the top margin.
- **Signature mark** (`[marks.signature]`): large bold text centred on the **left and right edges**
  of every plate, e.g. `1-A` / `1-B` for sheet 1 front/back, `2-A` / `2-B` for sheet 2. It runs along
  the edge in Registration colour so the operator and bindery can identify every printed sheet.
  - `text = "{sheet}-{side}"`. Tokens: `{sheet}`, `{sheets}`, `{sig}` (the signature numbers on the
    sheet, e.g. `2` or `3+4`) and `{side}`.
  - `front = "A"`, `back = "B"`. A work & turn/tumble plate uses both, e.g. `1-AB`.
  - `size` (pt), `left` / `right`, `rotate` (along the edge or horizontal).
  - **Position:** `edge_distance = 0.25` puts the text that far from the paper edge, on both sides.
    Without it, the text sits at the margin plus `inset`.
  - The log warns if the text would touch any page's bleed.

### Example jobs

```bash
# 16-page letter saddle-stitched book, 16pp signature, on a 38×25 sheet, sheetwise
sheet-impose book.pdf --set binding.style=saddle --set sheet.width=38 --set sheet.height=25

# 40-page 6×9 perfect-bound on a perfector, 40×28, 1/16" grind
sheet-impose book.pdf --set binding.style=perfect --set press.work_style=perfect \
  --set sheet.width=40 --set sheet.height=28 --set binding.grind=0.0625

# 2-sided business cards, gang on 19×13 with your colour bar along the left edge
sheet-impose cards.pdf --set colorbar.file=bars/cmyk.pdf --set colorbar.position=left

# posters on a 54" roll
sheet-impose posters.pdf --set sheet.width=54 --set sheet.height=0 --set press.work_style=single
```

## Run pdf-impose at login (optional)

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
    <string>/Users/leom1/Projects/PDFTooling/hotfolders.toml</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>/Users/leom1/Projects/PDFTooling/pdf-impose.log</string>
</dict></plist>
```

### Windows: start automatically at login

Simplest: press **Win+R**, type `shell:startup`, and put a shortcut there with
Target `C:\PDFTooling\target\release\pdf-impose.exe C:\PDFTooling\hotfolders.toml` and
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
