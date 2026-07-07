//! The `ext_linegrid` cell model and redraw-event application (`:help ui-linegrid`).
//! Single grid (grid 1): `ext_multigrid` is not enabled, so floats, the cmdline, messages and
//! `:confirm` prompts all composite into grid 1 — events for any other grid id are ignored.
//!
//! Blit contract (the UI half paints from this): iterate `row(r)`; a [`CellText::WideTail`] is
//! the right half of a double-width glyph — emit nothing and let the left cell cover both
//! columns; resolve style via [`Grid::attr`], falling back to `default_fg`/`default_bg`; nvim
//! does NOT paint the cursor into cells — draw it at [`Grid::cursor`] when the pane is focused
//! and `cursor_visible`.

use std::collections::HashMap;

use rmpv::Value;

pub type Rgb = (u8, u8, u8);

/// What a cell displays. Compact: the common case is one scalar with no allocation, so the
/// per-flush front-buffer copy stays cheap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellText {
    /// One Unicode scalar (the overwhelmingly common case).
    Char(char),
    /// A multi-codepoint grapheme cluster (combining marks, ZWJ emoji) — nvim sends the whole
    /// cluster as one cell's text.
    Cluster(Box<str>),
    /// The right half of a double-width char: nvim sends it as the empty string.
    WideTail,
}

impl Default for CellText {
    fn default() -> Self {
        Self::Char(' ')
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub text: CellText,
    /// Highlight id into [`Grid::hl`]. 0 = default colors.
    pub hl: u32,
}

/// One entry of the `hl_attr_define` table (`rgb_attr` map; `cterm_attr` ignored — rgb=true).
/// The bools mirror nvim's attribute flags one-to-one — a bitfield would only obscure that.
#[expect(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HlAttr {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    /// The "special" (underline) color; the blit may ignore it.
    pub sp: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    /// Any of underline/undercurl/underdouble/underdotted/underdashed — collapsed because
    /// ratatui exposes a single UNDERLINED modifier.
    pub underline: bool,
    pub strikethrough: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Horizontal,
    Vertical,
}

/// The composited screen: what nvim has told us to display, one frame behind its `flush`.
#[derive(Clone, Debug)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    /// Row-major; `len == cols as usize * rows as usize`.
    pub cells: Vec<Cell>,
    pub hl: HashMap<u32, HlAttr>,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    /// (row, col), 0-based, grid-relative.
    pub cursor: (u16, u16),
    /// `busy_start`/`busy_stop`: the cursor hides while nvim is busy.
    pub cursor_visible: bool,
    /// Current mode short name from `mode_change` (e.g. "normal", "insert", `cmdline_normal`).
    pub mode: String,
    pub cursor_shape: CursorShape,
}

impl Grid {
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
            hl: HashMap::new(),
            // Sane pre-`default_colors_set` seed so frame 1 doesn't flash white-on-white.
            default_fg: (0xc6, 0xc6, 0xc6),
            default_bg: (0x1e, 0x1e, 0x2e),
            cursor: (0, 0),
            cursor_visible: true,
            mode: "normal".to_string(),
            cursor_shape: CursorShape::Block,
        }
    }

    /// The cells of row `r` (empty when out of range — never panics).
    #[must_use]
    pub fn row(&self, r: u16) -> &[Cell] {
        if r >= self.rows {
            return &[];
        }
        let start = r as usize * self.cols as usize;
        &self.cells[start..start + self.cols as usize]
    }

    /// The attr for an hl id; 0 or unknown ids resolve to defaults.
    #[must_use]
    pub fn attr(&self, id: u32) -> HlAttr {
        self.hl.get(&id).copied().unwrap_or_default()
    }

    /// Row text with wide-tail cells skipped — for tests and debug dumps.
    #[must_use]
    pub fn row_text(&self, r: u16) -> String {
        let mut out = String::new();
        for c in self.row(r) {
            match &c.text {
                CellText::Char(ch) => out.push(*ch),
                CellText::Cluster(s) => out.push_str(s),
                CellText::WideTail => {}
            }
        }
        out
    }

    /// Allocation-reusing copy for the flush handoff (`Vec`/`HashMap` `clone_from` keep their
    /// buffers).
    pub fn copy_from(&mut self, src: &Grid) {
        self.cols = src.cols;
        self.rows = src.rows;
        self.cells.clone_from(&src.cells);
        self.hl.clone_from(&src.hl);
        self.default_fg = src.default_fg;
        self.default_bg = src.default_bg;
        self.cursor = src.cursor;
        self.cursor_visible = src.cursor_visible;
        self.mode.clone_from(&src.mode);
        self.cursor_shape = src.cursor_shape;
    }
}

