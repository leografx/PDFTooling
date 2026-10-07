//! Hot-folder configuration (`impose.toml`, lives inside the watched folder).
//!
//! Every field has a default, so a config file only needs the keys you want
//! to change. The file is re-read for every job, so edits apply immediately.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const CONFIG_FILE_NAME: &str = "impose.toml";

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
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// Keep every page exactly as it is in the PDF.
    Auto,
    /// Rotate landscape pages so they become portrait.
    Portrait,
    /// Rotate portrait pages so they become landscape.
    Landscape,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RotateDirection {
    Cw,
    Ccw,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    /// 1,1,2,2
    Grouped,
    /// 1,2,1,2
    Collated,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Remainder {
    /// Leftover slots get extra copies of the first pages (5 slots, 2 pages -> 1,1,1,2,2).
    Fill,
    /// Leftover slots stay empty (5 slots, 2 pages -> 1,1,2,2,_).
    Blank,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarkAnchor {
    /// Top of the mark lines up with the top of the trim box.
    TrimTop,
    /// Top of the mark lines up with the top of the bleed box.
    BleedTop,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AfterAction {
    Move,
    Delete,
    Keep,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub units: Units,
    pub sheet: Sheet,
    pub layout: Layout,
    pub bleed: Bleed,
    pub mark: Mark,
    pub folders: Folders,
    pub output: Output,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sheet {
    pub width: f64,
    /// 0 = height of the PDF bleed box (plus top/bottom margins).
    pub height: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub align: HAlign,
    pub valign: VAlign,
    /// Cut the sheet to the row (first bleed to last bleed) plus the eye mark.
    pub shrink_width_to_content: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    pub orientation: Orientation,
    pub rotate_direction: RotateDirection,
    /// Space between neighbouring bleed boxes. 0 = bleed touching bleed.
    pub gap: f64,
    /// Upper limit on copies across. 0 = as many as fit.
    pub max_across: usize,
    pub order: Order,
    pub remainder: Remainder,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bleed {
    /// Bleed added around the TrimBox when the PDF has no BleedBox. 0 = none.
    pub default_bleed: f64,
    /// When the PDF has no TrimBox: treat the outer (Crop/Media) box as bleed
    /// and inset it by this much to get the trim. 0 = trim = outer box.
    pub no_trimbox_inset: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mark {
    pub enabled: bool,
    pub width: f64,
    pub height: f64,
    /// Horizontal gap between the mark and the left bleed edge.
    pub offset_x: f64,
    /// Vertical shift from the anchor; positive moves the mark down.
    pub offset_y: f64,
    pub anchor: MarkAnchor,
    /// Subtract the mark area from `sheet.width` before fitting. When false the
    /// mark is added outside the row afterwards.
    pub reserve_space: bool,
    /// C, M, Y, K in percent.
    pub cmyk: [f64; 4],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Folders {
    /// Relative paths are relative to the hot folder.
    pub output: String,
    pub processed: String,
    pub error: String,
    /// What to do with the source PDF after a successful job.
    pub after_success: AfterAction,
    /// How long (ms) a file's size must stay unchanged before it is processed.
    pub stable_ms: u64,
    /// Safety rescan interval (ms) in case a filesystem event is missed.
    pub poll_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    /// Output file name. Tokens: {name} {across} {sheets} {pages}
    pub file_name: String,
    /// Compress the generated content streams.
    pub compress: bool,
    /// Write compact PDF 1.5 object/xref streams (smaller files).
    pub object_streams: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            units: Units::In,
            sheet: Sheet::default(),
            layout: Layout::default(),
            bleed: Bleed::default(),
            mark: Mark::default(),
            folders: Folders::default(),
            output: Output::default(),
        }
    }
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            width: 12.375,
            height: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            align: HAlign::Center,
            valign: VAlign::Center,
            shrink_width_to_content: false,
        }
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            orientation: Orientation::Auto,
            rotate_direction: RotateDirection::Cw,
            gap: 0.0,
            max_across: 0,
            order: Order::Grouped,
            remainder: Remainder::Fill,
        }
    }
}

impl Default for Bleed {
    fn default() -> Self {
        Self { default_bleed: 0.0, no_trimbox_inset: 0.0 }
    }
}

impl Default for Mark {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 0.25,
            height: 0.25,
            offset_x: 0.0,
            offset_y: 0.0,
            anchor: MarkAnchor::TrimTop,
            reserve_space: false,
            cmyk: [0.0, 0.0, 0.0, 100.0],
        }
    }
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            output: "../PDFOut".into(),
            processed: "Processed".into(),
            error: "Error".into(),
            after_success: AfterAction::Move,
            stable_ms: 1500,
            poll_ms: 3000,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Self {
            file_name: "{name}_{across}up.pdf".into(),
            compress: true,
            object_streams: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.sheet.width > 0.0, "sheet.width must be > 0");
        anyhow::ensure!(self.sheet.height >= 0.0, "sheet.height must be >= 0");
        anyhow::ensure!(self.layout.gap >= 0.0, "layout.gap must be >= 0");
        anyhow::ensure!(
            self.mark.width >= 0.0 && self.mark.height >= 0.0,
            "mark size must be >= 0"
        );
        for c in self.mark.cmyk {
            anyhow::ensure!((0.0..=100.0).contains(&c), "mark.cmyk values must be 0-100");
        }
        anyhow::ensure!(!self.output.file_name.trim().is_empty(), "output.file_name is empty");
        Ok(())
    }

    /// Convert a length in config units to PDF points.
    pub fn pt(&self, v: f64) -> f64 {
        self.units.to_pt(v)
    }
}

