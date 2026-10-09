use std::sync::OnceLock;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use regex::Regex;

const MAX_WRAP_ROWS: usize = 32;
// A bare token has no closing delimiter to prove where it ends. Only bridge
// small TUI margins, and cap the amount of grid text inspected on each hover.
const MAX_FILE_WRAP_PADDING: usize = 8;
const MAX_FILE_WRAP_CELLS: usize = 4096;

const MATCH_WINDOW: usize = 2000;

const BRACKET_PAIRS: [(char, char); 15] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('（', '）'),
    ('［', '］'),
    ('｛', '｝'),
    ('〈', '〉'),
    ('《', '》'),
    ('「', '」'),
    ('『', '』'),
    ('【', '】'),
    ('〔', '〕'),
    ('“', '”'),
    ('‘', '’'),
];

const SYMMETRIC_QUOTES: [char; 3] = ['\'', '"', '`'];

pub(super) struct SmartRange {
    pub start: Point,
    pub end: Point,
    pub exact: bool,
}

pub(super) fn grid_smart_range<T: EventListener>(
    term: &Term<T>,
    click: Point,
) -> Option<SmartRange> {
    if click.line < term.topmost_line() || click.line > term.bottommost_line() {
        return None;
    }

    if let Some((start, end)) = hyperlink_run(term, click) {
        return Some(SmartRange {
            start,
            end,
            exact: true,
        });
    }

    let (text, points, click_idx) = logical_line_at(term, click, false)?;
    let chars: Vec<char> = text.chars().collect();
    let separators = term.semantic_escape_chars();
    let resolved = |s: usize, e: usize| SmartRange {
        start: points[s],
        end: points[e],
        exact: !(s == 0 || separators.contains(chars[s - 1]))
            || !(e + 1 == chars.len() || separators.contains(chars[e + 1])),
    };

    if let Some((s, e)) = pair_range(&chars, click_idx) {
        return Some(resolved(s, e));
    }

    if is_cjk(chars[click_idx])
        && let Some((s, e)) = cjk_word_range(&text, click_idx)
    {
        return Some(resolved(s, e));
    }

    let (s, e) = smart_range(&text, &chars, click_idx, separators)?;
    Some(resolved(s, e))
}

pub(super) fn is_cjk(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x11FF
        | 0x2E80..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF
        | 0x20000..=0x3134F
    )
}

#[cfg(not(target_os = "macos"))]
fn is_kana_or_hangul(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x11FF
        | 0x3040..=0x30FF
        | 0x31F0..=0x31FF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF
        | 0xFF66..=0xFF9F
    )
}

#[cfg(not(target_os = "macos"))]
static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();

#[cfg(not(target_os = "macos"))]
fn warm() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            let _ = JIEBA.get_or_init(jieba_rs::Jieba::new);
        });
    });
}

pub(super) fn cjk_word_range(text: &str, click: usize) -> Option<(usize, usize)> {
    #[cfg(target_os = "macos")]
    {
        tokenizer::word_range(text, click)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let chars: Vec<char> = text.chars().collect();
        chars.get(click)?;
        jieba_word_range(&chars, click)
    }
}

#[cfg(not(target_os = "macos"))]
fn jieba_word_range(chars: &[char], click: usize) -> Option<(usize, usize)> {
    let mut rs = click;
    while rs > 0 && is_cjk(chars[rs - 1]) {
        rs -= 1;
    }
    let mut re = click;
    while re + 1 < chars.len() && is_cjk(chars[re + 1]) {
        re += 1;
    }
    if chars[rs..=re].iter().copied().any(is_kana_or_hangul) {
        return None;
    }
    let Some(jieba) = JIEBA.get() else {
        warm();
        return None;
    };
    let run: String = chars[rs..=re].iter().collect();
    let rel = click - rs;
    jieba
        .cut(&run, true)
        .iter()
        .find(|tok| rel < tok.end)
        .map(|tok| (rs + tok.start, rs + tok.end - 1))
}