/// What a redraw batch did, for the reader loop's flush handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub flush: bool,
}

/// Apply one `redraw` notification's params: an array of event batches, each
/// `[name, args..., args...]` where every element after the name is one invocation's arg array.
/// Unknown events, non-grid-1 grids and malformed args are ignored — never panic.
pub fn apply_redraw(
    grid: &mut Grid,
    mode_shapes: &mut Vec<CursorShape>,
    params: &[Value],
) -> Applied {
    let mut applied = Applied::default();
    for batch in params {
        let Some(batch) = batch.as_array() else { continue };
        let Some(name) = batch.first().and_then(Value::as_str) else { continue };
        for args in &batch[1..] {
            let Some(args) = args.as_array() else { continue };
            match name {
                "grid_resize" => apply_resize(grid, args),
                "grid_clear" => apply_clear(grid, args),
                "grid_line" => apply_line(grid, args),
                "grid_scroll" => apply_scroll(grid, args),
                "grid_cursor_goto" => apply_cursor_goto(grid, args),
                "hl_attr_define" => apply_hl_attr(grid, args),
                "default_colors_set" => apply_default_colors(grid, args),
                "mode_info_set" => apply_mode_info(mode_shapes, args),
                "mode_change" => apply_mode_change(grid, mode_shapes, args),
                "busy_start" => grid.cursor_visible = false,
                "busy_stop" => grid.cursor_visible = true,
                "flush" => applied.flush = true,
                _ => {}
            }
        }
    }
    applied
}

/// Whether an event's grid argument targets the one grid we track (grid 1).
fn on_grid_1(args: &[Value]) -> bool {
    args.first().and_then(Value::as_u64) == Some(1)
}

fn arg_u16(args: &[Value], i: usize) -> Option<u16> {
    args.get(i).and_then(Value::as_u64).and_then(|v| u16::try_from(v).ok())
}

fn arg_i64(args: &[Value], i: usize) -> Option<i64> {
    args.get(i).and_then(Value::as_i64)
}

fn apply_resize(grid: &mut Grid, args: &[Value]) {
    if !on_grid_1(args) {
        return;
    }
    let (Some(w), Some(h)) = (arg_u16(args, 1), arg_u16(args, 2)) else { return };
    grid.cols = w;
    grid.rows = h;
    // nvim fully repaints after a resize within the same batch; no content preserve needed.
    grid.cells.clear();
    grid.cells.resize(w as usize * h as usize, Cell::default());
    grid.cursor.0 = grid.cursor.0.min(h.saturating_sub(1));
    grid.cursor.1 = grid.cursor.1.min(w.saturating_sub(1));
}

fn apply_clear(grid: &mut Grid, args: &[Value]) {
    if !on_grid_1(args) {
        return;
    }
    grid.cells.fill(Cell::default());
}

fn apply_line(grid: &mut Grid, args: &[Value]) {
    if !on_grid_1(args) {
        return;
    }
    let (Some(row), Some(col_start)) = (arg_u16(args, 1), arg_u16(args, 2)) else { return };
    let Some(cells) = args.get(3).and_then(Value::as_array) else { return };
    if row >= grid.rows {
        return;
    }
    let base = row as usize * grid.cols as usize;
    let mut col = col_start as usize;
    // The carried hl id: entries with one element reuse the previous entry's id. nvim always
    // sends an id on an invocation's first cell; 0 is the defensive fallback.
    let mut carry: u32 = 0;
    for entry in cells {
        let Some(entry) = entry.as_array() else { continue };
        let Some(text) = entry.first().and_then(Value::as_str) else { continue };
        if entry.len() >= 2
            && let Some(id) = entry.get(1).and_then(Value::as_u64)
        {
            carry = u32::try_from(id).unwrap_or(0);
        }
        // An absent repeat means one cell; an EXPLICIT `repeat: 0` is legal and means zero —
        // nvim emits e.g. `[" ", 0, 0]` purely to reset the carried hl id without writing.
        // Clamping it up to 1 stamps a spurious cell over real content (seen live: the first
        // text column vanished whenever a number_hl_group extmark redrew the row).
        let repeat = entry.get(2).and_then(Value::as_u64).unwrap_or(1) as usize;
        let cell_text = |t: &str| -> CellText {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (None, _) => CellText::WideTail,
                (Some(c), None) => CellText::Char(c),
                _ => CellText::Cluster(t.into()),
            }
        };
        for _ in 0..repeat {
            if col >= grid.cols as usize {
                return; // clamp: drop anything past the row edge
            }
            grid.cells[base + col] = Cell { text: cell_text(text), hl: carry };
            col += 1;
        }
    }
}

