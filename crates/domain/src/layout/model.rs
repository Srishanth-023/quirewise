//! The shapes the layout analysis works on and produces.
//!
//! Positions are in points on the page as it is displayed: the origin is the top left corner and
//! `y` grows downward, so reading order is increasing `y`.

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x0: f32,
    /// Top edge.
    pub y0: f32,
    /// Right edge.
    pub x1: f32,
    /// Bottom edge.
    pub y1: f32,
}

impl Rect {
    /// A box from two opposite corners, in any order.
    pub fn new(ax: f32, ay: f32, bx: f32, by: f32) -> Self {
        Self {
            x0: ax.min(bx),
            y0: ay.min(by),
            x1: ax.max(bx),
            y1: ay.max(by),
        }
    }

    /// Width.
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    /// Height.
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// Vertical middle.
    pub fn mid_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    /// Horizontal middle.
    pub fn mid_x(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    /// The smallest box that holds both.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// True when the point lies inside or on the edge.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// True when the boxes share any area or touch, with `slack` points of tolerance.
    pub fn touches(&self, other: &Rect, slack: f32) -> bool {
        self.x0 - slack <= other.x1
            && other.x0 - slack <= self.x1
            && self.y0 - slack <= other.y1
            && other.y0 - slack <= self.y1
    }
}

/// One character with where it sits and how it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharBox {
    /// The character.
    pub ch: char,
    /// Its box.
    pub rect: Rect,
    /// Font size in points.
    pub size: f32,
    /// Bold weight.
    pub bold: bool,
    /// Italic or oblique.
    pub italic: bool,
}

/// A run of characters without a space.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// The text.
    pub text: String,
    /// Its box.
    pub rect: Rect,
    /// Font size in points.
    pub size: f32,
    /// Bold weight.
    pub bold: bool,
    /// Italic or oblique.
    pub italic: bool,
}

/// A thin line or a thin filled bar: the strokes tables are drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rule {
    /// Its box.
    pub rect: Rect,
}

/// A picture on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureBox {
    /// Where it is drawn.
    pub rect: Rect,
    /// Which of the document's pictures it is.
    pub index: usize,
}

/// Everything the analysis needs to know about one page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageContent {
    /// Page number from 1.
    pub number: u32,
    /// Page width in points as displayed.
    pub width: f32,
    /// Page height in points as displayed.
    pub height: f32,
    /// The words, in any order.
    pub words: Vec<Word>,
    /// The lines and bars that may be table borders.
    pub rules: Vec<Rule>,
    /// The pictures.
    pub pictures: Vec<PictureBox>,
}

/// Text in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// The text.
    pub text: String,
    /// Bold weight.
    pub bold: bool,
    /// Italic or oblique.
    pub italic: bool,
    /// Font size in points.
    pub size: f32,
}

/// One cell of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The text, lines joined by spaces.
    pub text: String,
    /// Bold weight throughout.
    pub bold: bool,
}

/// A table: every row has as many cells as the table has columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Where it is on the page.
    pub rect: Rect,
    /// The rows, top to bottom.
    pub rows: Vec<Vec<Cell>>,
    /// Left edge of every column, for exact placement.
    pub column_edges: Vec<f32>,
    /// Top edge of every row, for exact placement.
    pub row_edges: Vec<f32>,
}

impl Table {
    /// Number of columns.
    pub fn columns(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }
}

/// One piece of a page, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A heading of level 1 (largest) to 3.
    Heading {
        /// Level from 1 to 3.
        level: u8,
        /// The text.
        spans: Vec<Span>,
        /// Where it is.
        rect: Rect,
    },
    /// A paragraph.
    Paragraph {
        /// The text.
        spans: Vec<Span>,
        /// Where it is.
        rect: Rect,
    },
    /// An item of a bulleted or numbered list.
    ListItem {
        /// True for numbers or letters, false for bullets.
        ordered: bool,
        /// The marker as written, such as `1.` or `a)`, for ordered items.
        marker: String,
        /// Nesting depth from 0.
        level: u8,
        /// The text without its marker.
        spans: Vec<Span>,
        /// Where it is.
        rect: Rect,
    },
    /// A table.
    Table(Table),
    /// A picture.
    Picture(PictureBox),
}

impl Block {
    /// Where the block is on the page.
    pub fn rect(&self) -> Rect {
        match self {
            Self::Heading { rect, .. }
            | Self::Paragraph { rect, .. }
            | Self::ListItem { rect, .. } => *rect,
            Self::Table(t) => t.rect,
            Self::Picture(p) => p.rect,
        }
    }
}

/// A page after the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// Page number from 1.
    pub number: u32,
    /// Page width in points as displayed.
    pub width: f32,
    /// Page height in points as displayed.
    pub height: f32,
    /// The blocks in reading order.
    pub blocks: Vec<Block>,
}

/// The text of spans joined, without styles.
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect::<String>()
}