#[cfg(target_os = "macos")]
mod tokenizer {
    use core_foundation::base::{CFIndex, CFRange, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use std::os::raw::c_void;

    type CFStringTokenizerRef = *mut c_void;
    type CFLocaleRef = *const c_void;

    const UNIT_WORD_BOUNDARY: u64 = 4;

    unsafe extern "C" {
        fn CFStringTokenizerCreate(
            alloc: *const c_void,
            string: CFStringRef,
            range: CFRange,
            options: u64,
            locale: CFLocaleRef,
        ) -> CFStringTokenizerRef;
        fn CFStringTokenizerGoToTokenAtIndex(
            tokenizer: CFStringTokenizerRef,
            index: CFIndex,
        ) -> u64;
        fn CFStringTokenizerGetCurrentTokenRange(tokenizer: CFStringTokenizerRef) -> CFRange;
        fn CFLocaleCopyCurrent() -> CFLocaleRef;
        fn CFRelease(cf: *const c_void);
    }

    pub(super) fn word_range(text: &str, click: usize) -> Option<(usize, usize)> {
        let mut u16_of: Vec<CFIndex> = Vec::new();
        let mut total: CFIndex = 0;
        for c in text.chars() {
            u16_of.push(total);
            total += c.len_utf16() as CFIndex;
        }
        let click_u16 = *u16_of.get(click)?;

        let cf = CFString::new(text);
        let range = unsafe {
            let locale = CFLocaleCopyCurrent();
            let tok = CFStringTokenizerCreate(
                std::ptr::null(),
                cf.as_concrete_TypeRef(),
                CFRange::init(0, total),
                UNIT_WORD_BOUNDARY,
                locale,
            );
            let token_type = CFStringTokenizerGoToTokenAtIndex(tok, click_u16);
            let range = (token_type != 0).then(|| CFStringTokenizerGetCurrentTokenRange(tok));
            CFRelease(tok);
            if !locale.is_null() {
                CFRelease(locale);
            }
            range?
        };
        if range.location < 0 || range.length <= 0 {
            return None;
        }
        let start = u16_of.binary_search(&range.location).ok()?;
        let end = u16_of.partition_point(|&v| v < range.location + range.length) - 1;
        (start <= click && click <= end).then_some((start, end))
    }
}

pub(super) fn hyperlink_run<T: EventListener>(
    term: &Term<T>,
    click: Point,
) -> Option<(Point, Point)> {
    let grid = term.grid();
    let cols = term.columns();
    if click.column.0 >= cols {
        return None;
    }
    let uri = grid[click.line][click.column]
        .hyperlink()?
        .uri()
        .to_string();
    let same = |p: Point| {
        grid[p.line][p.column]
            .hyperlink()
            .is_some_and(|h| h.uri() == uri)
    };
    let wraps = |line: Line| grid[line][Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    let top = term.topmost_line();
    let bottom = term.bottommost_line();

    let mut start = click;
    let mut rows = 0;
    loop {
        let prev = if start.column.0 > 0 {
            Point::new(start.line, Column(start.column.0 - 1))
        } else if start.line > top && rows < MAX_WRAP_ROWS && wraps(start.line - 1) {
            rows += 1;
            Point::new(start.line - 1, Column(cols - 1))
        } else {
            break;
        };
        if !same(prev) {
            break;
        }
        start = prev;
    }
    let mut end = click;
    rows = 0;
    loop {
        let next = if end.column.0 + 1 < cols {
            Point::new(end.line, Column(end.column.0 + 1))
        } else if end.line < bottom && rows < MAX_WRAP_ROWS && wraps(end.line) {
            rows += 1;
            Point::new(end.line + 1, Column(0))
        } else {
            break;
        };
        if !same(next) {
            break;
        }
        end = next;
    }
    Some((start, end))
}

/// A logical line, the grid point of every character in it, and which of
/// those characters the click landed on.
pub(super) type LogicalLine = (String, Vec<Point>, usize);

pub(super) fn logical_line_at<T: EventListener>(
    term: &Term<T>,
    click: Point,
    bridge_hard_wrap: bool,
) -> Option<LogicalLine> {
    if bridge_hard_wrap {
        return link_lines_at(term, click).map(|(line, _)| line);
    }
    wrapped_line_at(term, click, false)
}

/// Every line a link under `click` can be read from, best first.
///
/// Bridging a bare path across a TUI margin is a guess the grid cannot check.
/// A path the TUI wrapped mid-name looks exactly like one that ends at the
/// margin with prose starting the next row: `host_ops.rs` above `中增加了`,
/// or English that lost its space to the wrap and reads `host_ops.rsand`.
/// So the line as the terminal wrapped it comes back as well, and the
/// resolver lets the filesystem choose, the bridged reading first.
pub(super) fn link_lines_at<T: EventListener>(
    term: &Term<T>,
    click: Point,
) -> Option<(LogicalLine, Option<LogicalLine>)> {
    if !on_grid(term, click) {
        return None;
    }
    if let Some(cell) = table_cell_at(term, click) {
        return Some((cell, wrapped_line_at(term, click, true)));
    }
    if let Some(link) = enclosed_file_line_at(term, click) {
        return Some((link, None));
    }
    let wrapped = wrapped_line_at(term, click, true);
    match padded_file_line_at(term, click) {
        Some(padded) => Some((padded, wrapped)),
        None => wrapped.map(|line| (line, None)),
    }
}

fn on_grid<T: EventListener>(term: &Term<T>, click: Point) -> bool {
    click.column.0 < term.columns()
        && click.line >= term.topmost_line()
        && click.line <= term.bottommost_line()
}

/// The line as the terminal itself wrapped it. `bridge_hard_wrap` also joins
/// a URL cut at the right edge by a hard newline.
fn wrapped_line_at<T: EventListener>(
    term: &Term<T>,
    click: Point,
    bridge_hard_wrap: bool,
) -> Option<LogicalLine> {
    if !on_grid(term, click) {
        return None;
    }
    let cols = term.columns();
    let grid = term.grid();
    let last_col = Column(cols - 1);
    let top = term.topmost_line();
    let bottom = term.bottommost_line();
    let wraps = |line: Line| grid[line][last_col].flags.contains(Flags::WRAPLINE);
    let is_link_char = |c: char| super::search::is_url_char(c);
    let hard = |line: Line| {
        bridge_hard_wrap && line < bottom && is_link_char(grid[line][last_col].c) && {
            let next = grid[Line(line.0 + 1)][Column(0)].c;
            is_link_char(next) && next != '@'
        }
    };
    let continues = |line: Line| wraps(line) || hard(line);

    let (start_line, end_line) = run_of_rows(click.line, top, bottom, continues);
    let rows = (start_line.0..=end_line.0).map(|line| (Line(line), Column(0), last_col));
    read_rows(term, click, rows)
}

/// What a table's renderer draws between cells. Only the box-drawing ones: a
/// bare `|` is a shell pipe or a markdown source line far more often than it
/// is a border.
const CELL_BORDERS: [char; 3] = ['│', '┃', '║'];

/// The text of the table cell under `click`, gathered from every row the
/// table's renderer wrapped it onto.
///
/// It is the same stitching [`logical_line_at`] does for a whole row, only
/// inside the cell's own columns: a row carries on into the next when its
/// text runs up to the cell's right border and the next row's picks up at
/// the left one, link characters on both sides of the break. The border
/// columns have to line up too, which is what stops the walk at a `├─┼`
/// rule or the table's edge.
///
/// A cell that fits on one row is left to the ordinary reading, so a table
/// changes nothing until it actually wraps something.
fn table_cell_at<T: EventListener>(
    term: &Term<T>,
    click: Point,
) -> Option<(String, Vec<Point>, usize)> {
    let grid = term.grid();
    let cols = term.columns();
    let is_border = |line: Line, col: usize| CELL_BORDERS.contains(&grid[line][Column(col)].c);
    if is_border(click.line, click.column.0) {
        return None;
    }
    let left = (0..click.column.0)
        .rev()
        .find(|&col| is_border(click.line, col))?;
    let right = (click.column.0 + 1..cols).find(|&col| is_border(click.line, col))?;

    // The written part of one row of the cell, padding trimmed off, or
    // nothing when that row is not part of this cell.
    let content = |line: Line| -> Option<(usize, usize)> {
        if !(is_border(line, left) && is_border(line, right)) {
            return None;
        }
        let written = |col: usize| {
            let cell = &grid[line][Column(col)];
            cell.c != ' ' || cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        };
        let first = (left + 1..right).find(|&col| written(col))?;
        let last = (left + 1..right).rev().find(|&col| written(col))?;
        Some((first, last))
    };
    let top = term.topmost_line();
    let bottom = term.bottommost_line();
    let is_link_char = |c: char| super::search::is_url_char(c);
    let continues = |line: Line| {
        line < bottom && {
            let next = line + 1;
            match (content(line), content(next)) {
                (Some((_, last)), Some((first, _))) => {
                    let head = grid[next][Column(first)].c;
                    // One column of padding either side is what every
                    // renderer leaves; a row that stops short of that ended
                    // on its own.
                    last + 2 >= right
                        && first <= left + 2
                        && is_link_char(grid[line][Column(last)].c)
                        && is_link_char(head)
                        && head != '@'
                }
                _ => false,
            }
        }
    };

    let (start_line, end_line) = run_of_rows(click.line, top, bottom, continues);
    if start_line == end_line {
        return None;
    }
    let rows = (start_line.0..=end_line.0).filter_map(|line| {
        let line = Line(line);
        let (first, last) = content(line)?;
        Some((line, Column(first), Column(last)))
    });
    read_rows(term, click, rows)
}

/// The rows `click` shares a line of text with: every row above that
/// `continues` into the next, and every row below that one continues into.
fn run_of_rows(
    click: Line,
    top: Line,
    bottom: Line,
    continues: impl Fn(Line) -> bool,
) -> (Line, Line) {
    let mut start_line = click;
    let mut guard = 0;
    while start_line > top && guard < MAX_WRAP_ROWS && continues(start_line - 1) {
        start_line -= 1;
        guard += 1;
    }
    let mut end_line = click;
    guard = 0;
    while end_line < bottom && guard < MAX_WRAP_ROWS && continues(end_line) {
        end_line += 1;
        guard += 1;
    }
    (start_line, end_line)
}

/// Reads the given stretch of each row, in order, as one string: its text,
/// the grid point of every character in it, and which of those is `click`.
fn read_rows<T: EventListener>(
    term: &Term<T>,
    click: Point,
    rows: impl Iterator<Item = (Line, Column, Column)>,
) -> Option<(String, Vec<Point>, usize)> {
    let grid = term.grid();
    let mut text = String::new();
    let mut points = Vec::new();
    let mut click_idx = None;
    for (line, from, to) in rows {
        for col in from.0..=to.0 {
            let cell = &grid[line][Column(col)];
            let p = Point::new(line, Column(col));
            if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                if p == click {
                    click_idx = Some(points.len());
                }
                continue;
            }
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                if p == click && !points.is_empty() {
                    click_idx = Some(points.len() - 1);
                }
                continue;
            }
            if p == click {
                click_idx = Some(points.len());
            }
            text.push(cell.c);
            points.push(p);
        }
    }
    let click_idx = click_idx.filter(|&i| i < points.len())?;
    Some((text, points, click_idx))
}

