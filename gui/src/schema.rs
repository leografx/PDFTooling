//! Every setting of `job.toml` (sheet-impose) and `impose.toml` (pdf-impose):
//! its key, label, icon, hover help, type, default and choices.
//! Defaults mirror `src/sheet/config.rs` and `src/row/config.rs`
//! (a test checks them against the real structs).

use egui_phosphor::regular as I;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    /// sheet-impose `job.toml`
    Job,
    /// pdf-impose `impose.toml`
    Impose,
}

impl FileKind {
    pub fn file_name(self) -> &'static str {
        match self {
            FileKind::Job => "job.toml",
            FileKind::Impose => "impose.toml",
        }
    }
    pub fn tool(self) -> &'static str {
        match self {
            FileKind::Job => "sheet-impose",
            FileKind::Impose => "pdf-impose",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            FileKind::Job => "Sheet Job",
            FileKind::Impose => "Row Hot Folder",
        }
    }
    pub fn sections(self) -> &'static [Section] {
        match self {
            FileKind::Job => JOB,
            FileKind::Impose => IMPOSE,
        }
    }
    pub fn template(self) -> &'static str {
        match self {
            FileKind::Job => pdf_impose::sheet::config::DEFAULT_JOB_TOML,
            FileKind::Impose => pdf_impose::row::config::DEFAULT_CONFIG_TOML,
        }
    }
}

/// What a number means, for the suffix shown after it and the drag speed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// A length in the file's `units` (in / mm / pt).
    Len,
    /// Always points.
    Pt,
    Ms,
    Pct,
    Plain,
}

