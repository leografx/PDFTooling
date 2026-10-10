//! `job.toml` — everything that describes a sheet imposition job.
//! Every key has a default, so a job file only lists what differs.

use crate::row::config::AfterAction;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    In,
    Mm,
    Pt,
}

impl Units {
    pub fn to_pt(self, v: f64) -> f64 {
        match self {
            Units::In => v * 72.0,
            Units::Mm => v * 72.0 / 25.4,
            Units::Pt => v,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Units::In => "in",
            Units::Mm => "mm",
            Units::Pt => "pt",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkStyle {
    /// One side only.
    Single,
    /// Separate front and back plates, sheet turned left-to-right.
    Sheetwise,
    /// One plate holds front and back; sheet turned left-to-right, cut in half.
    Workturn,
    /// One plate holds front and back; sheet tumbled head-to-foot, cut in half.
    Worktumble,
    /// Perfecting press: both sides in one pass (back plate, tumble by default).
    Perfect,
}

impl WorkStyle {
    pub fn duplex(self) -> bool {
        self != WorkStyle::Single
    }
    pub fn name(self) -> &'static str {
        match self {
            WorkStyle::Single => "single",
            WorkStyle::Sheetwise => "sheetwise",
            WorkStyle::Workturn => "workturn",
            WorkStyle::Worktumble => "worktumble",
            WorkStyle::Perfect => "perfect",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Flip {
    /// Decide from the work style.
    Auto,
    /// Left-to-right (work & back, work & turn).
    Turn,
    /// Head-to-foot (work & tumble, most perfectors).
    Tumble,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Binding {
    /// Single pieces, step-and-repeat / gang.
    Flat,
    /// Single pieces ordered so cut stacks come out in sequence.
    Cutstack,
    /// Folded signatures nested inside each other, stitched on the spine.
    Saddle,
    /// Folded signatures gathered one after another, glued spine.
    Perfect,
}

impl Binding {
    pub fn folded(self) -> bool {
        matches!(self, Binding::Saddle | Binding::Perfect)
    }
    pub fn name(self) -> &'static str {
        match self {
            Binding::Flat => "flat",
            Binding::Cutstack => "cutstack",
            Binding::Saddle => "saddle",
            Binding::Perfect => "perfect",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CreepMethod {
    /// Move the page content toward the spine.
    Shift,
    /// Scale the page content down, anchored at the spine.
    Scale,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CreepDirection {
    /// Toward the spine (normal compensation).
    In,
    /// Away from the spine.
    Out,
}

/// Which half of a signature carries the lip (lap).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Folio {
    None,
    /// Lip on the low-folio half (lower page numbers).
    Low,
    /// Lip on the high-folio half (higher page numbers).
    High,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Center,
    Gripper,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlankPages {
    End,
    BeforeBackCover,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    Repeat,
    Blank,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BarScale {
    /// Scale to the bar height, repeat along the length.
    Tile,
    /// Stretch to exactly length × height.
    Stretch,
    /// Uniform scale to fit length × height, centred.
    Fit,
    /// Natural size, centred.
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Job {
    pub units: Units,
    pub sheet: Sheet,
    pub product: Product,
    pub press: Press,
    pub binding: BindingCfg,
    pub layout: Layout,
    pub pages: Pages,
    pub marks: Marks,
    pub colorbar: ColorBar,
    pub output: Output,
    /// Hot-folder handling (only used when the job file sits in a watched folder).
    pub folders: Folders,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Folders {
    /// Relative paths are relative to the hot folder.
    pub output: String,
    pub processed: String,
    pub error: String,
    pub after_success: AfterAction,
    /// How long (ms) a file's size must stay unchanged before it is processed.
    pub stable_ms: u64,
    /// Safety rescan interval (ms).
    pub poll_ms: u64,
}

impl Default for Folders {
    fn default() -> Self {
        Folders {
            output: "../PDFOut".into(),
            processed: "Processed".into(),
            error: "Error".into(),
            after_success: AfterAction::Move,
            stable_ms: 1500,
            poll_ms: 3000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sheet {
    pub width: f64,
    /// 0 = roll / large format: the sheet length grows to fit.
    pub height: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub gripper: f64,
    pub gripper_edge: Edge,
    pub align: Align,
    /// Roll only: split into several sheets above this length (0 = no limit).
    pub max_length: f64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Product {
    /// Finished size; omit to use the PDF TrimBox.
    pub trim_width: Option<f64>,
    pub trim_height: Option<f64>,
    /// Bleed to print; omit to use the PDF BleedBox (0.125 in if the PDF has none).
    pub bleed: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Press {
    pub work_style: WorkStyle,
    pub back_flip: Flip,
    /// Work & turn / tumble: write the plate a second time as the back page.
    pub output_back_plate: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BindingCfg {
    pub style: Binding,
    pub edge: Edge,
    /// 0 = auto; otherwise 4, 8, 16 or 32.
    pub signature_pages: usize,
    /// Creep: how far the innermost pages move (saddle: centre spread of the
    /// book; perfect: centre of each signature). 0 = use `paper_caliper`.
    pub creep: f64,
    /// Paper thickness; when `creep` is 0, creep = caliper × (folios − 1).
    pub paper_caliper: f64,
    pub creep_method: CreepMethod,
    pub creep_direction: CreepDirection,
    /// Perfect binding: extra space added each side of the spine (grind-off).
    pub grind: f64,
    /// Folded signatures: lip (lap) length for the stitcher/gatherer to open the signature.
    pub lip: f64,
    /// Which half gets the lip: none | low (low folio) | high (high folio).
    pub lip_side: Folio,
    pub blank_pages: BlankPages,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    /// Flat pieces: space between trims. Default 2 × bleed.
    pub gutter: Option<f64>,
    /// Signatures: space on head/face fold lines. Default 2 × bleed.
    pub fold_gap: Option<f64>,
    /// Between signatures / blocks. Default 2 × bleed.
    pub block_gap: Option<f64>,
    pub fill: Fill,
    /// Put different signatures / pieces on the same sheet.
    pub gang: bool,
    /// One folding sheet per signature: every sheet carries a single signature,
    /// repeated as many times as fit (folded jobs only).
    pub one_signature_per_sheet: bool,
    /// Turn the whole layout 90° if more fits.
    pub allow_rotation: bool,
    /// Turn some blocks 90° to squeeze more onto the sheet (always logged).
    pub mixed_rotation: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Pages {
    /// Manual page assignment, see README. Kept raw and parsed by `layout::parse_map`.
    pub map: Option<toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Marks {
    pub trim: bool,
    pub bleed: bool,
    /// Gap between the bleed edge and the start of every mark.
    pub offset: f64,
    pub length: f64,
    /// In points, whatever `units` says.
    pub line_width: f64,
    /// Marks shortened below this are dropped.
    pub min_length: f64,
    pub back: bool,
    pub slug: bool,
    /// Dashed marks at head and foot showing each page's crept trim position.
    pub creep: bool,
    /// Signature / side text mark at the left and right edges of the sheet ("1-A", "1-B" …).
    pub signature: SigMark,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SigMark {
    pub enabled: bool,
    /// Tokens: {sheet} {sheets} {sig} {side}
    pub text: String,
    /// Letter for the front side.
    pub front: String,
    /// Letter for the back side.
    pub back: String,
    /// Font size in points.
    pub size: f64,
    pub left: bool,
    pub right: bool,
    /// Run the text along the edge (true) or horizontally (false).
    pub rotate: bool,
    /// Distance in from the sheet margin (used when `edge_distance` is not set).
    pub inset: f64,
    /// Distance from the paper edge to the text (overrides margin + inset).
    pub edge_distance: Option<f64>,
}

impl Default for SigMark {
    fn default() -> Self {
        SigMark {
            enabled: true,
            text: "{sheet}-{side}".into(),
            front: "A".into(),
            back: "B".into(),
            size: 10.0,
            left: true,
            right: true,
            rotate: true,
            inset: 0.0,
            edge_distance: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorBar {
    pub enabled: bool,
    pub position: Edge,
    pub height: f64,
    /// Gap between the bar and the layout area.
    pub offset: f64,
    /// PDF to use as the bar; empty = built-in CMYK patches.
    pub file: String,
    pub page: u32,
    pub scale: BarScale,
    /// 0 = full printable length of the sheet.
    pub length: f64,
    pub back: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    /// Tokens: {name} {work} {binding} {sheets}
    pub file_name: String,
    pub object_streams: bool,
}

impl Default for Job {
    fn default() -> Self {
        Job {
            units: Units::In,
            sheet: Sheet::default(),
            product: Product::default(),
            press: Press::default(),
            binding: BindingCfg::default(),
            layout: Layout::default(),
            pages: Pages::default(),
            marks: Marks::default(),
            colorbar: ColorBar::default(),
            output: Output::default(),
            folders: Folders::default(),
        }
    }
}
impl Default for Sheet {
    fn default() -> Self {
        Sheet {
            width: 19.0,
            height: 13.0,
            margin_left: 0.25,
            margin_right: 0.25,
            margin_top: 0.25,
            margin_bottom: 0.25,
            gripper: 0.375,
            gripper_edge: Edge::Bottom,
            align: Align::Center,
            max_length: 0.0,
        }
    }
}
impl Default for Press {
    fn default() -> Self {
        Press { work_style: WorkStyle::Sheetwise, back_flip: Flip::Auto, output_back_plate: false }
    }
}
impl Default for BindingCfg {
    fn default() -> Self {
        BindingCfg {
            style: Binding::Flat,
            edge: Edge::Left,
            signature_pages: 0,
            creep: 0.0,
            paper_caliper: 0.0,
            creep_method: CreepMethod::Shift,
            creep_direction: CreepDirection::In,
            grind: 0.0,
            lip: 0.0,
            lip_side: Folio::None,
            blank_pages: BlankPages::End,
        }
    }
}
impl Default for Layout {
    fn default() -> Self {
        Layout {
            gutter: None,
            fold_gap: None,
            block_gap: None,
            fill: Fill::Repeat,
            gang: true,
            one_signature_per_sheet: false,
            allow_rotation: true,
            mixed_rotation: true,
        }
    }
}
impl Default for Marks {
    fn default() -> Self {
        Marks {
            trim: true,
            bleed: true,
            offset: 0.0625,
            length: 0.1875,
            line_width: 0.25,
            min_length: 0.0625,
            back: true,
            slug: true,
            creep: true,
            signature: SigMark::default(),
        }
    }
}
impl Default for ColorBar {
    fn default() -> Self {
        ColorBar {
            enabled: true,
            position: Edge::Top,
            height: 0.1875,
            offset: 0.0625,
            file: String::new(),
            page: 1,
            scale: BarScale::Tile,
            length: 0.0,
            back: true,
        }
    }
}
impl Default for Output {
    fn default() -> Self {
        Output { file_name: "{name}_{work}_{binding}.pdf".into(), object_streams: true }
    }
}

impl Job {
    pub fn pt(&self, v: f64) -> f64 {
        self.units.to_pt(v)
    }

    /// Load a job file, applying `--set key.path=value` overrides first.
    pub fn load(path: Option<&Path>, overrides: &[String]) -> Result<Job> {
        let mut value: toml::Table = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
                toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?
            }
            None => toml::Table::new(),
        };
        for ov in overrides {
            apply_override(&mut value, ov)?;
        }
        let job: Job = toml::Value::Table(value).try_into().context("invalid job settings")?;
        job.validate()?;
        Ok(job)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.sheet.width > 0.0, "sheet.width must be > 0");
        anyhow::ensure!(self.sheet.height >= 0.0, "sheet.height must be >= 0 (0 = roll)");
        if let Some(b) = self.product.bleed {
            anyhow::ensure!(b >= 0.0, "product.bleed must be >= 0");
        }
        anyhow::ensure!(
            matches!(self.binding.signature_pages, 0 | 4 | 8 | 16 | 32),
            "binding.signature_pages must be 0 (auto), 4, 8, 16 or 32"
        );
        anyhow::ensure!(
            !(self.press.work_style == WorkStyle::Single && self.binding.style.folded()),
            "a {} bound job can't be printed single sided — use sheetwise, perfect, workturn or worktumble",
            self.binding.style.name()
        );
        anyhow::ensure!(self.marks.line_width > 0.0, "marks.line_width must be > 0");
        anyhow::ensure!(self.marks.signature.size > 0.0, "marks.signature.size must be > 0");
        if let Some(d) = self.marks.signature.edge_distance {
            anyhow::ensure!(d >= 0.0, "marks.signature.edge_distance must be >= 0");
        }
        anyhow::ensure!(self.binding.lip >= 0.0, "binding.lip must be >= 0");
        anyhow::ensure!(self.binding.creep >= 0.0 && self.binding.paper_caliper >= 0.0, "binding.creep and binding.paper_caliper must be >= 0 (use creep_direction = \"out\" to reverse)");
        if self.binding.lip > 0.0 && self.binding.lip_side == Folio::None {
            log::warn!("binding.lip is set but binding.lip_side = \"none\" — no lip added (use \"low\" or \"high\")");
        }
        Ok(())
    }

    /// The flip used to reach the back side.
    pub fn flip(&self) -> Flip {
        match (self.press.back_flip, self.press.work_style) {
            (Flip::Auto, WorkStyle::Perfect | WorkStyle::Worktumble) => Flip::Tumble,
            (Flip::Auto, _) => Flip::Turn,
            (f, _) => f,
        }
    }
}

/// `a.b.c=value`; value parsed as TOML (number, bool, array, "string") or bare string.
fn apply_override(root: &mut toml::Table, ov: &str) -> Result<()> {
    let (key, raw) = ov.split_once('=').with_context(|| format!("--set needs key=value, got `{ov}`"))?;
    let value: toml::Value = toml::from_str::<toml::Table>(&format!("v = {raw}"))
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| toml::Value::String(raw.trim().to_string()));
    let parts: Vec<&str> = key.trim().split('.').collect();
    let mut table = root;
    for p in &parts[..parts.len() - 1] {
        table = table
            .entry(p.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .with_context(|| format!("`{p}` in `{key}` is not a section"))?;
    }
    table.insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

pub const DEFAULT_JOB_TOML: &str = r#"# ------------------------------------------------------------------
# sheet-impose job settings
# Every key is optional; anything left out uses the default shown here.
# Any key can also be overridden on the command line:
#   sheet-impose book.pdf -c job.toml --set binding.style=saddle --set sheet.width=28
# ------------------------------------------------------------------

units = "in"               # in | mm | pt   (marks.line_width is always in points)

[sheet]                    # press sheet or large-format media
width  = 19
height = 13                # 0 = roll: the length grows to fit the job
margin_left   = 0.25
margin_right  = 0.25
margin_top    = 0.25
margin_bottom = 0.25
gripper       = 0.375      # extra unprintable band on the gripper edge
gripper_edge  = "bottom"   # bottom | top | left | right
align         = "center"   # center | gripper  (push the layout against the gripper)
max_length    = 0          # roll only: start a new sheet beyond this length (0 = no limit)

[product]
# trim_width  = 8.5        # default: the PDF TrimBox
# trim_height = 11
# bleed       = 0.125      # default: the PDF BleedBox (0.125 in when the PDF has none)

[press]
work_style = "sheetwise"   # single | sheetwise | workturn | worktumble | perfect
back_flip  = "auto"        # auto | turn | tumble  (auto: tumble for perfect/worktumble, else turn)
output_back_plate = false  # workturn/worktumble: also write the plate again as the back page

[binding]
style = "flat"             # flat | cutstack | saddle | perfect
edge  = "left"             # left | right | top   (spine / binding edge)
signature_pages = 0        # 0 = auto (largest of 32/16/8/4 that fits) or 4 | 8 | 16 | 32
creep = 0                  # creep compensation for the innermost pages (saddle: centre spread,
                           #   perfect: centre of each signature); 0 = from paper_caliper
paper_caliper   = 0        # paper thickness, e.g. 0.004 — creep = caliper × (folios − 1) when creep = 0
creep_method    = "shift"  # shift (move content toward the spine) | scale (shrink toward the spine)
creep_direction = "in"     # in (toward the spine) | out
grind = 0                  # perfect: extra space each side of the spine for milling
lip      = 0               # lip (lap) so the stitcher can open the signature, e.g. 0.375
lip_side = "none"          # none | low (low folio: lip on the half with the lower pages) | high (high folio)
blank_pages = "end"        # end | before_back_cover   (where padding pages go)

[layout]
# gutter    = 0.25         # flat pieces, trim to trim   (default 2 × bleed)
# fold_gap  = 0.25         # signature head/face folds   (default 2 × bleed; spine is always 0)
# block_gap = 0.25         # between signatures/blocks   (default 2 × bleed)
fill   = "repeat"          # repeat | blank  — what goes in spare positions
gang   = true              # different signatures/pieces may share a sheet
one_signature_per_sheet = false   # true = 1 folding sheet per signature: each sheet carries one
                                  # single-fold folio (4pp), every copy upright, repeated head to foot
                                  # as many times as it fits (set binding.signature_pages for bigger)
allow_rotation = true      # turn the whole layout 90° if more fits
mixed_rotation = true      # turn some blocks 90° to fit more (always logged)

[pages]
# Manual page assignment — overrides the auto page order.
#   map = [ signature, ... ]
#   signature = [ front, back ]                one sheet
#            or [ [front, back], [front, back] ] several sheets
#   front/back = [ page, page, ... ]           slots in reading order (top-left first)
#   page = 3 | 0 (blank) | "3@180" (rotated)
# map = [ [ [4, 1], [2, 3] ] ]

[marks]
trim   = true
bleed  = true
offset = 0.0625            # gap between the bleed and the marks — marks never cross trim or bleed
length = 0.1875
line_width = 0.25          # points
min_length = 0.0625        # marks squeezed shorter than this are left out
back   = true              # marks on back plates too
slug   = true              # job / sheet / side info line in the margin
creep  = true              # dashed creep marks at head/foot where each crept page's face will trim

[marks.signature]          # text mark at the left & right edges, centred: 1-A, 1-B, 2-A, 2-B …
enabled = true
text    = "{sheet}-{side}" # tokens: {sheet} {sheets} {sig} (signature numbers on the sheet) {side}
front   = "A"              # {side} on the front plate (work & turn plates use front+back, e.g. "AB")
back    = "B"              # {side} on the back plate
size    = 10               # points
left    = true
right   = true
rotate  = true             # true = runs along the edge, false = horizontal
inset   = 0                # distance in from the sheet margin (when edge_distance is not set)
edge_distance = 0.25       # distance from the paper edge to the text; overrides margin + inset
                           # (delete this line to place the text by margin + inset instead)

[colorbar]
enabled  = true
position = "top"           # top | bottom | left | right
height   = 0.1875
offset   = 0.0625          # gap between the bar and the layout
file     = ""              # a PDF to use as the bar (empty = built-in CMYK bar)
page     = 1
scale    = "tile"          # tile | stretch | fit | none
length   = 0               # 0 = full printable length of the sheet (scales with the sheet)
back     = true

[output]
file_name = "{name}_{work}_{binding}.pdf"   # tokens: {name} {work} {binding} {sheets}
object_streams = true

[folders]                  # only used when this file is in a hot folder (relative to that folder)
output    = "../PDFOut"
processed = "Processed"
error     = "Error"
after_success = "move"     # move | delete | keep
stable_ms = 1500           # file size must be unchanged this long before processing
poll_ms   = 3000           # safety rescan interval
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_parses_to_defaults() {
        let j: Job = toml::from_str(DEFAULT_JOB_TOML).unwrap();
        j.validate().unwrap();
        assert_eq!(j.sheet.width, Job::default().sheet.width);
        assert_eq!(j.press.work_style, WorkStyle::Sheetwise);
        assert!(j.product.bleed.is_none());
    }

    #[test]
    fn overrides() {
        let j = Job::load(None, &["binding.style=saddle".into(), "sheet.width=28".into(), "pages.map=[[[4,1],[2,3]]]".into()]).unwrap();
        assert_eq!(j.binding.style, Binding::Saddle);
        assert_eq!(j.sheet.width, 28.0);
        assert!(j.pages.map.is_some());
    }

    #[test]
    fn single_sided_book_rejected() {
        assert!(Job::load(None, &["binding.style=saddle".into(), "press.work_style=single".into()]).is_err());
    }
}