/// Restore an unquoted file path wrapped near the right edge of a TUI's text
/// area. A short row, a new rooted path, a style change, or prose ends the
/// continuation. This only reads grid cells; existence checks stay in the
/// shared asynchronous link resolver.
fn padded_file_line_at<T: EventListener>(
    term: &Term<T>,
    mut click: Point,
) -> Option<(String, Vec<Point>, usize)> {
    use super::search::{PathStyle, file_candidates_at, url_span_at};

    let cols = term.columns();
    if cols > MAX_FILE_WRAP_CELLS {
        return None;
    }
    let grid = term.grid();
    let wraps = |line: Line| grid[line][Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    let spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
    let path_char = |c: char| {
        c.is_alphanumeric()
            || matches!(
                c,
                '/' | '\\' | '.' | '_' | '-' | ':' | '#' | '~' | '+' | '%' | '@'
            )
    };
    // Inclusive end of this row, then start of the next. Inspect just the
    // margins here; only a plausible run pays for lifting full rows.
    let join = |line: Line| -> Option<(usize, usize)> {
        if line >= term.bottommost_line() {
            return None;
        }
        if wraps(line) {
            return Some((cols - 1, 0));
        }
        let end = (cols.saturating_sub(MAX_FILE_WRAP_PADDING + 2)..cols)
            .rev()
            .find(|&col| {
                let cell = &grid[line][Column(col)];
                !cell.c.is_whitespace() && !cell.flags.intersects(spacer)
            })?;
        let next = Line(line.0 + 1);
        let start = (0..cols.min(MAX_FILE_WRAP_PADDING + 1)).find(|&col| {
            let cell = &grid[next][Column(col)];
            !cell.c.is_whitespace() && !cell.flags.intersects(spacer)
        })?;
        let last = &grid[line][Column(end)];
        let first = &grid[next][Column(start)];
        let last_width = if last.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        if cols.saturating_sub(end + last_width) > MAX_FILE_WRAP_PADDING
            || !path_char(last.c)
            || !path_char(first.c)
            || matches!(first.c, '/' | '\\' | '~' | '@')
            || last.fg != first.fg
            || last.bg != first.bg
        {
            return None;
        }
        // A drive-qualified or explicitly relative path starts a new token.
        let second = (start + 1 < cols).then(|| grid[next][Column(start + 1)].c);
        let third = (start + 2 < cols).then(|| grid[next][Column(start + 2)].c);
        if (first.c.is_ascii_alphabetic() && second == Some(':'))
            || (first.c == '.'
                && (matches!(second, Some('/' | '\\'))
                    || (second == Some('.') && matches!(third, Some('/' | '\\')))))
        {
            return None;
        }
        Some((end, start))
    };

    if grid[click.line][click.column]
        .flags
        .contains(Flags::WIDE_CHAR_SPACER)
    {
        click.column.0 = click.column.0.checked_sub(1)?;
    }
    if grid[click.line][click.column].c.is_whitespace() {
        return None;
    }
    let mut start = click.line;
    let mut end = click.line;
    while start > term.topmost_line() && join(start - 1).is_some() {
        start -= 1;
        if (end.0 - start.0) as usize >= MAX_WRAP_ROWS
            || (end.0 - start.0 + 1) as usize * cols > MAX_FILE_WRAP_CELLS
        {
            return None;
        }
    }
    while join(end).is_some() {
        end += 1;
        if (end.0 - start.0) as usize >= MAX_WRAP_ROWS
            || (end.0 - start.0 + 1) as usize * cols > MAX_FILE_WRAP_CELLS
        {
            return None;
        }
    }
    if start == end || (start.0..end.0).all(|row| wraps(Line(row))) {
        return None;
    }

    let mut text = String::new();
    let mut points = Vec::new();
    let mut seams = Vec::new();
    for row in start.0..=end.0 {
        let line = Line(row);
        let first = if line == start { 0 } else { join(line - 1)?.1 };
        let last = if line == end { cols - 1 } else { join(line)?.0 };
        for col in first..=last {
            let cell = &grid[line][Column(col)];
            if !cell.flags.intersects(spacer) {
                text.push(cell.c);
                points.push(Point::new(line, Column(col)));
            }
        }
        if line < end && !wraps(line) {
            seams.push(points.len());
        }
    }
    let click_idx = points.iter().position(|&point| point == click)?;
    if url_span_at(&text, click_idx).is_some() {
        return None;
    }
    let spans_seams = file_candidates_at(&text, click_idx)
        .iter()
        .any(|candidate| {
            let file_name = candidate.path.rsplit(['/', '\\']).next().unwrap_or("");
            let has_extension = file_name.rsplit_once('.').is_some_and(|(name, ext)| {
                !name.is_empty() && !ext.is_empty() && ext.chars().all(char::is_alphanumeric)
            });
            candidate.looks_like_a_path(PathStyle::Windows)
            && (candidate.line.is_some() || has_extension)
            // Nearby prose may also fill its row. Only the seams inside the
            // pointed-at token matter, not every row inspected around it.
            && seams
                    .iter()
                    .any(|&seam| candidate.start < seam && candidate.end >= seam)
        });
    spans_seams.then_some((text, points, click_idx))
}

/// Enclosures prove that a path continues even across a short row or a wide
/// margin, where the bare-token heuristic deliberately stops.
fn enclosed_file_line_at<T: EventListener>(
    term: &Term<T>,
    mut click: Point,
) -> Option<(String, Vec<Point>, usize)> {
    let grid = term.grid();
    let cols = term.columns();
    let wraps = |line: Line| grid[line][Column(cols - 1)].flags.contains(Flags::WRAPLINE);
    let cells = |line: Line| -> Vec<(char, Point)> {
        (0..cols)
            .filter_map(|col| {
                let cell = &grid[line][Column(col)];
                (!cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER))
                .then_some((cell.c, Point::new(line, Column(col))))
            })
            .collect()
    };
    let closing = |open: char| {
        BRACKET_PAIRS
            .iter()
            .find_map(|&(left, right)| (left == open).then_some(right))
            .or_else(|| SYMMETRIC_QUOTES.contains(&open).then_some(open))
    };
    if grid[click.line][click.column]
        .flags
        .contains(Flags::WIDE_CHAR_SPACER)
    {
        click.column.0 = click.column.0.checked_sub(1)?;
    }
    if grid[click.line][click.column].c.is_whitespace() {
        return None;
    }

    // Find the opener in the current token or a preceding row's tail. A
    // whitespace break inside a row is never a continuation.
    let mut line = click.line;
    let mut col = click.column.0;
    let (start, open, close) = loop {
        let row = cells(line);
        let at = row.iter().rposition(|&(c, p)| {
            p.column.0 <= col && (line == click.line || wraps(line) || !c.is_whitespace())
        })?;
        if row[at].0.is_whitespace() {
            return None;
        }
        let first = row[..at]
            .iter()
            .rposition(|&(c, _)| c.is_whitespace())
            .map_or(0, |i| i + 1);
        if let Some(close) = closing(row[first].0) {
            break (row[first].1, row[first].0, close);
        }
        if row[..first].iter().any(|&(c, _)| !c.is_whitespace())
            || line == term.topmost_line()
            || (click.line.0 - line.0) as usize >= MAX_WRAP_ROWS
            || (first > 0 && wraps(line - 1))
        {
            return None;
        }
        line -= 1;
        col = cols - 1;
    };

    let mut text = String::new();
    let mut points = Vec::new();
    let mut depth = 0;
    let mut crossed_hard_break = false;
    line = start.line;
    col = start.column.0;
    loop {
        let row = cells(line);
        let first = row.iter().position(|&(_, p)| p.column.0 >= col)?;
        for (i, &(c, point)) in row.iter().enumerate().skip(first) {
            if c.is_whitespace() {
                if wraps(line) || row[i..].iter().any(|&(c, _)| !c.is_whitespace()) {
                    return None;
                }
                break;
            }
            text.push(c);
            points.push(point);
            if c == close && points.len() > 1 {
                depth -= 1;
                if depth == 0 {
                    if !crossed_hard_break {
                        return None;
                    }
                    let click_idx = points.iter().position(|&p| p == click)?;
                    // URL authorities retain the existing, stricter
                    // hard-break rules; this fallback is for file paths only.
                    if super::search::url_span_at(&text, 1).is_some() {
                        return None;
                    }
                    let candidate = super::search::file_candidates_at(&text, click_idx)
                        .into_iter()
                        .next()?;
                    if !candidate.looks_like_a_path(super::search::PathStyle::Windows) {
                        return None;
                    }
                    // Keep location prose and suffixes following the closer
                    // available to the shared file parser.
                    for &(c, point) in &row[i + 1..] {
                        text.push(c);
                        points.push(point);
                    }
                    return Some((text, points, click_idx));
                }
            } else if c == open {
                depth += 1;
            }
        }
        if line == term.bottommost_line() || (line.0 - start.line.0) as usize >= MAX_WRAP_ROWS {
            return None;
        }
        let hard = !wraps(line);
        crossed_hard_break |= hard;
        line += 1;
        col = if hard {
            cells(line)
                .iter()
                .find(|&&(c, _)| !c.is_whitespace())?
                .1
                .column
                .0
        } else {
            0
        };
    }
}

/// The cells `points` covers, as one run per row — what an underline has to
/// paint when those points are not one stretch of the grid. A wide glyph at
/// the end of a run takes its spacer column along.
pub(super) fn row_runs<T: EventListener>(term: &Term<T>, points: &[Point]) -> Vec<(Point, Point)> {
    let mut runs: Vec<(Point, Point)> = Vec::new();
    for &p in points {
        match runs.last_mut() {
            Some((_, end)) if end.line == p.line => *end = p,
            _ => runs.push((p, p)),
        }
    }
    let grid = term.grid();
    for (_, end) in &mut runs {
        let next = end.column + 1;
        if next.0 < term.columns() && grid[end.line][next].flags.contains(Flags::WIDE_CHAR_SPACER) {
            *end = Point::new(end.line, next);
        }
    }
    runs
}

/// Every cell from `start` to `end` in reading order, as one run per row.
pub(super) fn grid_runs(start: Point, end: Point, cols: usize) -> Vec<(Point, Point)> {
    (start.line.0..=end.line.0)
        .map(|line| {
            let line = Line(line);
            let from = if line == start.line {
                start.column
            } else {
                Column(0)
            };
            let to = if line == end.line {
                end.column
            } else {
                Column(cols.saturating_sub(1))
            };
            (Point::new(line, from), Point::new(line, to))
        })
        .collect()
}