fn apply_scroll(grid: &mut Grid, args: &[Value]) {
    if !on_grid_1(args) {
        return;
    }
    let (Some(top), Some(bot), Some(left), Some(right), Some(rows)) =
        (arg_u16(args, 1), arg_u16(args, 2), arg_u16(args, 3), arg_u16(args, 4), arg_i64(args, 5))
    else {
        return;
    };
    // Region rows [top, bot) × cols [left, right), both exclusive-end, clamped to the grid.
    let top = i64::from(top.min(grid.rows));
    let bot = i64::from(bot.min(grid.rows));
    let left = (left.min(grid.cols)) as usize;
    let right = (right.min(grid.cols)) as usize;
    if left >= right || top >= bot || rows == 0 {
        return;
    }
    let cols = grid.cols as usize;
    // Every dst with both dst and dst+rows inside [top, bot) copies dst+rows → dst. Ascending
    // for rows > 0 (content moves up), descending for rows < 0, so in-place copies never read
    // rows already overwritten. Vacated rows are left as-is — nvim repaints them via grid_line.
    let copy_row = |cells: &mut [Cell], dst: i64, src: i64| {
        let (d, s) = (dst as usize * cols, src as usize * cols);
        // Split-borrow via split_at_mut to move a row within the same Vec.
        if d < s {
            let (head, tail) = cells.split_at_mut(s);
            head[d + left..d + right].clone_from_slice(&tail[left..right]);
        } else {
            let (head, tail) = cells.split_at_mut(d);
            tail[left..right].clone_from_slice(&head[s + left..s + right]);
        }
    };
    if rows > 0 {
        let mut dst = top;
        while dst < bot && dst + rows < bot {
            copy_row(&mut grid.cells, dst, dst + rows);
            dst += 1;
        }
    } else {
        let mut dst = bot - 1;
        while dst >= top && dst + rows >= top {
            copy_row(&mut grid.cells, dst, dst + rows);
            if dst == top {
                break;
            }
            dst -= 1;
        }
    }
}

fn apply_cursor_goto(grid: &mut Grid, args: &[Value]) {
    if !on_grid_1(args) {
        return;
    }
    let (Some(row), Some(col)) = (arg_u16(args, 1), arg_u16(args, 2)) else { return };
    grid.cursor = (row.min(grid.rows.saturating_sub(1)), col.min(grid.cols.saturating_sub(1)));
}

fn apply_hl_attr(grid: &mut Grid, args: &[Value]) {
    let Some(id) = args.first().and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) else {
        return;
    };
    let Some(rgb) = args.get(1).and_then(Value::as_map) else { return };
    let mut attr = HlAttr::default();
    for (k, v) in rgb {
        let Some(k) = k.as_str() else { continue };
        match k {
            "foreground" => attr.fg = v.as_u64().map(unpack_rgb),
            "background" => attr.bg = v.as_u64().map(unpack_rgb),
            "special" => attr.sp = v.as_u64().map(unpack_rgb),
            "bold" => attr.bold = v.as_bool().unwrap_or(false),
            "italic" => attr.italic = v.as_bool().unwrap_or(false),
            "reverse" => attr.reverse = v.as_bool().unwrap_or(false),
            "strikethrough" => attr.strikethrough = v.as_bool().unwrap_or(false),
            "underline" | "undercurl" | "underdouble" | "underdotted" | "underdashed" => {
                attr.underline |= v.as_bool().unwrap_or(false);
            }
            _ => {}
        }
    }
    grid.hl.insert(id, attr);
}