#[derive(Clone, Copy, Debug)]
pub struct Opt {
    pub value: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub help: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub enum Ty {
    /// Floating-point value.
    Num { def: f64, min: f64, max: f64, unit: Unit },
    /// Optional length: missing from the file = automatic.
    OptNum { auto: &'static str, sample: f64, min: f64, unit: Unit },
    /// Whole number.
    Int { def: i64, min: i64, max: i64, unit: Unit },
    /// Whole number picked from a fixed list.
    IntChoice { def: i64, opts: &'static [(i64, &'static str)] },
    Bool(bool),
    /// String from a fixed list. `seg` = show as a segmented control.
    Choice { def: &'static str, opts: &'static [Opt], seg: bool },
    Text { def: &'static str, tokens: &'static [&'static str] },
    /// Path to a PDF (with a Browse… button).
    PdfFile { def: &'static str },
    /// [C, M, Y, K] percentages.
    Cmyk([f64; 4]),
    /// sheet-impose `pages.map`, edited as TOML.
    PageMap,
}

/// Effective values of the open file (file value, else default), for `when` rules.
pub trait Look {
    fn s(&self, key: &str) -> String;
    fn n(&self, key: &str) -> f64;
    fn b(&self, key: &str) -> bool;
}

pub type Rule = fn(&dyn Look) -> bool;

#[derive(Clone, Copy)]
pub struct Field {
    /// Dotted TOML path, e.g. `marks.signature.size`.
    pub key: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub help: &'static str,
    pub ty: Ty,
    /// The setting only matters when this is true; otherwise the row is dimmed
    /// and the reason is shown on hover.
    pub when: Option<(Rule, &'static str)>,
}

#[derive(Clone, Copy)]
pub struct Group {
    pub title: &'static str,
    pub footer: &'static str,
    pub fields: &'static [Field],
}

#[derive(Clone, Copy)]
pub struct Section {
    pub title: &'static str,
    /// Filled icon drawn white on a coloured tile in the sidebar.
    pub icon: &'static str,
    pub color: [u8; 3],
    pub subtitle: &'static str,
    pub groups: &'static [Group],
}

impl Section {
    pub fn fields(&self) -> impl Iterator<Item = &'static Field> {
        self.groups.iter().flat_map(|g| g.fields.iter())
    }
}

pub fn find(kind: FileKind, key: &str) -> Option<&'static Field> {
    kind.sections().iter().flat_map(|s| s.fields()).find(|f| f.key == key)
}

const fn f(key: &'static str, label: &'static str, icon: &'static str, ty: Ty, help: &'static str) -> Field {
    Field { key, label, icon, help, ty, when: None }
}
const fn fw(key: &'static str, label: &'static str, icon: &'static str, ty: Ty, help: &'static str, rule: Rule, why: &'static str) -> Field {
    Field { key, label, icon, help, ty, when: Some((rule, why)) }
}
const fn o(value: &'static str, label: &'static str, icon: &'static str, help: &'static str) -> Opt {
    Opt { value, label, icon, help }
}
const fn len(def: f64) -> Ty {
    Ty::Num { def, min: 0.0, max: 10_000.0, unit: Unit::Len }
}
const fn ms(def: i64) -> Ty {
    Ty::Int { def, min: 0, max: 600_000, unit: Unit::Ms }
}
const fn pct(def: f64) -> Ty {
    Ty::Num { def, min: 0.0, max: 100.0, unit: Unit::Pct }
}

// ---------------------------------------------------------------- shared choices

const UNITS: &[Opt] = &[
    o("in", "Inches", I::RULER, "Every length in this file is in inches."),
    o("mm", "Millimetres", I::RULER, "Every length in this file is in millimetres."),
    o("pt", "Points", I::RULER, "Every length in this file is in PostScript points (72 per inch)."),
];

const EDGES4: &[Opt] = &[
    o("top", "Top", I::ARROW_LINE_UP, "The top edge of the sheet."),
    o("bottom", "Bottom", I::ARROW_LINE_DOWN, "The bottom edge of the sheet."),
    o("left", "Left", I::ARROW_LINE_LEFT, "The left edge of the sheet."),
    o("right", "Right", I::ARROW_LINE_RIGHT, "The right edge of the sheet."),
];

const AFTER: &[Opt] = &[
    o("move", "Move to Processed", I::FOLDER_SIMPLE_USER, "Move the source PDF into the Processed folder."),
    o("delete", "Delete", I::TRASH, "Delete the source PDF once the output is written."),
    o("keep", "Leave in place", I::HAND, "Leave the source PDF in the hot folder (it is remembered and not processed again)."),
];

const FOLDER_FIELDS: &[Field] = &[
    f("folders.output", "Output folder", I::FOLDER_SIMPLE_STAR, Ty::Text { def: "../PDFOut", tokens: &[] },
      "Where finished PDFs are written. A relative path is relative to the hot folder."),
    f("folders.processed", "Processed folder", I::FOLDER_SIMPLE_USER, Ty::Text { def: "Processed", tokens: &[] },
      "Where source PDFs go after a successful job (when “After success” is Move)."),
    f("folders.error", "Error folder", I::FOLDER_SIMPLE_MINUS, Ty::Text { def: "Error", tokens: &[] },
      "Where source PDFs go when a job fails. A <name>.error.txt file explains why."),
    f("folders.after_success", "After success", I::CHECK_CIRCLE, Ty::Choice { def: "move", opts: AFTER, seg: false },
      "What happens to the source PDF after its imposition is written."),
];
const FOLDER_TIMING: &[Field] = &[
    f("folders.stable_ms", "Wait for file to settle", I::HOURGLASS_MEDIUM, ms(1500),
      "A dropped file's size must stay unchanged this long before it is processed, so half-copied files are never imposed."),
    f("folders.poll_ms", "Rescan interval", I::ARROWS_CLOCKWISE, ms(3000),
      "Safety rescan of the hot folder, in case the operating system misses a file event."),
];
const FOLDER_NUMBERED: &[Field] = &[
    f("folders.subfolders", "Folders are jobs", I::FOLDERS, Ty::Bool(true),
      "A folder dropped into the hot folder is one job: its PDFs are joined in the order of their trailing number (_001, _01, _1 …) and the output is named after the folder."),
    f("folders.numbered_files", "Join numbered files", I::LIST_NUMBERS, Ty::Bool(false),
      "Loose PDFs named name_001.pdf, name_002.pdf … dropped straight into the hot folder are joined into one job called “name”."),
    fw("folders.group_settle_ms", "Wait for the last part", I::TIMER, ms(5000),
       "Numbered files: wait this long after the last part arrives before joining them, so a slow copy is not split into two jobs.",
       |l| l.b("folders.numbered_files"), "Only used when “Join numbered files” is on."),
];

const fn folders_section() -> Section {
    Section {
        title: "Hot Folder",
        icon: egui_phosphor::fill::FOLDER_OPEN,
        color: [0x1E, 0x90, 0xFF],
        subtitle: "Used when this file sits in a watched folder. Relative paths are relative to that folder.",
        groups: &[
            Group { title: "Folders", footer: "", fields: FOLDER_FIELDS },
            Group { title: "Timing", footer: "", fields: FOLDER_TIMING },
            Group {
                title: "Multi-file jobs",
                footer: "Files are ordered by number (_2 comes before _10). Gaps and duplicates are reported in the log.",
                fields: FOLDER_NUMBERED,
            },
        ],
    }
}

fn folded(l: &dyn Look) -> bool {
    matches!(l.s("binding.style").as_str(), "saddle" | "perfect")
}
fn duplex(l: &dyn Look) -> bool {
    l.s("press.work_style") != "single"
}
fn roll(l: &dyn Look) -> bool {
    l.n("sheet.height") == 0.0
}

// ================================================================ job.toml

pub static JOB: &[Section] = &[
    Section {
        title: "General",
        icon: egui_phosphor::fill::GEAR_SIX,
        color: [0x8E, 0x8E, 0x93],
        subtitle: "Units and the output file.",
        groups: &[
            Group {
                title: "Measurements",
                footer: "Lengths left at their default are plain numbers read in these units (sheet width 19 means 19 mm when units are mm).",
                fields: &[f("units", "Units", I::RULER, Ty::Choice { def: "in", opts: UNITS, seg: true },
                    "Unit for every length in this file. Line width and text size are always in points.")],
            },
            Group {
                title: "Output",
                footer: "",
                fields: &[
                    f("output.file_name", "File name", I::FILE_PDF, Ty::Text { def: "{name}_{work}_{binding}.pdf", tokens: &["{name}", "{work}", "{binding}", "{sheets}"] },
                      "Name of the imposed PDF. {name} = source file name, {work} = work style, {binding} = binding style, {sheets} = number of sheets."),
                    f("output.object_streams", "Compact PDF", I::FILE_ZIP, Ty::Bool(true),
                      "Write PDF 1.5 object streams: smaller files that every current RIP reads."),
                ],
            },
        ],
    },
    Section {
        title: "Sheet",
        icon: egui_phosphor::fill::FILE,
        color: [0x0A, 0x84, 0xFF],
        subtitle: "The press sheet or large-format media the job is laid out on.",
        groups: &[
            Group {
                title: "Size",
                footer: "Set the height to 0 for roll media: the sheet grows to fit the job.",
                fields: &[
                    f("sheet.width", "Width", I::ARROWS_HORIZONTAL, Ty::Num { def: 19.0, min: 0.0, max: 10_000.0, unit: Unit::Len },
                      "Width of the press sheet or roll."),
                    f("sheet.height", "Height", I::ARROWS_VERTICAL, len(13.0),
                      "Height of the press sheet. 0 = roll / large format: the length grows to fit."),
                    fw("sheet.max_length", "Max roll length", I::SCROLL, len(0.0),
                       "Roll media only: start a new sheet when the layout would run longer than this. 0 = no limit.",
                       roll, "Only for roll media (sheet height 0)."),
                ],
            },
            Group {
                title: "Margins",
                footer: "Nothing (marks included) is placed in the margins.",
                fields: &[
                    f("sheet.margin_top", "Top", I::ARROW_LINE_UP, len(0.25), "Unprintable space at the top of the sheet."),
                    f("sheet.margin_bottom", "Bottom", I::ARROW_LINE_DOWN, len(0.25), "Unprintable space at the bottom of the sheet."),
                    f("sheet.margin_left", "Left", I::ARROW_LINE_LEFT, len(0.25), "Unprintable space at the left of the sheet."),
                    f("sheet.margin_right", "Right", I::ARROW_LINE_RIGHT, len(0.25), "Unprintable space at the right of the sheet."),
                ],
            },
            Group {
                title: "Gripper",
                footer: "",
                fields: &[
                    f("sheet.gripper", "Gripper", I::HAND_GRABBING, len(0.375),
                      "Extra unprintable band where the press grippers hold the sheet, added to that edge's margin."),
                    f("sheet.gripper_edge", "Gripper edge", I::ALIGN_BOTTOM, Ty::Choice { def: "bottom", opts: EDGES4, seg: true },
                      "Which edge of the sheet goes into the press first."),
                    f("sheet.align", "Place layout", I::ALIGN_CENTER_VERTICAL, Ty::Choice { def: "center", seg: true, opts: &[
                        o("center", "Centred", I::ALIGN_CENTER_VERTICAL, "Centre the layout on the sheet."),
                        o("gripper", "Against gripper", I::ALIGN_BOTTOM, "Push the layout against the gripper edge."),
                    ] }, "Where the imposed layout sits on the sheet."),
                ],
            },
        ],
    },
    Section {
        title: "Product",
        icon: egui_phosphor::fill::CROP,
        color: [0xFF, 0x9F, 0x0A],
        subtitle: "Finished size and bleed. Leave on Automatic to read them from the PDF.",
        groups: &[Group {
            title: "Finished piece",
            footer: "Automatic uses the PDF TrimBox and BleedBox (0.125 in bleed when the PDF has no BleedBox).",
            fields: &[
                f("product.trim_width", "Trim width", I::ARROWS_HORIZONTAL, Ty::OptNum { auto: "From PDF TrimBox", sample: 8.5, min: 0.0, unit: Unit::Len },
                  "Finished width of one page. Turn off Automatic to override the PDF."),
                f("product.trim_height", "Trim height", I::ARROWS_VERTICAL, Ty::OptNum { auto: "From PDF TrimBox", sample: 11.0, min: 0.0, unit: Unit::Len },
                  "Finished height of one page. Turn off Automatic to override the PDF."),
                f("product.bleed", "Bleed", I::FRAME_CORNERS, Ty::OptNum { auto: "From PDF BleedBox", sample: 0.125, min: 0.0, unit: Unit::Len },
                  "How much artwork past the trim is printed on every side."),
            ],
        }],
    },
    Section {
        title: "Press",
        icon: egui_phosphor::fill::PRINTER,
        color: [0x5E, 0x5C, 0xE6],
        subtitle: "How the sheet goes through the press.",
        groups: &[Group {
            title: "Work style",
            footer: "",
            fields: &[
                f("press.work_style", "Work style", I::PRINTER, Ty::Choice { def: "sheetwise", seg: false, opts: &[
                    o("single", "Single sided", I::FILE, "One side only — no back."),
                    o("sheetwise", "Sheetwise (work & back)", I::COPY, "Separate front and back plates; the sheet is turned left to right."),
                    o("workturn", "Work & turn", I::ARROWS_LEFT_RIGHT, "One plate carries front and back; the sheet is turned left to right and cut in half."),
                    o("worktumble", "Work & tumble", I::ARROWS_DOWN_UP, "One plate carries front and back; the sheet is tumbled head to foot and cut in half."),
                    o("perfect", "Perfecting press", I::PRINTER, "Both sides printed in one pass (back plate tumbled by default)."),
                ] }, "Single, sheetwise, work & turn, work & tumble or perfector. Folded books need a two-sided work style."),
                fw("press.back_flip", "Back flip", I::ARROW_U_UP_LEFT, Ty::Choice { def: "auto", seg: true, opts: &[
                    o("auto", "Auto", I::MAGIC_WAND, "Tumble for perfecting and work & tumble, turn for everything else."),
                    o("turn", "Turn", I::ARROWS_LEFT_RIGHT, "Back side reached by turning the sheet left to right."),
                    o("tumble", "Tumble", I::ARROWS_DOWN_UP, "Back side reached by tumbling the sheet head to foot."),
                ] }, "How the sheet is flipped to print the back. Decides how back plates are mirrored.",
                   duplex, "Not used for single-sided work."),
                fw("press.output_back_plate", "Write back plate too", I::COPY_SIMPLE, Ty::Bool(false),
                   "Work & turn / tumble: also write the same plate again as the back page, so the PDF has a page for each side.",
                   |l| matches!(l.s("press.work_style").as_str(), "workturn" | "worktumble"), "Only for work & turn and work & tumble."),
            ],
        }],
    },
    Section {
        title: "Binding",
        icon: egui_phosphor::fill::BOOK_OPEN,
        color: [0xFF, 0x45, 0x3A],
        subtitle: "Flat work, cut & stack, saddle stitch or perfect bound.",
        groups: &[
            Group {
                title: "Binding",
                footer: "",
                fields: &[
                    f("binding.style", "Binding style", I::BOOKS, Ty::Choice { def: "flat", seg: false, opts: &[
                        o("flat", "Flat (step & repeat)", I::SQUARES_FOUR, "Single pieces repeated or ganged on the sheet."),
                        o("cutstack", "Cut & stack", I::STACK, "Single pieces ordered so that cut stacks come out in sequence."),
                        o("saddle", "Saddle stitch", I::BOOK_OPEN, "Folded signatures nested inside each other and stitched on the spine."),
                        o("perfect", "Perfect bound", I::BOOKS, "Folded signatures gathered one after another, spine milled and glued."),
                    ] }, "How the finished pieces are bound. Saddle and perfect produce folded signatures."),
                    fw("binding.edge", "Binding edge", I::ALIGN_LEFT, Ty::Choice { def: "left", seg: true, opts: &[
                        o("left", "Left", I::ARROW_LINE_LEFT, "Spine on the left (most books)."),
                        o("right", "Right", I::ARROW_LINE_RIGHT, "Spine on the right (right-to-left reading)."),
                        o("top", "Top", I::ARROW_LINE_UP, "Spine at the top (calendars, pads)."),
                        o("bottom", "Bottom", I::ARROW_LINE_DOWN, "Spine at the bottom."),
                    ] }, "Which edge of the finished page is the spine.", folded, "Only for saddle and perfect binding."),
                    fw("binding.signature_pages", "Pages per signature", I::BOOK_BOOKMARK, Ty::IntChoice { def: 0, opts: &[
                        (0, "Automatic"), (4, "4 pages"), (8, "8 pages"), (16, "16 pages"), (32, "32 pages"),
                    ] }, "Pages in each folded signature. Automatic picks the largest of 32/16/8/4 that fits the sheet.",
                       folded, "Only for saddle and perfect binding."),
                    fw("binding.blank_pages", "Blank pages go", I::FILE_DASHED, Ty::Choice { def: "end", seg: true, opts: &[
                        o("end", "At the end", I::ARROW_LINE_DOWN, "Padding pages are added after the last page."),
                        o("before_back_cover", "Before back cover", I::BOOK, "Padding pages go before the last page so the back cover stays outside."),
                    ] }, "Where padding pages go when the page count doesn't fill the last signature.",
                       folded, "Only for saddle and perfect binding."),
                ],
            },
            Group {
                title: "Creep",
                footer: "Creep grows linearly from the outside folio (0) to the innermost one. When Creep is 0 it is calculated: caliper × (folios − 1).",
                fields: &[
                    fw("binding.creep", "Creep", I::ARROWS_IN_LINE_HORIZONTAL, len(0.0),
                       "How far the innermost pages move (saddle: the centre spread of the book; perfect: the centre of each signature). 0 = calculate from paper caliper.",
                       folded, "Only for saddle and perfect binding."),
                    fw("binding.paper_caliper", "Paper caliper", I::STACK_SIMPLE, Ty::Num { def: 0.0, min: 0.0, max: 100.0, unit: Unit::Len },
                       "Thickness of one sheet of paper, e.g. 0.004 in. Used to calculate creep when Creep is 0.",
                       folded, "Only for saddle and perfect binding."),
                    fw("binding.creep_method", "Method", I::ARROWS_OUT_LINE_HORIZONTAL, Ty::Choice { def: "shift", seg: true, opts: &[
                        o("shift", "Shift", I::ARROW_LEFT, "Move the page content toward the spine."),
                        o("scale", "Scale", I::ARROWS_IN, "Shrink the page content, anchored at the spine."),
                    ] }, "How the content is compensated for creep.", folded, "Only for saddle and perfect binding."),
                    fw("binding.creep_direction", "Direction", I::ARROWS_LEFT_RIGHT, Ty::Choice { def: "in", seg: true, opts: &[
                        o("in", "Toward spine", I::ARROW_LINE_LEFT, "Normal compensation: inner pages move toward the spine."),
                        o("out", "Away from spine", I::ARROW_LINE_RIGHT, "Reverse compensation."),
                    ] }, "Which way crept pages move.", folded, "Only for saddle and perfect binding."),
                ],
            },
            Group {
                title: "Lip & grind",
                footer: "",
                fields: &[
                    fw("binding.lip", "Lip (lap)", I::BOOKMARK_SIMPLE, len(0.0),
                       "Extra paper on one half of each signature so the stitcher or gatherer can open it, e.g. 0.375 in.",
                       folded, "Only for saddle and perfect binding."),
                    fw("binding.lip_side", "Lip on", I::SPLIT_HORIZONTAL, Ty::Choice { def: "none", seg: true, opts: &[
                        o("none", "None", I::PROHIBIT, "No lip."),
                        o("low", "Low folio", I::SORT_ASCENDING, "Lip on the half that carries the lower page numbers."),
                        o("high", "High folio", I::SORT_DESCENDING, "Lip on the half that carries the higher page numbers."),
                    ] }, "Which half of the signature carries the lip. Must be Low or High for the lip to be added.",
                       folded, "Only for saddle and perfect binding."),
                    fw("binding.grind", "Grind-off", I::WAVE_SAWTOOTH, len(0.0),
                       "Perfect binding: extra space added each side of the spine, milled away before gluing.",
                       |l| l.s("binding.style") == "perfect", "Only for perfect binding."),
                ],
            },
        ],
    },
    Section {
        title: "Layout",
        icon: egui_phosphor::fill::SQUARES_FOUR,
        color: [0x30, 0xD1, 0x58],
        subtitle: "Spacing, filling and rotation on the sheet.",
        groups: &[
            Group {
                title: "Spacing",
                footer: "Automatic = twice the bleed. The spine fold of a signature is always 0.",
                fields: &[
                    f("layout.gutter", "Gutter", I::COLUMNS, Ty::OptNum { auto: "2 × bleed", sample: 0.25, min: 0.0, unit: Unit::Len },
                      "Flat and cut & stack work: space between neighbouring trims."),
                    f("layout.fold_gap", "Fold gap", I::ROWS, Ty::OptNum { auto: "2 × bleed", sample: 0.25, min: 0.0, unit: Unit::Len },
                      "Signatures: space on the head and face fold lines."),
                    f("layout.block_gap", "Block gap", I::DOTS_NINE, Ty::OptNum { auto: "2 × bleed", sample: 0.25, min: 0.0, unit: Unit::Len },
                      "Space between whole signatures or blocks of pieces on the sheet."),
                ],
            },
            Group {
                title: "Filling the sheet",
                footer: "",
                fields: &[
                    f("layout.fill", "Spare positions", I::SQUARES_FOUR, Ty::Choice { def: "repeat", seg: true, opts: &[
                        o("repeat", "Repeat", I::REPEAT, "Fill spare positions with more copies."),
                        o("blank", "Leave blank", I::SQUARE, "Leave spare positions empty."),
                    ] }, "What goes into positions left over on the sheet."),
                    f("layout.gang", "Gang up", I::INTERSECT_SQUARE, Ty::Bool(true),
                      "Allow different signatures or pieces to share a sheet."),
                    fw("layout.one_signature_per_sheet", "One signature per sheet", I::BOOK_OPEN_TEXT, Ty::Bool(false),
                       "Each sheet carries a single signature, repeated head to foot as many times as it fits, every copy facing the same way. Uses 4-page folios unless Pages per signature is set.",
                       folded, "Only for saddle and perfect binding."),
                ],
            },
            Group {
                title: "Rotation",
                footer: "Every rotation is reported in the log.",
                fields: &[
                    f("layout.allow_rotation", "Rotate whole layout", I::ARROW_CLOCKWISE, Ty::Bool(true),
                      "Turn the whole layout 90° when more fits that way."),
                    f("layout.mixed_rotation", "Mix rotations", I::ARROWS_CLOCKWISE, Ty::Bool(true),
                      "Turn some blocks 90° to squeeze more onto the sheet. Ignored in one-signature-per-sheet mode."),
                ],
            },
        ],
    },
    Section {
        title: "Page Map",
        icon: egui_phosphor::fill::LIST_NUMBERS,
        color: [0x40, 0xC8, 0xE0],
        subtitle: "Manual page assignment. Overrides the automatic page order.",
        groups: &[Group {
            title: "Manual assignment",
            footer: "map = [ signature, … ]   signature = [ front, back ]   front/back = [ page, … ] in reading order (top-left first).  page = 3, 0 for blank, or \"3@180\" to rotate.",
            fields: &[f("pages.map", "Page map", I::GRID_NINE, Ty::PageMap,
                "Which page goes in which position. Example for one 4-page sheet: [ [ [4, 1], [2, 3] ] ]. Turn off Automatic to type a map.")],
        }],
    },
    Section {
        title: "Marks",
        icon: egui_phosphor::fill::CROSSHAIR,
        color: [0xFF, 0x37, 0x5F],
        subtitle: "Trim, bleed and creep marks. Marks never cross a trim or bleed edge.",
        groups: &[
            Group {
                title: "Printer’s marks",
                footer: "",
                fields: &[
                    f("marks.trim", "Trim marks", I::SCISSORS, Ty::Bool(true), "Crop marks at every trim corner, outside the bleed."),
                    f("marks.bleed", "Bleed marks", I::FRAME_CORNERS, Ty::Bool(true), "Marks showing where the bleed ends."),
                    fw("marks.creep", "Creep marks", I::ARROWS_IN_LINE_HORIZONTAL, Ty::Bool(true),
                       "Dashed marks at head and foot showing where each crept page will be trimmed.",
                       folded, "Only for saddle and perfect binding."),
                    f("marks.slug", "Slug line", I::INFO, Ty::Bool(true), "Job, sheet and side information printed in the margin."),
                    fw("marks.back", "Marks on back", I::COPY, Ty::Bool(true), "Also draw marks on back plates.", duplex, "Not used for single-sided work."),
                ],
            },
            Group {
                title: "Mark size",
                footer: "",
                fields: &[
                    f("marks.offset", "Offset", I::ARROWS_OUT_SIMPLE, len(0.0625), "Gap between the bleed edge and the start of every mark."),
                    f("marks.length", "Length", I::LINE_SEGMENT, len(0.1875), "Length of a mark."),
                    f("marks.line_width", "Line width", I::PEN_NIB, Ty::Num { def: 0.25, min: 0.01, max: 10.0, unit: Unit::Pt },
                      "Stroke width of the marks, always in points."),
                    f("marks.min_length", "Shortest mark", I::ARROWS_IN_SIMPLE, len(0.0625),
                      "Marks squeezed shorter than this (by a neighbouring bleed) are left out."),
                ],
            },
        ],
    },
    Section {
        title: "Signature Text",
        icon: egui_phosphor::fill::TEXT_AA,
        color: [0xBF, 0x5A, 0xF2],
        subtitle: "1-A, 1-B, 2-A … at the left and right edges of each sheet, centred.",
        groups: &[
            Group {
                title: "Text",
                footer: "",
                fields: &[
                    f("marks.signature.enabled", "Show signature text", I::TEXT_T, Ty::Bool(true), "Print the sheet / side label on the sheet edges."),
                    f("marks.signature.text", "Text", I::TEXT_AA, Ty::Text { def: "{sheet}-{side}", tokens: &["{sheet}", "{sheets}", "{sig}", "{side}"] },
                      "Label text. {sheet} = sheet number, {sheets} = total sheets, {sig} = signature numbers on the sheet, {side} = the front/back letter."),
                    f("marks.signature.front", "Front letter", I::NUMBER_SQUARE_ONE, Ty::Text { def: "A", tokens: &[] }, "{side} on the front plate."),
                    fw("marks.signature.back", "Back letter", I::NUMBER_SQUARE_TWO, Ty::Text { def: "B", tokens: &[] }, "{side} on the back plate.",
                       duplex, "Not used for single-sided work."),
                    f("marks.signature.size", "Text size", I::TEXT_H, Ty::Num { def: 10.0, min: 1.0, max: 200.0, unit: Unit::Pt }, "Font size in points."),
                ],
            },
            Group {
                title: "Placement",
                footer: "Distance from edge measures from the paper edge; turn it to Automatic to place the text by margin + inset instead.",
                fields: &[
                    f("marks.signature.left", "Left edge", I::ALIGN_LEFT, Ty::Bool(true), "Print the label at the left edge of the sheet."),
                    f("marks.signature.right", "Right edge", I::ALIGN_RIGHT, Ty::Bool(true), "Print the label at the right edge of the sheet."),
                    f("marks.signature.rotate", "Run along edge", I::TEXT_ALIGN_JUSTIFY, Ty::Bool(true), "On: text runs along the sheet edge (rotated 90°). Off: horizontal."),
                    f("marks.signature.edge_distance", "Distance from edge", I::ARROW_LINE_RIGHT, Ty::OptNum { auto: "Margin + inset", sample: 0.25, min: 0.0, unit: Unit::Len },
                      "Distance from the paper edge to the text. Overrides margin + inset."),
                    f("marks.signature.inset", "Inset", I::ARROWS_IN_SIMPLE, len(0.0), "Distance in from the sheet margin (used when Distance from edge is Automatic)."),
                ],
            },
        ],
    },
    Section {
        title: "Colour Bar",
        icon: egui_phosphor::fill::PALETTE,
        color: [0xFF, 0x2D, 0x55],
        subtitle: "Built-in CMYK patches, or your own bar imported from a PDF.",
        groups: &[
            Group {
                title: "Colour bar",
                footer: "",
                fields: &[
                    f("colorbar.enabled", "Show colour bar", I::PALETTE, Ty::Bool(true), "Print a colour bar on the sheet."),
                    f("colorbar.position", "Position", I::ALIGN_TOP, Ty::Choice { def: "top", opts: EDGES4, seg: true }, "Which edge of the layout the bar runs along."),
                    f("colorbar.back", "On back too", I::COPY, Ty::Bool(true), "Also print the bar on back plates."),
                ],
            },
            Group {
                title: "Size",
                footer: "Length 0 = the full printable length, so the bar scales with the sheet.",
                fields: &[
                    f("colorbar.height", "Height", I::ARROWS_VERTICAL, len(0.1875), "Height (thickness) of the bar."),
                    f("colorbar.length", "Length", I::ARROWS_HORIZONTAL, len(0.0), "Length of the bar. 0 = full printable length of the sheet."),
                    f("colorbar.offset", "Offset", I::ARROWS_OUT_SIMPLE, len(0.0625), "Gap between the bar and the layout."),
                ],
            },
            Group {
                title: "Custom bar",
                footer: "Leave the file empty to use the built-in CMYK bar.",
                fields: &[
                    f("colorbar.file", "Bar PDF", I::FILE_PDF, Ty::PdfFile { def: "" },
                      "A PDF to use as the colour bar. A relative path is relative to this settings file."),
                    fw("colorbar.page", "Page", I::FILE, Ty::Int { def: 1, min: 1, max: 9999, unit: Unit::Plain }, "Which page of the bar PDF to use.",
                       |l| !l.s("colorbar.file").is_empty(), "Only used with a custom bar PDF."),
                    fw("colorbar.scale", "Scaling", I::ARROWS_OUT, Ty::Choice { def: "tile", seg: false, opts: &[
                        o("tile", "Tile", I::DOTS_THREE, "Scale to the bar height and repeat along the length."),
                        o("stretch", "Stretch", I::ARROWS_OUT_LINE_HORIZONTAL, "Stretch to exactly length × height."),
                        o("fit", "Fit", I::CORNERS_IN, "Scale uniformly to fit length × height, centred."),
                        o("none", "Natural size", I::SQUARE, "Use the PDF's own size, centred."),
                    ] }, "How the bar PDF is sized to the bar area.", |l| !l.s("colorbar.file").is_empty(), "Only used with a custom bar PDF."),
                ],
            },
        ],
    },
    folders_section(),
];

// ================================================================ impose.toml

fn spot_on(l: &dyn Look) -> bool {
    l.b("mark.enabled") && l.b("mark.spot.enabled")
}
fn under_on(l: &dyn Look) -> bool {
    l.b("mark.enabled") && l.b("mark.underlay.enabled")
}
fn mark_on(l: &dyn Look) -> bool {
    l.b("mark.enabled")
}

pub static IMPOSE: &[Section] = &[
    Section {
        title: "General",
        icon: egui_phosphor::fill::GEAR_SIX,
        color: [0x8E, 0x8E, 0x93],
        subtitle: "Units and the output file.",
        groups: &[
            Group {
                title: "Measurements",
                footer: "Lengths left at their default are plain numbers read in these units.",
                fields: &[f("units", "Units", I::RULER, Ty::Choice { def: "in", opts: UNITS, seg: true }, "Unit for every length in this file.")],
            },
            Group {
                title: "Output",
                footer: "",
                fields: &[
                    f("output.file_name", "File name", I::FILE_PDF, Ty::Text { def: "{name}_{across}up.pdf", tokens: &["{name}", "{across}", "{sheets}", "{pages}"] },
                      "Name of the imposed PDF. {name} = source file name, {across} = copies across, {sheets} = sheets written, {pages} = pages in the source."),
                    f("output.compress", "Compress", I::FILE_ZIP, Ty::Bool(true), "Compress the generated page content."),
                    f("output.object_streams", "Compact PDF", I::PACKAGE, Ty::Bool(true), "Write PDF 1.5 object streams for smaller files."),
                ],
            },
        ],
    },
    Section {
        title: "Sheet",
        icon: egui_phosphor::fill::FILE,
        color: [0x0A, 0x84, 0xFF],
        subtitle: "The row is laid out bleed to bleed inside the sheet width.",
        groups: &[
            Group {
                title: "Size",
                footer: "Height 0 = the sheet is as tall as the PDF bleed box (plus top/bottom margins).",
                fields: &[
                    f("sheet.width", "Width", I::ARROWS_HORIZONTAL, Ty::Num { def: 12.375, min: 0.0, max: 10_000.0, unit: Unit::Len },
                      "Sheet width. The row, from the first bleed to the last, must fit inside."),
                    f("sheet.height", "Height", I::ARROWS_VERTICAL, len(0.0), "Sheet height. 0 = the PDF bleed height."),
                    f("sheet.shrink_width_to_content", "Shrink to row", I::ARROWS_IN_LINE_HORIZONTAL, Ty::Bool(false),
                      "On: the sheet is cut to the row (first bleed to last bleed) plus the eye mark. Off: the sheet stays the full width and the row is placed by Alignment."),
                ],
            },
            Group {
                title: "Margins",
                footer: "",
                fields: &[
                    f("sheet.margin_top", "Top", I::ARROW_LINE_UP, len(0.0), "Space above the row (added to the height when Height is 0)."),
                    f("sheet.margin_bottom", "Bottom", I::ARROW_LINE_DOWN, len(0.0), "Space below the row (added to the height when Height is 0)."),
                    f("sheet.margin_left", "Left", I::ARROW_LINE_LEFT, len(0.0), "Space at the left of the sheet."),
                    f("sheet.margin_right", "Right", I::ARROW_LINE_RIGHT, len(0.0), "Space at the right of the sheet."),
                ],
            },
            Group {
                title: "Alignment",
                footer: "The eye mark is ignored when centring.",
                fields: &[
                    fw("sheet.align", "Horizontal", I::ALIGN_CENTER_HORIZONTAL, Ty::Choice { def: "center", seg: true, opts: &[
                        o("left", "Left", I::ALIGN_LEFT, "Row against the left margin."),
                        o("center", "Centre", I::ALIGN_CENTER_HORIZONTAL, "Row centred on the sheet."),
                        o("right", "Right", I::ALIGN_RIGHT, "Row against the right margin."),
                    ] }, "Where the row sits across the sheet.",
                       |l| !l.b("sheet.shrink_width_to_content"), "Not used when Shrink to row is on."),
                    f("sheet.valign", "Vertical", I::ALIGN_CENTER_VERTICAL, Ty::Choice { def: "center", seg: true, opts: &[
                        o("top", "Top", I::ALIGN_TOP, "Align page tops."),
                        o("center", "Centre", I::ALIGN_CENTER_VERTICAL, "Centre pages vertically."),
                        o("bottom", "Bottom", I::ALIGN_BOTTOM, "Align page bottoms."),
                    ] }, "Vertical position when pages differ in height."),
                ],
            },
        ],
    },
    Section {
        title: "Layout",
        icon: egui_phosphor::fill::COLUMNS,
        color: [0x30, 0xD1, 0x58],
        subtitle: "How copies and pages fill the row.",
        groups: &[
            Group {
                title: "Orientation",
                footer: "",
                fields: &[
                    f("layout.orientation", "Orientation", I::DEVICE_ROTATE, Ty::Choice { def: "auto", seg: true, opts: &[
                        o("auto", "As in PDF", I::MAGIC_WAND, "Keep every page exactly as it is in the PDF."),
                        o("portrait", "Portrait", I::DEVICE_MOBILE, "Turn landscape pages so they become portrait."),
                        o("landscape", "Landscape", I::DEVICE_MOBILE_SPEAKER, "Turn portrait pages so they become landscape."),
                    ] }, "Page orientation in the row."),
                    fw("layout.rotate_direction", "Turn", I::ARROW_CLOCKWISE, Ty::Choice { def: "cw", seg: true, opts: &[
                        o("cw", "Clockwise", I::ARROW_CLOCKWISE, "Turn pages 90° clockwise."),
                        o("ccw", "Counter-clockwise", I::ARROW_COUNTER_CLOCKWISE, "Turn pages 90° counter-clockwise."),
                    ] }, "Which way pages are turned when Orientation forces a turn.",
                       |l| l.s("layout.orientation") != "auto", "Only used when Orientation is Portrait or Landscape."),
                ],
            },
            Group {
                title: "Copies",
                footer: "Example: 4 fit across and the PDF has 2 pages — Grouped gives 1,1,2,2 and Collated gives 1,2,1,2.",
                fields: &[
                    f("layout.gap", "Gap", I::COLUMNS, len(0.0), "Space between neighbouring bleed boxes. 0 = bleed touching bleed."),
                    f("layout.max_across", "Max across", I::DOTS_THREE, Ty::Int { def: 0, min: 0, max: 999, unit: Unit::Plain },
                      "Upper limit on copies across the row. 0 = as many as fit."),
                    f("layout.order", "Order", I::SORT_ASCENDING, Ty::Choice { def: "grouped", seg: true, opts: &[
                        o("grouped", "Grouped 1,1,2,2", I::ROWS, "Copies of each page side by side."),
                        o("collated", "Collated 1,2,1,2", I::REPEAT, "Pages repeat as a set."),
                    ] }, "How the pages of a multi-page PDF are distributed along the row."),
                    f("layout.remainder", "Leftover slots", I::SQUARE_HALF, Ty::Choice { def: "fill", seg: true, opts: &[
                        o("fill", "Fill", I::PLUS_SQUARE, "Leftover slots get extra copies of the first pages (5 slots, 2 pages → 1,1,1,2,2)."),
                        o("blank", "Blank", I::SQUARE, "Leftover slots stay empty (5 slots, 2 pages → 1,1,2,2,_)."),
                    ] }, "What happens when the slots don't divide evenly between the pages."),
                ],
            },
        ],
    },
    Section {
        title: "Bleed",
        icon: egui_phosphor::fill::FRAME_CORNERS,
        color: [0xFF, 0x9F, 0x0A],
        subtitle: "Fallbacks for PDFs without proper boxes.",
        groups: &[Group {
            title: "Missing boxes",
            footer: "",
            fields: &[
                f("bleed.default_bleed", "Default bleed", I::FRAME_CORNERS, len(0.0),
                  "Bleed added around the TrimBox when the PDF has no BleedBox. 0 = none."),
                f("bleed.no_trimbox_inset", "Trim inset", I::CROP, len(0.0),
                  "When the PDF has no TrimBox: the page box is treated as the bleed and inset by this much to get the trim. 0 = trim is the page box."),
            ],
        }],
    },
    Section {
        title: "Eye Mark",
        icon: egui_phosphor::fill::EYE,
        color: [0x48, 0x48, 0x4A],
        subtitle: "Placed outside the left bleed, top aligned to the trim.",
        groups: &[
            Group {
                title: "Eye mark",
                footer: "",
                fields: &[
                    f("mark.enabled", "Show eye mark", I::EYE, Ty::Bool(true), "Draw the eye mark left of the first copy, outside its bleed."),
                    fw("mark.width", "Width", I::ARROWS_HORIZONTAL, len(0.25), "Width of the eye mark.", mark_on, "The eye mark is off."),
                    fw("mark.height", "Height", I::ARROWS_VERTICAL, len(0.25), "Height of the eye mark.", mark_on, "The eye mark is off."),
                    fw("mark.cmyk", "Colour", I::DROP, Ty::Cmyk([0.0, 0.0, 0.0, 100.0]), "Process colour of the eye mark, in C M Y K percent.", mark_on, "The eye mark is off."),
                    fw("mark.overprint", "Overprint", I::STACK, Ty::Bool(true), "The eye mark overprints (doesn't knock out the white underlay).", mark_on, "The eye mark is off."),
                ],
            },
            Group {
                title: "Position",
                footer: "",
                fields: &[
                    fw("mark.anchor", "Align top to", I::ALIGN_TOP, Ty::Choice { def: "trim_top", seg: true, opts: &[
                        o("trim_top", "Trim top", I::CROP, "Top of the mark lines up with the top of the trim box."),
                        o("bleed_top", "Bleed top", I::FRAME_CORNERS, "Top of the mark lines up with the top of the bleed box."),
                    ] }, "What the top of the mark lines up with.", mark_on, "The eye mark is off."),
                    fw("mark.offset_x", "Gap to bleed", I::ARROWS_OUT_LINE_HORIZONTAL, len(0.0), "Horizontal gap between the mark and the left bleed edge.", mark_on, "The eye mark is off."),
                    fw("mark.offset_y", "Move down", I::ARROW_DOWN, Ty::Num { def: 0.0, min: -10_000.0, max: 10_000.0, unit: Unit::Len },
                       "Vertical shift from the anchor; positive moves the mark down.", mark_on, "The eye mark is off."),
                    fw("mark.reserve_space", "Reserve space", I::ARROWS_IN_LINE_HORIZONTAL, Ty::Bool(false),
                       "On: the mark's width is taken out of the sheet width before fitting copies. Off: the mark is added outside the row afterwards.",
                       mark_on, "The eye mark is off."),
                ],
            },
        ],
    },
    Section {
        title: "Die Spot",
        icon: egui_phosphor::fill::SCISSORS,
        color: [0xFF, 0x00, 0xA0],
        subtitle: "A spot-colour copy of the eye mark, in place on top of it.",
        groups: &[Group {
            title: "Spot duplicate",
            footer: "Spot colour names are case-sensitive: “die” and “Die” are different plates.",
            fields: &[
                fw("mark.spot.enabled", "Duplicate as spot", I::COPY, Ty::Bool(true), "Add a spot-colour copy of the eye mark.", mark_on, "The eye mark is off."),
                fw("mark.spot.name", "Spot name", I::TAG, Ty::Text { def: "die", tokens: &[] }, "Separation (spot colour) name. Case-sensitive.", spot_on, "The spot duplicate is off."),
                fw("mark.spot.tint", "Tint", I::DROP_HALF, pct(100.0), "Tint of the spot colour.", spot_on, "The spot duplicate is off."),
                fw("mark.spot.cmyk", "Screen colour", I::PALETTE, Ty::Cmyk([0.0, 100.0, 0.0, 0.0]),
                   "How the spot looks on screen and in composite proofs (magenta by default).", spot_on, "The spot duplicate is off."),
                fw("mark.spot.overprint", "Overprint", I::STACK, Ty::Bool(true), "The spot copy overprints (OPM 1).", spot_on, "The spot duplicate is off."),
            ],
        }],
    },
    Section {
        title: "White Underlay",
        icon: egui_phosphor::fill::RECTANGLE,
        color: [0xA2, 0x84, 0x5E],
        subtitle: "A spot-colour band behind the eye mark, as wide as the mark.",
        groups: &[Group {
            title: "Underlay",
            footer: "",
            fields: &[
                fw("mark.underlay.enabled", "Show underlay", I::RECTANGLE, Ty::Bool(true), "Add the spot-colour band behind the eye mark.", mark_on, "The eye mark is off."),
                fw("mark.underlay.name", "Spot name", I::TAG, Ty::Text { def: "white", tokens: &[] }, "Separation (spot colour) name. Case-sensitive.", under_on, "The underlay is off."),
                fw("mark.underlay.extent", "Height", I::ARROWS_VERTICAL, Ty::Choice { def: "page", seg: true, opts: &[
                    o("page", "Whole page", I::ARROWS_VERTICAL, "From the top edge to the bottom edge of the sheet."),
                    o("trim", "Trim only", I::CROP, "From trim top to trim bottom of the first copy."),
                ] }, "How tall the underlay is.", under_on, "The underlay is off."),
                fw("mark.underlay.tint", "Tint", I::DROP_HALF, pct(100.0), "Tint of the spot colour.", under_on, "The underlay is off."),
                fw("mark.underlay.cmyk", "Screen colour", I::PALETTE, Ty::Cmyk([0.0, 0.0, 0.0, 10.0]),
                   "How the spot looks on screen and in composite proofs (10% black by default).", under_on, "The underlay is off."),
                fw("mark.underlay.overprint", "Overprint", I::STACK, Ty::Bool(false), "The underlay overprints.", under_on, "The underlay is off."),
            ],
        }],
    },
    folders_section(),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key the schema writes must be accepted by the real config structs,
    /// and the defaults here must match theirs.
    #[test]
    fn schema_matches_engines() {
        for kind in [FileKind::Job, FileKind::Impose] {
            let mut doc = toml_edit::DocumentMut::new();
            for s in kind.sections() {
                for fld in s.fields() {
                    let v: Option<toml_edit::Value> = match fld.ty {
                        Ty::Num { def, .. } => Some(def.into()),
                        Ty::OptNum { sample, .. } => Some(sample.into()),
                        Ty::Int { def, .. } | Ty::IntChoice { def, .. } => Some(def.into()),
                        Ty::Bool(b) => Some(b.into()),
                        Ty::Choice { def, .. } | Ty::Text { def, .. } | Ty::PdfFile { def } => Some(def.into()),
                        Ty::Cmyk(c) => Some(toml_edit::Value::Array(c.iter().copied().collect())),
                        Ty::PageMap => None,
                    };
                    if let Some(v) = v {
                        crate::doc::set_value(&mut doc, fld.key, v);
                    }
                }
            }
            let text = doc.to_string();
            match kind {
                FileKind::Job => {
                    let j: pdf_impose::sheet::config::Job = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
                    let d = pdf_impose::sheet::config::Job::default();
                    assert_eq!(j.sheet.width, d.sheet.width);
                    assert_eq!(j.sheet.gripper, d.sheet.gripper);
                    assert_eq!(j.marks.offset, d.marks.offset);
                    assert_eq!(j.colorbar.height, d.colorbar.height);
                    assert_eq!(j.output.file_name, d.output.file_name);
                    assert_eq!(j.folders.group_settle_ms, d.folders.group_settle_ms);
                    assert_eq!(j.press.work_style, d.press.work_style);
                    assert_eq!(j.binding.style, d.binding.style);
                    assert_eq!(j.marks.signature.text, d.marks.signature.text);
                }
                FileKind::Impose => {
                    let c: pdf_impose::row::config::Config = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
                    c.validate().unwrap();
                    let d = pdf_impose::row::config::Config::default();
                    assert_eq!(c.sheet.width, d.sheet.width);
                    assert_eq!(c.mark.spot.name, "die");
                    assert_eq!(c.mark.underlay.cmyk, d.mark.underlay.cmyk);
                    assert_eq!(c.mark.spot.cmyk, d.mark.spot.cmyk);
                    assert_eq!(c.layout.order, d.layout.order);
                    assert_eq!(c.output.file_name, d.output.file_name);
                }
            }
        }
    }

    /// Every key in the shipped templates appears in the schema (nothing is
    /// missing from the editor).
    #[test]
    fn schema_covers_templates() {
        for kind in [FileKind::Job, FileKind::Impose] {
            let doc: toml_edit::DocumentMut = kind.template().parse().unwrap();
            let mut keys = Vec::new();
            crate::doc::collect_keys(doc.as_table(), "", &mut keys);
            for k in keys {
                assert!(find(kind, &k).is_some(), "{} key `{k}` has no editor row", kind.file_name());
            }
        }
    }
}