pub(super) fn pair_range(chars: &[char], click: usize) -> Option<(usize, usize)> {
    bracket_range(chars, click).or_else(|| quote_range(chars, click))
}

fn is_contraction(chars: &[char], i: usize) -> bool {
    if chars[i] != '\'' {
        return false;
    }
    let flanked = |j: Option<usize>| {
        j.and_then(|j| chars.get(j))
            .is_some_and(|c| c.is_alphanumeric())
    };
    flanked(i.checked_sub(1)) && flanked(Some(i + 1))
}

pub(super) fn quote_range(chars: &[char], click: usize) -> Option<(usize, usize)> {
    let q = *chars.get(click)?;
    if !SYMMETRIC_QUOTES.contains(&q) || is_contraction(chars, click) {
        return None;
    }
    let quote_at = |i: usize| chars[i] == q && !is_contraction(chars, i);
    let before = (0..click).filter(|&i| quote_at(i)).count();
    if before % 2 == 0 {
        let close = (click + 1..chars.len()).find(|&i| quote_at(i))?;
        Some((click, close))
    } else {
        let open = (0..click).rev().find(|&i| quote_at(i))?;
        Some((open, click))
    }
}

fn pair_is_plausible(chars: &[char], open: char, s: usize, e: usize) -> bool {
    if open != '<' {
        return true;
    }
    e > s + 1 && !chars[s + 1].is_whitespace() && !chars[e - 1].is_whitespace()
}

pub(super) fn bracket_range(chars: &[char], click: usize) -> Option<(usize, usize)> {
    let c = *chars.get(click)?;
    if let Some((open, close)) = BRACKET_PAIRS.iter().find(|(o, _)| *o == c) {
        let mut depth = 0usize;
        for (i, &ch) in chars.iter().enumerate().skip(click) {
            if ch == *open {
                depth += 1;
            } else if ch == *close {
                depth -= 1;
                if depth == 0 {
                    return pair_is_plausible(chars, *open, click, i).then_some((click, i));
                }
            }
        }
        return None;
    }
    if let Some((open, close)) = BRACKET_PAIRS.iter().find(|(_, c2)| *c2 == c) {
        let mut depth = 0usize;
        for i in (0..=click).rev() {
            let ch = chars[i];
            if ch == *close {
                depth += 1;
            } else if ch == *open {
                depth -= 1;
                if depth == 0 {
                    return pair_is_plausible(chars, *open, i, click).then_some((i, click));
                }
            }
        }
    }
    None
}

fn regexes() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        [
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}",
            r"\b[0-9]+(?:\.[0-9]+)?[eE][+-]?[0-9]+\b",
            r"[A-Za-z0-9._+@%~-]*(?:/[A-Za-z0-9._+@%~-]+)+/?",
            r"[0-9A-Za-z_]+(?:[.-][0-9A-Za-z_]+)*",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static smart-select regex"))
        .collect()
    })
}

pub(super) fn smart_range(
    text: &str,
    chars: &[char],
    click: usize,
    separators: &str,
) -> Option<(usize, usize)> {
    if click >= chars.len() || chars[click].is_whitespace() {
        return None;
    }

    let boundary = |c: char| c.is_whitespace() || separators.contains(c);
    let (mut pws, mut pwe) = (click, click);
    if !boundary(chars[click]) {
        while pws > 0 && !boundary(chars[pws - 1]) {
            pws -= 1;
        }
        while pwe + 1 < chars.len() && !boundary(chars[pwe + 1]) {
            pwe += 1;
        }
    }
    let (ws, we) = narrow_to_script(chars, click, pws, pwe);
    let extends = |s: usize, e: usize| s <= ws && e >= we && (s < ws || e > we);

    if let Some((s, e, _url)) = super::search::url_span_at(text, click)
        && extends(s, e)
    {
        return Some((s, e));
    }

    let byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    let w_start = click.saturating_sub(MATCH_WINDOW);
    let w_end = (click + MATCH_WINDOW).min(chars.len() - 1);
    let wb_start = byte_of[w_start];
    let wb_end = byte_of[w_end] + chars[w_end].len_utf8();
    let window = &text[wb_start..wb_end];
    let click_byte = byte_of[click] - wb_start;

    for re in regexes() {
        let Some(m) = re
            .find_iter(window)
            .find(|m| m.range().contains(&click_byte))
        else {
            continue;
        };
        let s = text[..wb_start + m.start()].chars().count();
        let e = text[..wb_start + m.end()].chars().count() - 1;
        if extends(s, e) {
            return Some((s, e));
        }
    }
    ((ws, we) != (pws, pwe)).then_some((ws, we))
}