/// Default config written into a hot folder that has none.
pub const DEFAULT_CONFIG_TOML: &str = r#"# ------------------------------------------------------------------
# pdf-impose hot-folder settings
# This file is re-read for every PDF, so edits apply immediately.
# Any key you delete falls back to the default shown here.
# ------------------------------------------------------------------

# Units for every length in this file: "in", "mm" or "pt"
units = "in"

[sheet]
width  = 12.375        # sheet width; the row (first bleed -> last bleed) must fit inside
height = 0             # 0 = sheet height is the PDF bleed height
margin_left   = 0
margin_right  = 0
margin_top    = 0      # added above the bleed when height = 0
margin_bottom = 0      # added below the bleed when height = 0
align  = "center"      # left | center | right  row position (mark ignored), when shrink = false
valign = "center"      # top | center | bottom   (if pages differ in height)
# false = sheet stays sheet.width wide, row placed by `align`
# true  = sheet is cut to the row (first bleed -> last bleed) + the eye mark outside it
shrink_width_to_content = false

[layout]
orientation      = "auto"  # auto = as the PDF is | portrait | landscape
rotate_direction = "cw"    # cw | ccw  (used when orientation forces a turn)
gap        = 0             # space between bleed boxes (0 = bleed to bleed)
max_across = 0             # 0 = as many as fit, otherwise a cap
order      = "grouped"     # grouped = 1,1,2,2  | collated = 1,2,1,2
remainder  = "fill"        # fill = extra copies of first pages | blank = leave empty

[bleed]
default_bleed    = 0       # bleed added around TrimBox if the PDF has no BleedBox
no_trimbox_inset = 0       # if no TrimBox: trim = page box inset by this amount

[mark]                     # eye mark: left of the first copy, outside the bleed
enabled  = true
width    = 0.25
height   = 0.25
offset_x = 0               # gap between mark and the left bleed edge
offset_y = 0               # positive moves the mark down from the anchor
anchor   = "trim_top"      # trim_top | bleed_top
reserve_space = false      # false = mark is added outside the row after fitting
                           # true  = mark width is taken out of sheet.width first
cmyk     = [0, 0, 0, 100]  # C, M, Y, K percent

[folders]                  # relative paths are relative to this hot folder
output    = "../PDFOut"
processed = "Processed"
error     = "Error"
after_success = "move"     # move | delete | keep
stable_ms = 1500           # file size must be unchanged this long before processing
poll_ms   = 3000           # safety rescan interval

[output]
file_name = "{name}_{across}up.pdf"   # tokens: {name} {across} {sheets} {pages}
compress  = true
object_streams = true      # smaller PDF 1.5 files
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_matches_default_struct() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG_TOML).unwrap();
        let d = Config::default();
        assert_eq!(parsed.sheet.width, d.sheet.width);
        assert_eq!(parsed.mark.width, d.mark.width);
        assert_eq!(parsed.layout.order, d.layout.order);
        assert_eq!(parsed.folders.output, d.folders.output);
        parsed.validate().unwrap();
    }

    #[test]
    fn partial_config_ok() {
        let c: Config = toml::from_str("units = \"mm\"\n[sheet]\nwidth = 314.325\n").unwrap();
        assert!((c.pt(c.sheet.width) - 891.0).abs() < 0.01);
        assert!(c.mark.enabled);
    }
}