fn apply_default_colors(grid: &mut Grid, args: &[Value]) {
    // Signed reads: a negative value means "unset" — keep the previous/seed default.
    if let Some(fg) = arg_i64(args, 0).filter(|v| *v >= 0) {
        grid.default_fg = unpack_rgb(fg as u64);
    }
    if let Some(bg) = arg_i64(args, 1).filter(|v| *v >= 0) {
        grid.default_bg = unpack_rgb(bg as u64);
    }
}

fn apply_mode_info(mode_shapes: &mut Vec<CursorShape>, args: &[Value]) {
    let Some(infos) = args.get(1).and_then(Value::as_array) else { return };
    mode_shapes.clear();
    for info in infos {
        let shape = info
            .as_map()
            .and_then(|m| {
                m.iter()
                    .find(|(k, _)| k.as_str() == Some("cursor_shape"))
                    .and_then(|(_, v)| v.as_str())
                    .map(|s| match s {
                        "horizontal" => CursorShape::Horizontal,
                        "vertical" => CursorShape::Vertical,
                        _ => CursorShape::Block,
                    })
            })
            .unwrap_or_default();
        mode_shapes.push(shape);
    }
}

fn apply_mode_change(grid: &mut Grid, mode_shapes: &[CursorShape], args: &[Value]) {
    if let Some(name) = args.first().and_then(Value::as_str) {
        name.clone_into(&mut grid.mode);
    }
    if let Some(idx) = args.get(1).and_then(Value::as_u64) {
        grid.cursor_shape = mode_shapes.get(idx as usize).copied().unwrap_or_default();
    }
}