pub(super) fn narrow_to_script(
    chars: &[char],
    click: usize,
    lo: usize,
    hi: usize,
) -> (usize, usize) {
    let class = is_cjk(chars[click]);
    let mut s = click;
    while s > lo && is_cjk(chars[s - 1]) == class {
        s -= 1;
    }
    let mut e = click;
    while e < hi && is_cjk(chars[e + 1]) == class {
        e += 1;
    }
    (s, e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;

    const SEPS: &str = ",│`|:\"' ()[]{}<>\t";

    fn ensure_segmenter() {
        #[cfg(not(target_os = "macos"))]
        let _ = JIEBA.get_or_init(jieba_rs::Jieba::new);
    }

    fn range(text: &str, click: usize) -> Option<(usize, usize)> {
        let chars: Vec<char> = text.chars().collect();
        smart_range(text, &chars, click, SEPS)
    }

    fn term_with(cols: usize, rows: usize, input: &str) -> Term<VoidListener> {
        let config = alacritty_terminal::term::Config {
            semantic_escape_chars: SEPS.to_string(),
            ..Default::default()
        };
        let mut term = Term::new(
            config,
            &crate::terminal::size::TermSize::new(cols, rows),
            VoidListener,
        );
        let mut parser: alacritty_terminal::vte::ansi::Processor =
            alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut term, input.as_bytes());
        term
    }

    fn grid_select(term: &Term<VoidListener>, line: i32, col: usize) -> Option<String> {
        let r = grid_smart_range(term, Point::new(Line(line), Column(col)))?;
        Some(term.bounds_to_string(r.start, r.end))
    }

    fn col_of(row: &str, needle: &str) -> usize {
        row.find(needle).expect("needle in fixture")
    }

    #[test]
    fn osc8_hyperlink_selects_the_declared_extent_not_the_visible_word() {
        let term = term_with(
            40,
            3,
            "go \x1b]8;;https://example.com/x\x1b\\click here\x1b]8;;\x1b\\ now",
        );
        let line = "go click here now";
        assert_eq!(
            grid_select(&term, 0, col_of(line, "here")).as_deref(),
            Some("click here"),
        );
        assert_ne!(
            grid_select(&term, 0, col_of(line, "now")).as_deref(),
            Some("click here"),
        );
    }

    #[test]
    fn osc8_hyperlink_follows_a_soft_wrap() {
        let term = term_with(
            20,
            4,
            "\x1b]8;;https://e.com\x1b\\aaaaaaaaaabbbbbbbbbbcccccccccc\x1b]8;;\x1b\\",
        );
        let whole = "aaaaaaaaaabbbbbbbbbbcccccccccc";
        assert_eq!(grid_select(&term, 1, 2).as_deref(), Some(whole));
        assert_eq!(grid_select(&term, 0, 3).as_deref(), Some(whole));
    }

    #[test]
    fn soft_wrapped_url_is_stitched_back_into_one_selection() {
        let term = term_with(20, 4, "see https://example.com/deep/path here");
        let whole = "https://example.com/deep/path";
        assert_eq!(
            grid_select(&term, 0, col_of("see https://example", "example")).as_deref(),
            Some(whole),
        );
        assert_eq!(
            grid_select(&term, 1, col_of("com/deep/path here", "deep")).as_deref(),
            Some(whole),
        );
    }

    /// A path the terminal itself wrapped is one path, from either side of
    /// the seam and however many rows it takes.
    #[test]
    fn a_soft_wrapped_file_path_is_one_link() {
        let dir = std::env::temp_dir().join(format!("tty7-wrap-{}", std::process::id()));
        let make = |rel: &str| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("create dirs");
            std::fs::write(&path, b"x").expect("create file");
            path
        };
        let ascii = make("a/bb/ccc/dddd/notes.md");
        let cjk = make("文档/子目录/笔记.md");
        let deep = make("aaaa/bbbb/cccc/dddd/eeee/ffff/notes.md");
        let roots = crate::terminal::search::LinkRoots::local(vec![dir.clone()]);

        let resolved = |input: &str, line: i32, col: usize| {
            let term = term_with(20, 5, input);
            let click = Point::new(Line(line), Column(col));
            let (text, _points, idx) = logical_line_at(&term, click, true)?;
            let link = crate::terminal::search::link_at(
                &text,
                idx,
                &roots,
                true,
                &mut crate::terminal::search::local_probe,
            )?;
            match link.target {
                crate::terminal::search::LinkTarget::File { path, .. } => Some(path),
                crate::terminal::search::LinkTarget::Url(_) => None,
            }
        };

        // 20 columns, so each of these runs past the right edge.
        let line = "see a/bb/ccc/dddd/notes.md here";
        for (row, col, where_) in [
            (0, 6, "before the seam"),
            (0, 19, "the last cell of the first row"),
            (1, 0, "the first cell of the second row"),
            (1, 2, "after the seam"),
        ] {
            assert_eq!(
                resolved(line, row, col).as_deref(),
                Some(ascii.as_path()),
                "{where_} is the same link"
            );
        }

        assert_eq!(
            resolved("see 文档/子目录/笔记.md here", 1, 1).as_deref(),
            Some(cjk.as_path()),
            "a wide-character path wraps like any other"
        );
        assert_eq!(
            resolved("at aaaa/bbbb/cccc/dddd/eeee/ffff/notes.md end", 1, 10).as_deref(),
            Some(deep.as_path()),
            "and one that takes three rows is still one path"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A short row outside the bounded TUI margin is still an independent
    /// line; a following filename alone does not prove a continuation.
    #[test]
    fn a_newline_short_of_the_right_edge_is_not_a_wrap() {
        let dir = std::env::temp_dir().join(format!("tty7-nowrap-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/bb/ccc/dddd")).expect("create dirs");
        std::fs::write(dir.join("a/bb/ccc/dddd/notes.md"), b"x").expect("create file");
        let roots = crate::terminal::search::LinkRoots::local(vec![dir.clone()]);

        let term = term_with(40, 5, "see a/bb/ccc/dddd/\r\nnotes.md here");
        let click = Point::new(Line(0), Column(6));
        let (text, _points, idx) = logical_line_at(&term, click, true).expect("logical line");
        let link = crate::terminal::search::link_at(
            &text,
            idx,
            &roots,
            true,
            &mut crate::terminal::search::local_probe,
        )
        .expect("the directory on the first row still resolves");
        match link.target {
            crate::terminal::search::LinkTarget::File { path, is_dir, .. } => {
                assert_eq!(path, dir.join("a/bb/ccc/dddd"));
                assert!(is_dir, "the first row on its own names a directory");
            }
            crate::terminal::search::LinkTarget::Url(url) => panic!("expected a file, got {url}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enclosed_file_links_keep_the_absolute_path_across_padded_hard_breaks() {
        use crate::terminal::search::{LinkRoots, LinkTarget, PathStyle, Probe, link_at};

        let roots = LinkRoots {
            dirs: vec!["D:/workspace/code/backend/risk-insight".into()],
            local_home: false,
            style: PathStyle::Windows,
        };
        // Pin the pane's Windows spelling on every runner. Path equality
        // treats `/` and `\` as equivalent only on Windows, so compare strings.
        for (input, expected) in [
            (
                "手机分布卡 (D:/workspace/code/frontend/risk-insight-ui/\r\n  output/playwright/overview-redesign/overview-redesign-mobile-cards.png)",
                r"D:\workspace\code\frontend\risk-insight-ui\output\playwright\overview-redesign\overview-redesign-mobile-cards.png",
            ),
            (
                "查看 (D:/\r\n  workspace/code/frontend/risk-insight-ui/output/\r\n  playwright/overview-redesign/overview-redesign-mobile-cards.png)",
                r"D:\workspace\code\frontend\risk-insight-ui\output\playwright\overview-redesign\overview-redesign-mobile-cards.png",
            ),
            (
                "查看 (D:\\项目\\报告\\\r\n  截图(最终版).png)",
                r"D:\项目\报告\截图(最终版).png",
            ),
        ] {
            let term = term_with(100, 6, input);
            for row in 0..input.split("\r\n").count() {
                let col = if row == 0 {
                    (0..100)
                        .find(|&col| term.grid()[Line(0)][Column(col)].c == 'D')
                        .unwrap()
                } else {
                    3
                };
                let click = Point::new(Line(row as i32), Column(col));
                let (text, points, idx) = logical_line_at(&term, click, true).unwrap();
                let link = link_at(&text, idx, &roots, true, &mut |path, _| {
                    assert_eq!(
                        path.to_str(),
                        Some(expected),
                        "unexpected resolved path: {text}"
                    );
                    Probe::Hit { is_dir: false }
                })
                .expect("file link from every row");
                assert!(
                    matches!(link.target, LinkTarget::File { ref path, .. } if path.to_str() == Some(expected))
                );
                assert_eq!(points[link.start].line, Line(0));
                assert_eq!(
                    points[link.end].line,
                    Line(input.split("\r\n").count() as i32 - 1)
                );
            }
        }
    }

    #[test]
    fn padded_hard_breaks_do_not_join_separate_links_or_plain_output() {
        for input in [
            "see (D:/reports/one.png)\r\n  output/two.png",
            "see (D:/reports/\r\n\r\n  two.png)",
            "see (D:/reports/\r\n  unrelated output/two.png)",
            "see D:/reports/\r\n  output/two.png",
            "see (https://good.com\r\n  @evil.com/path)",
            "see (D:/reports/\r\n  two.png]",
            "see (D:/reports/\r\n  two.png",
        ] {
            let term = term_with(80, 5, input);
            let (text, _, _) =
                logical_line_at(&term, Point::new(Line(0), Column(6)), true).unwrap();
            assert!(!text.contains("two.png"), "unrelated rows joined: {text}");
            assert!(!text.contains("evil.com"), "URL authority changed: {text}");
        }
    }

    #[test]
    fn bare_file_links_cross_tui_margins_from_every_path_cell() {
        use super::super::search::{LinkRoots, LinkTarget, PathStyle, Probe, link_at};
        use unicode_width::UnicodeWidthStr;

        let roots = LinkRoots {
            dirs: vec!["D:/workspace/code/backend/tty7".into()],
            style: PathStyle::Windows,
            local_home: false,
        };
        // Include the resolved root in the relative case's expected spelling,
        // independently of the path resolver being exercised.
        for (head, tail, token, expected, line, column) in [
            (
                "修改位于 D:/workspace/code/frontend/risk-insight-ui/src/views/dashboard/overview/components/",
                "KpiStrip.vue:262，文件级检查已通过。",
                "D:/workspace/code/frontend/risk-insight-ui/src/views/dashboard/overview/components/",
                r"D:\workspace\code\frontend\risk-insight-ui\src\views\dashboard\overview\components\KpiStrip.vue",
                262,
                None,
            ),
            (
                "查看 D:/项目/报",
                "告/main.rs:12。说明",
                "D:/项目/报",
                r"D:\项目\报告\main.rs",
                12,
                None,
            ),
            (
                "见 src/terminal/smart_se",
                "lect.rs:880。当时这个限制",
                "src/terminal/smart_se",
                r"D:\workspace\code\backend\tty7\src\terminal\smart_select.rs",
                880,
                None,
            ),
            (
                "查看 D:\\项目\\报告\\",
                "说明.rs:12:3。已修复",
                "D:\\项目\\报告\\",
                r"D:\项目\报告\说明.rs",
                12,
                Some(3),
            ),
        ] {
            for padding in [1, 4, MAX_FILE_WRAP_PADDING] {
                for indent in [0, 2, MAX_FILE_WRAP_PADDING] {
                    let cols = head.width() + padding;
                    let input = format!("{head}\r\n{}{tail}", " ".repeat(indent));
                    let term = term_with(cols, 8, &input);
                    let click =
                        Point::new(Line(0), Column(head[..head.find(token).unwrap()].width()));
                    let (text, points, idx) = logical_line_at(&term, click, true).unwrap();
                    let mut probe = |path: &std::path::Path, require_file: bool| {
                        assert_eq!(
                            path.to_str(),
                            Some(expected),
                            "a directory prefix or relative tail escaped: {text}"
                        );
                        assert!(require_file);
                        Probe::Hit { is_dir: false }
                    };
                    let link = link_at(&text, idx, &roots, true, &mut probe).unwrap();
                    assert!(points[link.end].line > points[link.start].line);
                    for point in &points[link.start..=link.end] {
                        let wide = term.grid()[point.line][point.column]
                            .flags
                            .contains(Flags::WIDE_CHAR);
                        for extra in 0..=usize::from(wide) {
                            let click = Point::new(point.line, Column(point.column.0 + extra));
                            let (text, _, idx) = logical_line_at(&term, click, true).unwrap();
                            let link = link_at(&text, idx, &roots, true, &mut probe).unwrap();
                            assert!(matches!(link.target, LinkTarget::File {
                                path, line: Some(actual_line), column: actual_column, is_dir: false
                            } if path.to_str() == Some(expected) && actual_line == line && actual_column == column));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bare_file_links_keep_multiple_hard_breaks_and_soft_wraps() {
        use super::super::search::{PathStyle, unresolved_candidate};

        for cols in [26, 30, 34] {
            let term = term_with(
                cols,
                10,
                "see /srv/abcdefghijklmnop/\r\n    qrstuvwxyzabcdefghijk/\r\n  main.rs:12:3。后续",
            );
            let click = Point::new(Line(0), Column(5));
            let (text, points, idx) = logical_line_at(&term, click, true).unwrap();
            let candidate = unresolved_candidate(&text, idx, PathStyle::Posix).unwrap();
            assert_eq!(
                candidate.path,
                "/srv/abcdefghijklmnop/qrstuvwxyzabcdefghijk/main.rs"
            );
            assert_eq!(candidate.line, Some(12));
            assert_eq!(candidate.column, Some(3));
            assert_eq!(points[candidate.end].line, Line(2));
        }

        let head = "see /srv/project/very-long-directory/";
        let term = term_with(20, 6, &format!("{head}\r\n  src/main.rs:12。说明"));
        let click = Point::new(Line(0), Column(5));
        let (text, _, idx) = logical_line_at(&term, click, true).unwrap();
        let candidate = unresolved_candidate(&text, idx, PathStyle::Posix).unwrap();
        assert_eq!(
            candidate.path,
            "/srv/project/very-long-directory/src/main.rs"
        );
        assert_eq!(candidate.line, Some(12));
    }

    #[test]
    fn bare_file_wraps_reject_independent_rows_and_respect_scan_limits() {
        for tail in [
            "  /other/file.rs:8",
            "  D:/other/file.rs:8",
            "  ./file.rs:8",
            "  ../file.rs:8",
            "  @evil.com/x",
            "",
            "  unrelated output/file.rs:8",
        ] {
            let head = "see /srv/project/";
            let term = term_with(head.len() + 3, 6, &format!("{head}\r\n{tail}"));
            assert!(
                padded_file_line_at(&term, Point::new(Line(0), Column(5))).is_none(),
                "{tail}"
            );
        }
        let term = term_with(24, 4, "\x1b[32msee /srv/project/\r\n\x1b[31m  main.rs:12");
        assert!(padded_file_line_at(&term, Point::new(Line(0), Column(5))).is_none());
        let term = term_with(28, 4, "see https://good.com/path\r\n  more/path.rs:12");
        assert!(padded_file_line_at(&term, Point::new(Line(0), Column(5))).is_none());

        let row = format!("{}/\r\n", "a".repeat(1015));
        let term = term_with(1024, 8, &format!("/srv/{}main.rs:12", row.repeat(5)));
        assert!(padded_file_line_at(&term, Point::new(Line(0), Column(2))).is_none());
        let row = format!("{}/\r\n", "a".repeat(15));
        let term = term_with(24, 40, &format!("/srv/{}main.rs:12", row.repeat(34)));
        assert!(padded_file_line_at(&term, Point::new(Line(0), Column(2))).is_none());
    }

    #[test]
    fn prose_on_adjacent_full_rows_does_not_hide_a_wrapped_file() {
        use super::super::search::{PathStyle, unresolved_candidate};

        let term = term_with(
            30,
            6,
            "previous output aaaaaaaaaaaa\r\nsee /srv/abcdefghijklmnop/\r\n  main.rs:12 explanationxxx\r\nmore prose follows",
        );
        for click in [
            Point::new(Line(1), Column(5)),
            Point::new(Line(2), Column(3)),
        ] {
            let (text, points, idx) = logical_line_at(&term, click, true).unwrap();
            let candidate = unresolved_candidate(&text, idx, PathStyle::Posix).unwrap();
            assert_eq!(candidate.path, "/srv/abcdefghijklmnop/main.rs");
            assert_eq!(candidate.line, Some(12));
            assert_eq!(points[candidate.start].line, Line(1));
            assert_eq!(points[candidate.end].line, Line(2));
        }
    }

    /// A path that simply ends at the margin is bridged as well, since the
    /// grid cannot tell it from one wrapped mid-name. The row as printed comes
    /// back beside the bridge for the resolver to fall back on; a line that
    /// nothing bridged has no second reading.
    #[test]
    fn a_bridged_bare_path_keeps_the_row_as_printed() {
        let head = "  updated src/host_ops.rs";
        let output = format!("{head}\r\n  and the cache");
        let click = Point::new(Line(0), Column(head.find("host_ops").unwrap()));

        let term = term_with(head.len() + 4, 6, &output);
        let ((bridged, _, _), wrapped) = link_lines_at(&term, click).unwrap();
        assert!(bridged.contains("host_ops.rsand"), "{bridged}");
        let (row, points, idx) = wrapped.expect("the row as printed");
        assert_eq!(row.trim_end(), head);
        assert_eq!(points[idx], click);

        let term = term_with(head.len() + MAX_FILE_WRAP_PADDING + 2, 6, &output);
        let ((row, _, _), wrapped) = link_lines_at(&term, click).unwrap();
        assert_eq!(row.trim_end(), head);
        assert!(wrapped.is_none());
    }

    #[test]
    fn enclosed_file_links_support_quotes_mixed_wraps_and_line_locations() {
        use crate::terminal::search::{LinkRoots, LinkTarget, PathStyle, Probe, link_at};

        let roots = LinkRoots {
            style: PathStyle::Posix,
            ..Default::default()
        };
        for (open, close) in [('(', ')'), ('[', ']'), ('"', '"'), ('\'', '\''), ('`', '`')] {
            for cols in [23, 40, 100] {
                let input = format!(
                    "see {open}/srv/project/very-long-directory/\r\n  src/main.rs:12:3{close} done"
                );
                let term = term_with(cols, 12, &input);
                for row in 0..12 {
                    for col in 0..cols {
                        let click = Point::new(Line(row), Column(col));
                        let Some((text, points, idx)) = enclosed_file_line_at(&term, click) else {
                            continue;
                        };
                        let link = link_at(&text, idx, &roots, true, &mut |path, _| {
                            assert_eq!(
                                path,
                                std::path::Path::new(
                                    "/srv/project/very-long-directory/src/main.rs"
                                )
                            );
                            Probe::Hit { is_dir: false }
                        })
                        .unwrap();
                        assert!(matches!(
                            link.target,
                            LinkTarget::File {
                                line: Some(12),
                                column: Some(3),
                                ..
                            }
                        ));
                        assert!(points[link.end].line > points[link.start].line);
                    }
                }
                let (text, _, idx) =
                    logical_line_at(&term, Point::new(Line(0), Column(6)), true).unwrap();
                let candidate =
                    crate::terminal::search::unresolved_candidate(&text, idx, PathStyle::Posix)
                        .unwrap();
                assert_eq!(
                    candidate.path,
                    "/srv/project/very-long-directory/src/main.rs"
                );
            }
        }

        let term = term_with(80, 4, "see (/srv/project/\r\n  src/main.rs), line 12");
        let click = Point::new(Line(0), Column(6));
        let (text, _, idx) = logical_line_at(&term, click, true).unwrap();
        let link = link_at(&text, idx, &roots, true, &mut |_, _| Probe::Hit {
            is_dir: false,
        })
        .unwrap();
        assert!(matches!(
            link.target,
            LinkTarget::File { line: Some(12), .. }
        ));
        let (selected, _, _) = logical_line_at(&term, click, false).unwrap();
        assert!(
            !selected.contains("main.rs"),
            "double-click keeps hard line boundaries"
        );
    }

    #[test]
    fn hard_wrapped_url_is_bridged_only_for_links() {
        let term = term_with(20, 4, "https://example.com/\r\ndeep/path/seg rest");
        assert!(
            !term.grid()[Line(0)][Column(19)]
                .flags
                .contains(Flags::WRAPLINE),
            "fixture must be a hard newline, not a soft wrap"
        );

        let click = Point::new(Line(0), Column(3));
        let (text, _points, _idx) =
            logical_line_at(&term, click, true).expect("logical line under click");
        let idx = text.find("https").expect("url in bridged line");
        let (_s, _e, url) =
            crate::terminal::search::url_span_at(&text, idx + 2).expect("url span in bridged line");
        assert_eq!(url, "https://example.com/deep/path/seg");

        let (text, _points, _idx) =
            logical_line_at(&term, click, false).expect("logical line under click");
        assert!(
            !text.contains("deep"),
            "double-click must not bridge a hard newline: {text:?}"
        );
    }

    #[test]
    fn a_hard_break_before_userinfo_is_never_bridged() {
        let term = term_with(20, 4, "go1 https://good.com\r\n@evil.com/x rest");
        assert!(
            !term.grid()[Line(0)][Column(19)]
                .flags
                .contains(Flags::WRAPLINE),
            "fixture must be a hard newline, not a soft wrap"
        );

        let click = Point::new(Line(0), Column(8));
        let (text, _points, idx) =
            logical_line_at(&term, click, true).expect("logical line under click");
        assert!(
            !text.contains("evil"),
            "a hard break before `@` must not bridge: {text:?}"
        );
        let (_s, _e, url) =
            crate::terminal::search::url_span_at(&text, idx).expect("url span under click");
        assert_eq!(url, "https://good.com");
    }

    #[test]
    fn a_soft_wrap_before_userinfo_still_stitches() {
        let term = term_with(20, 4, "see https://user1234@ex.com/z rest");
        assert!(
            term.grid()[Line(0)][Column(19)]
                .flags
                .contains(Flags::WRAPLINE),
            "fixture must be a soft wrap, not a hard newline"
        );

        let click = Point::new(Line(0), Column(10));
        let (text, _points, idx) =
            logical_line_at(&term, click, true).expect("logical line under click");
        let (_s, _e, url) =
            crate::terminal::search::url_span_at(&text, idx).expect("url span under click");
        assert_eq!(url, "https://user1234@ex.com/z");
    }

    /// A table renderer wraps a long cell by hand: every row is a hard
    /// newline, framed by borders, with the next cell's text in between.
    /// This is the shape Claude Code prints a markdown table in.
    const TABLE: [&str; 8] = [
        "┌───────────────────────────────┬──────────┐",
        "│ report.xlsx (https://s3.examp │ 10/2     │",
        "│ le.com/a/b.xlsx?Sig=xy%3D&Exp │ 20:13    │",
        "│ ires=179)（美妆）             │          │",
        "├───────────────────────────────┼──────────┤",
        "│ short https://a.com           │ 10/2     │",
        "│ tail.com                      │          │",
        "└───────────────────────────────┴──────────┘",
    ];

    const TABLE_URL: &str = "https://s3.example.com/a/b.xlsx?Sig=xy%3D&Expires=179";

    fn table_term() -> Term<VoidListener> {
        term_with(50, 8, &TABLE.join("\r\n"))
    }

    fn table_col(line: usize, needle: &str) -> usize {
        let row = TABLE[line];
        row[..row.find(needle).expect("needle in fixture")]
            .chars()
            .count()
    }

    fn table_link(term: &Term<VoidListener>, line: usize, col: usize) -> Option<String> {
        let click = Point::new(Line(line as i32), Column(col));
        let (text, _points, idx) = logical_line_at(term, click, true)?;
        crate::terminal::search::url_span_at(&text, idx).map(|(_, _, url)| url)
    }

    #[test]
    fn a_url_a_table_wrapped_inside_its_cell_is_one_link() {
        let term = table_term();
        for (line, needle, where_) in [
            (1, "https", "the row it starts on"),
            (2, "Exp", "the middle row"),
            (3, "ires", "the row it ends on"),
        ] {
            assert_eq!(
                table_link(&term, line, table_col(line, needle)).as_deref(),
                Some(TABLE_URL),
                "{where_} opens the whole URL"
            );
        }
    }

    #[test]
    fn a_table_cell_underlines_only_its_own_columns() {
        let term = table_term();
        let click = Point::new(Line(2), Column(4));
        let (text, points, idx) = logical_line_at(&term, click, true).expect("cell text");
        let (s, e, _) = crate::terminal::search::url_span_at(&text, idx).expect("a url");
        let runs: Vec<_> = row_runs(&term, &points[s..=e])
            .iter()
            .map(|(a, b)| (a.line.0, a.column.0, b.column.0))
            .collect();
        assert_eq!(
            runs,
            vec![(1, table_col(1, "https"), 30), (2, 2, 30), (3, 2, 9)]
        );
    }

    #[test]
    fn a_table_cell_that_stops_short_of_its_border_is_not_wrapped() {
        let term = table_term();
        assert_eq!(
            table_link(&term, 5, table_col(5, "a.com")).as_deref(),
            Some("https://a.com"),
            "the next row's text is its own line, not the URL's tail"
        );
    }

    #[test]
    fn double_click_does_not_read_across_table_rows() {
        let term = table_term();
        let click = Point::new(Line(2), Column(4));
        let (text, _points, _idx) = logical_line_at(&term, click, false).expect("row text");
        assert!(
            !text.contains("https"),
            "selection keeps to its row: {text:?}"
        );
    }

    #[test]
    fn wide_glyph_and_its_spacer_resolve_to_the_same_word() {
        ensure_segmenter();
        let term = term_with(40, 3, "run 北京欢迎你 done");
        let expected = grid_select(&term, 0, 4);
        assert_eq!(expected.as_deref(), Some("北京"), "click on 北");
        assert_eq!(
            grid_select(&term, 0, 5).as_deref(),
            expected.as_deref(),
            "spacer of 北"
        );
        assert_eq!(
            grid_select(&term, 0, 6).as_deref(),
            Some("北京"),
            "click on 京"
        );
        assert_eq!(
            grid_select(&term, 0, 7).as_deref(),
            Some("北京"),
            "spacer of 京"
        );
    }

    #[test]
    fn wide_glyph_wrapping_to_the_next_row_keeps_its_word_intact() {
        ensure_segmenter();
        let term = term_with(9, 4, "abcdefgh北京欢迎你");
        assert_eq!(grid_select(&term, 1, 0).as_deref(), Some("北京"));
    }

    #[test]
    fn click_past_the_last_column_yields_no_range() {
        let term = term_with(10, 2, "hello");
        assert!(grid_smart_range(&term, Point::new(Line(0), Column(10))).is_none());
        assert!(grid_smart_range(&term, Point::new(Line(0), Column(99))).is_none());
    }

    #[test]
    fn click_outside_the_grid_rows_yields_no_range() {
        let term = term_with(10, 2, "hello");
        assert!(grid_smart_range(&term, Point::new(Line(2), Column(0))).is_none());
        assert!(grid_smart_range(&term, Point::new(Line(9_000), Column(0))).is_none());
        assert!(grid_smart_range(&term, Point::new(Line(-1), Column(0))).is_none());
    }

    fn selected(text: &str, click: usize) -> Option<String> {
        let chars: Vec<char> = text.chars().collect();
        range(text, click).map(|(s, e)| chars[s..=e].iter().collect())
    }

    #[test]
    fn url_expands_past_scheme_colon() {
        let text = "fetch https://example.com/a/b?q=1 done";
        let click = text.find("example").unwrap();
        assert_eq!(
            selected(text, click).as_deref(),
            Some("https://example.com/a/b?q=1")
        );
    }

    #[test]
    fn url_trailing_comma_excluded() {
        let text = "see https://a.com/x, then";
        let click = text.find("a.com").unwrap();
        assert_eq!(selected(text, click).as_deref(), Some("https://a.com/x"));
    }

    #[test]
    fn email_only_fires_when_it_extends_the_word() {
        let text = "author:dev@example.com pushed";
        let click = text.find("example").unwrap();
        assert_eq!(range(text, click), None);
        let chars: Vec<char> = text.chars().collect();
        let got = smart_range(text, &chars, click, ",@:() ");
        let (s, e) = got.expect("email should match");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "dev@example.com");
    }

    #[test]
    fn plain_word_yields_none() {
        let text = "just some words";
        let click = text.find("some").unwrap();
        assert_eq!(range(text, click), None);
    }

    #[test]
    fn path_across_quote_boundary_stays_plain() {
        let text = "cat /usr/local/bin/tool";
        let click = text.find("local").unwrap();
        assert_eq!(range(text, click), None);
    }

    #[test]
    fn path_glued_to_colon_expands() {
        let text = "error:/tmp/x/y";
        let click = text.find("tmp").unwrap();
        assert_eq!(range(text, click), None);
    }

    #[test]
    fn whitespace_click_yields_none() {
        assert_eq!(range("a b", 1), None);
    }

    #[test]
    fn scientific_notation_with_custom_separators() {
        let text = "n = 6.02e+23 mol";
        let chars: Vec<char> = text.chars().collect();
        let click = text.find("02").unwrap();
        let got = smart_range(text, &chars, click, ",.+():");
        let (s, e) = got.expect("sci notation should match");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "6.02e+23");
    }

    #[test]
    fn identifier_with_custom_separators() {
        let text = "run foo-bar.baz now";
        let chars: Vec<char> = text.chars().collect();
        let click = text.find("bar").unwrap();
        let got = smart_range(text, &chars, click, ",.-():");
        let (s, e) = got.expect("identifier should match");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "foo-bar.baz");
    }

    #[test]
    fn latin_word_glued_directly_to_han_narrows_without_punctuation() {
        let text = "已合并到main分支";
        let chars: Vec<char> = text.chars().collect();
        let click = chars.iter().position(|&c| c == 'm').unwrap();
        let (s, e) = smart_range(text, &chars, click, SEPS).expect("narrowed word");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "main");
    }

    #[test]
    fn latin_word_glued_to_cjk_narrows_to_the_latin_run() {
        let text = "分支 worktree-feat-smart-select，已 rebase";
        let chars: Vec<char> = text.chars().collect();
        let click = chars.iter().position(|&c| c == 'w').unwrap() + 10;
        let (s, e) = smart_range(text, &chars, click, SEPS).expect("narrowed word");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "worktree-feat-smart-select");
    }

    #[test]
    fn symmetric_quotes_pair_by_parity() {
        let chars: Vec<char> = r#"echo 'a,b' "c d" x"#.chars().collect();
        assert_eq!(quote_range(&chars, 5), Some((5, 9)));
        assert_eq!(quote_range(&chars, 9), Some((5, 9)));
        assert_eq!(quote_range(&chars, 11), Some((11, 15)));
        assert_eq!(quote_range(&chars, 15), Some((11, 15)));
        let chars: Vec<char> = "say 'oops".chars().collect();
        assert_eq!(quote_range(&chars, 4), None);
        assert_eq!(quote_range(&chars, 1), None);
    }

    #[test]
    fn contraction_apostrophes_do_not_pair() {
        let chars: Vec<char> = "it's a test, isn't it".chars().collect();
        assert_eq!(quote_range(&chars, 2), None);
        assert_eq!(quote_range(&chars, 16), None);
        let text = "it's a test, isn't it";
        assert_eq!(range(text, 2), None);
    }

    #[test]
    fn contractions_do_not_skew_a_real_quote() {
        let chars: Vec<char> = "echo 'it isn't so' done".chars().collect();
        let open = 5;
        let close = chars.iter().rposition(|&c| c == '\'').unwrap();
        assert_eq!(quote_range(&chars, open), Some((open, close)));
        assert_eq!(quote_range(&chars, close), Some((open, close)));
    }

    #[test]
    fn trailing_apostrophe_still_closes() {
        let chars: Vec<char> = "the 'dogs' bark".chars().collect();
        assert_eq!(quote_range(&chars, 4), Some((4, 9)));
        assert_eq!(quote_range(&chars, 9), Some((4, 9)));
    }

    #[test]
    fn directional_cjk_quotes_pair_like_brackets() {
        let chars: Vec<char> = "他说“你好”了".chars().collect();
        assert_eq!(bracket_range(&chars, 2), Some((2, 5)));
        assert_eq!(bracket_range(&chars, 5), Some((2, 5)));
    }

    #[test]
    fn fullwidth_brackets_pair() {
        let chars: Vec<char> = "说（worktree 分支）好".chars().collect();
        let open = chars.iter().position(|&c| c == '（').unwrap();
        let close = chars.iter().position(|&c| c == '）').unwrap();
        assert_eq!(bracket_range(&chars, open), Some((open, close)));
        assert_eq!(bracket_range(&chars, close), Some((open, close)));
        let chars: Vec<char> = "书名《三体》完".chars().collect();
        assert_eq!(bracket_range(&chars, 2), Some((2, 5)));
    }

    #[test]
    fn bracket_forward_and_backward_with_nesting() {
        let chars: Vec<char> = "f(a(b)c) x".chars().collect();
        assert_eq!(bracket_range(&chars, 1), Some((1, 7)));
        assert_eq!(bracket_range(&chars, 7), Some((1, 7)));
        assert_eq!(bracket_range(&chars, 3), Some((3, 5)));
        assert_eq!(bracket_range(&chars, 0), None);
    }

    #[test]
    fn angle_brackets_pair_only_when_they_hug_their_contents() {
        for (text, want) in [
            ("let v: Vec<String> = x", "<String>"),
            ("<div class=\"row\">hi", "<div class=\"row\">"),
            ("usage: tty7 <command> [opts]", "<command>"),
            ("From: Jo <j@example.com> ok", "<j@example.com>"),
            ("map: HashMap<K, V> here", "<K, V>"),
        ] {
            let chars: Vec<char> = text.chars().collect();
            let click = chars.iter().position(|&c| c == '<').unwrap();
            let (s, e) = bracket_range(&chars, click).unwrap_or_else(|| panic!("{text}"));
            let got: String = chars[s..=e].iter().collect();
            assert_eq!(got, want, "{text}");
        }
        for text in [
            "if a < b then c > d",
            "awk '{ if ($1 > 100 && $2 < 5) print }'",
            "WHERE a < 10 AND b > 20",
            "empty <> pair",
        ] {
            let chars: Vec<char> = text.chars().collect();
            for (i, &c) in chars.iter().enumerate() {
                if c == '<' || c == '>' {
                    assert_eq!(bracket_range(&chars, i), None, "{text} at {i}");
                }
            }
        }
        for text in ["cargo build 2>&1 | tee out", "grep -rn foo src/ > /tmp/o"] {
            let chars: Vec<char> = text.chars().collect();
            for (i, &c) in chars.iter().enumerate() {
                if c == '<' || c == '>' {
                    assert_eq!(bracket_range(&chars, i), None, "{text} at {i}");
                }
            }
        }
    }

    #[test]
    fn unmatched_bracket_yields_none() {
        let chars: Vec<char> = "f(a".chars().collect();
        assert_eq!(bracket_range(&chars, 1), None);
    }

    #[test]
    fn cjk_segmentation_selects_a_dictionary_word_not_the_whole_run() {
        ensure_segmenter();
        let text = "run 北京欢迎你 done";
        let chars: Vec<char> = text.chars().collect();
        let click = chars.iter().position(|&c| c == '京').unwrap();
        let (s, e) = cjk_word_range(text, click).expect("segmented range");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "北京");
    }

    #[test]
    fn cjk_segmentation_survives_surrogate_pairs_before_the_click() {
        ensure_segmenter();
        for text in ["你好世界", "🙂 你好世界", "🙂🙂🙂 你好世界"] {
            let chars: Vec<char> = text.chars().collect();
            let click = chars.iter().position(|&c| c == '世').unwrap();
            let (s, e) = cjk_word_range(text, click).expect("segmented range");
            let sel: String = chars[s..=e].iter().collect();
            assert_eq!(sel, "世界", "{text:?} segmented wrong");
        }
    }

    #[test]
    fn cjk_punctuation_is_its_own_token() {
        ensure_segmenter();
        let text = "比赛，天气";
        let chars: Vec<char> = text.chars().collect();
        let click = chars.iter().position(|&c| c == '，').unwrap();
        let (s, e) = cjk_word_range(text, click).expect("segmented range");
        let sel: String = chars[s..=e].iter().collect();
        assert_eq!(sel, "，");
    }

    #[test]
    fn japanese_is_not_shredded_into_single_kana() {
        ensure_segmenter();
        let text = "日本語の文章です";
        let chars: Vec<char> = text.chars().collect();
        let click = chars.iter().position(|&c| c == 'で').unwrap();
        if let Some((s, e)) = cjk_word_range(text, click) {
            let sel: String = chars[s..=e].iter().collect();
            assert_eq!(sel, "です");
        }
    }

    #[test]
    fn is_cjk_covers_han_kana_hangul_fullwidth() {
        for c in ['中', 'あ', 'ア', '한', '，', '（'] {
            assert!(is_cjk(c), "{c} should be CJK");
        }
        for c in ['a', '1', '-', '/', 'é'] {
            assert!(!is_cjk(c), "{c} should not be CJK");
        }
    }
}