fn unpack_rgb(v: u64) -> Rgb {
    (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `["name", args...]` — one event batch with one invocation.
    fn ev(name: &str, args: Vec<Value>) -> Value {
        Value::Array(vec![Value::from(name), Value::Array(args)])
    }

    fn apply(grid: &mut Grid, events: &[Value]) -> Applied {
        apply_redraw(grid, &mut Vec::new(), events)
    }

    fn line_cells(entries: Vec<Value>) -> Value {
        Value::Array(entries)
    }

    fn cell(text: &str, rest: &[u64]) -> Value {
        let mut v = vec![Value::from(text)];
        v.extend(rest.iter().map(|&n| Value::from(n)));
        Value::Array(v)
    }

    #[test]
    fn grid_line_applies_text_and_carries_hl() {
        let mut g = Grid::new(10, 2);
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(2),
                    line_cells(vec![cell("h", &[1]), cell("i", &[]), cell("!", &[2, 3])]),
                ],
            )],
        );
        let row = g.row(0);
        assert_eq!(row[2], Cell { text: CellText::Char('h'), hl: 1 });
        assert_eq!(row[3], Cell { text: CellText::Char('i'), hl: 1 }); // carried
        for cell in &row[4..7] {
            assert_eq!(*cell, Cell { text: CellText::Char('!'), hl: 2 });
        }
        assert_eq!(row[0], Cell::default()); // untouched before col_start
        assert_eq!(row[7], Cell::default()); // untouched after the run
    }

    #[test]
    fn grid_line_zero_repeat_writes_nothing() {
        // nvim emits `[" ", 0, 0]` (explicit repeat 0) purely to reset the carried hl id —
        // seen live after a number_hl_group extmark redraw: `[[" ",43,2],["3"],[" "],[" ",0,0]]`
        // at col_start 2. Clamping 0 up to 1 stamped a blank over the first text cell.
        let mut g = Grid::new(20, 1);
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell("f", &[7]), cell("n", &[])]),
                ],
            )],
        );
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell(" ", &[43, 0]), cell(" ", &[0, 0])]),
                ],
            )],
        );
        // Both zero-repeat entries wrote nothing; the original text is intact.
        assert_eq!(g.row(0)[0], Cell { text: CellText::Char('f'), hl: 7 });
        assert_eq!(g.row(0)[1], Cell { text: CellText::Char('n'), hl: 7 });
    }

    #[test]
    fn grid_line_zero_repeat_writes_nothing_but_updates_the_carry() {
        let mut g = Grid::new(10, 1);
        // Seed real content, then replay nvim's hl-reset pattern: an explicit `repeat: 0`
        // entry must not stamp a cell (it once blanked the first text column whenever a
        // number_hl_group extmark redrew the row) — but its hl id must still carry over.
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell("f", &[7]), cell("n", &[])]),
                ],
            )],
        );
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell(" ", &[9, 0]), cell("X", &[])]),
                ],
            )],
        );
        // The zero-repeat " " wrote nothing: "X" landed at col 0 (with the carried hl 9),
        // and col 1 still holds the original "n".
        assert_eq!(g.row(0)[0], Cell { text: CellText::Char('X'), hl: 9 });
        assert_eq!(g.row(0)[1], Cell { text: CellText::Char('n'), hl: 7 });
    }

    #[test]
    fn grid_line_repeat_fills_a_row() {
        let mut g = Grid::new(80, 1);
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell(" ", &[0, 80])]),
                ],
            )],
        );
        assert!(g.row(0).iter().all(|c| *c == Cell::default()));
    }

    #[test]
    fn doublewidth_stores_a_wide_tail() {
        let mut g = Grid::new(4, 1);
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell("漢", &[1]), cell("", &[1])]),
                ],
            )],
        );
        assert_eq!(g.row(0)[0], Cell { text: CellText::Char('漢'), hl: 1 });
        assert_eq!(g.row(0)[1], Cell { text: CellText::WideTail, hl: 1 });
        assert_eq!(g.row_text(0), "漢  "); // tail skipped, trailing default spaces kept
    }

    #[test]
    fn multi_codepoint_cluster_is_one_cell() {
        let mut g = Grid::new(4, 1);
        apply(
            &mut g,
            &[ev(
                "grid_line",
                vec![
                    Value::from(1),
                    Value::from(0),
                    Value::from(0),
                    line_cells(vec![cell("e\u{301}", &[1])]),
                ],
            )],
        );
        assert_eq!(g.row(0)[0].text, CellText::Cluster("e\u{301}".into()));
    }

    /// A 6-row grid where every cell of row r is marked Char(digit r) for provenance checks.
    fn marked(rows: u16, cols: u16) -> Grid {
        let mut g = Grid::new(cols, rows);
        for r in 0..rows {
            for c in 0..cols {
                g.cells[r as usize * cols as usize + c as usize] =
                    Cell { text: CellText::Char(char::from(b'0' + r as u8)), hl: u32::from(r) };
            }
        }
        g
    }

    #[test]
    fn grid_scroll_up_moves_the_region() {
        let mut g = marked(6, 10);
        // top=1 bot=5 left=2 right=8, rows=2: rows 1,2 receive old rows 3,4 (cols 2..8 only).
        apply(
            &mut g,
            &[ev("grid_scroll", vec![1, 1, 5, 2, 8, 2, 0].into_iter().map(Value::from).collect())],
        );
        assert_eq!(g.row(1)[2].text, CellText::Char('3'));
        assert_eq!(g.row(2)[7].text, CellText::Char('4'));
        assert_eq!(g.row(1)[1].text, CellText::Char('1')); // outside cols untouched
        assert_eq!(g.row(1)[8].text, CellText::Char('1'));
        assert_eq!(g.row(0)[2].text, CellText::Char('0')); // outside rows untouched
        assert_eq!(g.row(5)[2].text, CellText::Char('5'));
    }

    #[test]
    fn grid_scroll_down_iterates_bottom_up() {
        let mut g = marked(6, 10);
        // rows=-1: content moves down one; row 4 gets old row 3, row 2 gets old row 1.
        apply(
            &mut g,
            &[ev(
                "grid_scroll",
                vec![
                    Value::from(1),
                    Value::from(1),
                    Value::from(5),
                    Value::from(0),
                    Value::from(10),
                    Value::from(-1),
                    Value::from(0),
                ],
            )],
        );
        assert_eq!(g.row(4)[0].text, CellText::Char('3'));
        assert_eq!(g.row(3)[0].text, CellText::Char('2'));
        assert_eq!(g.row(2)[0].text, CellText::Char('1')); // no self-overwrite corruption
    }

    #[test]
    fn grid_resize_reallocates_and_clamps_cursor() {
        let mut g = Grid::new(10, 5);
        g.cursor = (4, 9);
        apply(&mut g, &[ev("grid_resize", vec![Value::from(1), Value::from(4), Value::from(2)])]);
        assert_eq!((g.cols, g.rows), (4, 2));
        assert_eq!(g.cells.len(), 8);
        assert_eq!(g.cursor, (1, 3));
    }

    #[test]
    fn grid_clear_resets_cells() {
        let mut g = marked(3, 3);
        apply(&mut g, &[ev("grid_clear", vec![Value::from(1)])]);
        assert!(g.cells.iter().all(|c| *c == Cell::default()));
    }

    #[test]
    fn hl_attr_define_parses_colors_and_flags() {
        let mut g = Grid::new(1, 1);
        let rgb = Value::Map(vec![
            (Value::from("foreground"), Value::from(0x00ff_8800_u64)),
            (Value::from("bold"), Value::from(true)),
            (Value::from("undercurl"), Value::from(true)),
        ]);
        apply(
            &mut g,
            &[ev(
                "hl_attr_define",
                vec![Value::from(5), rgb, Value::Map(vec![]), Value::Array(vec![])],
            )],
        );
        let a = g.attr(5);
        assert_eq!(a.fg, Some((255, 136, 0)));
        assert!(a.bold && a.underline && !a.italic);
        assert_eq!(g.attr(99), HlAttr::default()); // unknown id → defaults
    }

    #[test]
    fn default_colors_set_treats_negative_as_unset() {
        let mut g = Grid::new(1, 1);
        let seed = (g.default_fg, g.default_bg);
        apply(
            &mut g,
            &[ev(
                "default_colors_set",
                vec![Value::from(-1i64), Value::from(0x0010_2030_u64), Value::from(-1i64)],
            )],
        );
        assert_eq!(g.default_fg, seed.0); // negative fg: seed kept
        assert_eq!(g.default_bg, (0x10, 0x20, 0x30));
    }

    #[test]
    fn cursor_mode_and_busy_events() {
        let mut g = Grid::new(10, 5);
        let mut shapes = Vec::new();
        let infos = Value::Array(vec![
            Value::Map(vec![(Value::from("cursor_shape"), Value::from("block"))]),
            Value::Map(vec![(Value::from("cursor_shape"), Value::from("vertical"))]),
        ]);
        apply_redraw(
            &mut g,
            &mut shapes,
            &[
                ev("mode_info_set", vec![Value::from(true), infos]),
                ev("mode_change", vec![Value::from("insert"), Value::from(1)]),
                ev("grid_cursor_goto", vec![Value::from(1), Value::from(2), Value::from(3)]),
                ev("busy_start", vec![]),
            ],
        );
        assert_eq!(g.mode, "insert");
        assert_eq!(g.cursor_shape, CursorShape::Vertical);
        assert_eq!(g.cursor, (2, 3));
        assert!(!g.cursor_visible);
        apply_redraw(&mut g, &mut shapes, &[ev("busy_stop", vec![])]);
        assert!(g.cursor_visible);
    }

    #[test]
    fn flush_is_reported_and_unknown_events_ignored() {
        let mut g = Grid::new(2, 2);
        let applied = apply(
            &mut g,
            &[
                ev("set_title", vec![Value::from("x")]),
                ev("frobnicate", vec![Value::from(1)]),
                ev("flush", vec![]),
            ],
        );
        assert!(applied.flush);
    }

    #[test]
    fn grid_line_out_of_bounds_is_clamped() {
        let mut g = Grid::new(3, 2);
        apply(
            &mut g,
            &[
                // Row beyond the grid: dropped.
                ev(
                    "grid_line",
                    vec![
                        Value::from(1),
                        Value::from(9),
                        Value::from(0),
                        line_cells(vec![cell("x", &[1])]),
                    ],
                ),
                // Column overflow: in-range cells written, the rest dropped.
                ev(
                    "grid_line",
                    vec![
                        Value::from(1),
                        Value::from(0),
                        Value::from(2),
                        line_cells(vec![cell("a", &[1]), cell("b", &[1])]),
                    ],
                ),
            ],
        );
        assert_eq!(g.row(0)[2].text, CellText::Char('a'));
        assert_eq!(g.row(1)[0], Cell::default());
    }

    #[test]
    fn copy_from_matches_clone() {
        let mut src = marked(4, 6);
        src.hl.insert(3, HlAttr { bold: true, ..HlAttr::default() });
        src.cursor = (2, 2);
        src.mode = "visual".into();
        let mut dst = Grid::new(1, 1);
        dst.copy_from(&src);
        assert_eq!(dst.cells, src.cells);
        assert_eq!(dst.hl, src.hl);
        assert_eq!((dst.cols, dst.rows, dst.cursor), (src.cols, src.rows, src.cursor));
        assert_eq!(dst.mode, src.mode);
    }
}
