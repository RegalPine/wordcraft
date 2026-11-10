//! The band the page display paints over the text each bookmark covers, and only on screen.

use crate::display::{BOOKMARK_COLOR, DisplayOptions, Draw, POINT_W, page_display};
#[allow(unused_imports)]
use crate::*;
use wordcraft_doc::Pos;
use wordcraft_doc::para::InlineObject;

/// A bookmark mark's width in a paragraph's text: one U+FFFC, three bytes.
const OBJ_W: usize = 3;

fn lay(doc: &Document) -> DocLayout {
    let mut c = LayoutCache::new();
    layout(doc, &mut c, &LayoutOptions::default())
}

/// Bookmark `name` over the body text from (`p0`,`o0`) to (`p1`,`o1`), the offsets counting the
/// paragraph's text as it is before this call adds its marks.
fn mark(d: &mut Document, p0: usize, o0: usize, p1: usize, o1: usize, name: &str) {
    let end = InlineObject::BookmarkEnd { name: name.into() };
    let start = InlineObject::BookmarkStart { name: name.into() };
    d.insert_object(&Pos::body(p1, o1), end, &Default::default()).unwrap();
    d.insert_object(&Pos::body(p0, o0), start, &Default::default()).unwrap();
}

/// How long paragraph `k` of the body is now, marks included.
fn text_len(d: &Document, k: usize) -> usize {
    d.para(StoryRef::Body, &Path::top(k)).unwrap().len()
}

/// The `k`th placed line of the first page: its paragraph layout, first line index and origin.
fn line(l: &DocLayout, k: usize) -> (&ParaLayout, usize, f32) {
    let (para, l0, x, _) = l.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Placed::Lines { para, l0, x, y, .. } => Some((para, *l0, *x, *y)),
            _ => None,
        })
        .nth(k)
        .unwrap();
    (&**para, l0, x)
}

/// The bands the first page draws with the option `on`, as (left, width).
fn bands(d: &Document, l: &DocLayout, on: bool) -> Vec<(f32, f32)> {
    let opts = DisplayOptions { bookmarks: on, ..Default::default() };
    page_display(d, &l.pages[0], &opts)
        .iter()
        .filter_map(|dr| match dr {
            Draw::Fill { rect, color, .. } if *color == BOOKMARK_COLOR => Some((rect.x, rect.w)),
            _ => None,
        })
        .collect()
}

/// Where a band should sit: the page x and width of the body paragraph `k`'s text `from..to`,
/// those offsets counting the marks in the paragraph as they are now.
fn span(l: &DocLayout, k: usize, from: usize, to: usize) -> (f32, f32) {
    let (pl, li, x) = line(l, k);
    let (a, z) = (x + pl.x_of(li, from).unwrap(), x + pl.x_of(li, to).unwrap());
    (a, z - a)
}

#[test]
fn a_bookmark_bands_the_text_it_covers() {
    crate::test_fonts();
    let mut d = Document::from_text("Hello brave world");
    mark(&mut d, 0, 6, 0, 11, "brave");
    let l = lay(&d);
    let b = bands(&d, &l, true);
    let (x, w) = span(&l, 0, 6 + OBJ_W, 11 + OBJ_W);
    assert_eq!(b.len(), 1, "one band, over the word the bookmark holds");
    assert!((b[0].0 - x).abs() < 0.5 && (b[0].1 - w).abs() < 0.5, "the band sits over the word, from {x} to {}: {b:?}", x + w);
    assert!(w > 20.0, "and as wide as the word: {w}");
    assert!(bands(&d, &l, false).is_empty(), "off, nothing is painted: the marks come from the file");
}

/// The band goes under the letters, so bookmarked text stays readable.
#[test]
fn the_band_is_drawn_under_the_letters() {
    crate::test_fonts();
    let mut d = Document::from_text("Hello brave world");
    mark(&mut d, 0, 6, 0, 11, "brave");
    let l = lay(&d);
    let opts = DisplayOptions { bookmarks: true, ..Default::default() };
    let draws = page_display(&d, &l.pages[0], &opts);
    let band = draws.iter().position(|dr| matches!(dr, Draw::Fill { color, .. } if *color == BOOKMARK_COLOR)).unwrap();
    let letters = draws.iter().position(|dr| matches!(dr, Draw::Glyphs { .. })).unwrap();
    assert!(band < letters, "the band at {band} is drawn before the glyphs at {letters}");
}

#[test]
fn a_bookmark_over_several_paragraphs_bands_each_one_it_touches() {
    crate::test_fonts();
    let mut d = Document::from_text("First of all\nMiddle words here\nLast of all");
    mark(&mut d, 0, 6, 2, 4, "Whole");
    let l = lay(&d);
    let b = bands(&d, &l, true);
    assert_eq!(b.len(), 3, "the middle paragraph holds no mark of its own, and is banded all the same");
    // It opens where its start mark is, and runs to the end of that paragraph.
    let (x, w) = span(&l, 0, 6 + OBJ_W, text_len(&d, 0));
    assert!((b[0].0 - x).abs() < 0.5 && (b[0].1 - w).abs() < 0.5, "to the end of the first: {b:?} over {x}..{}", x + w);
    // The middle one is covered whole, its first letter to its last.
    let (x, w) = span(&l, 1, 0, text_len(&d, 1));
    assert!((b[1].0 - x).abs() < 0.5 && (b[1].1 - w).abs() < 0.5, "the whole middle line: {b:?} over {x}..{}", x + w);
    // The last only as far as the end mark.
    let (x, w) = span(&l, 2, 0, 4);
    assert!((b[2].0 - x).abs() < 0.5 && (b[2].1 - w).abs() < 0.5, "and the last to its end mark: {b:?} over {x}..{}", x + w);
}

/// A bookmark over nothing still gets a mark, so it can be found.
#[test]
fn a_point_bookmark_gets_a_narrow_block() {
    crate::test_fonts();
    let mut d = Document::from_text("Hello world");
    mark(&mut d, 0, 6, 0, 6, "here");
    let l = lay(&d);
    let b = bands(&d, &l, true);
    assert_eq!(b.len(), 1, "the point is marked");
    assert!((b[0].1 - POINT_W).abs() < 0.1, "as a block {POINT_W} wide, not a zero-width band: {b:?}");
    let (x, _) = span(&l, 0, 6 + OBJ_W, 6 + OBJ_W);
    assert!((b[0].0 - x).abs() < 0.5, "at the place the bookmark names: {b:?} over {x}");
}

/// The names a file keeps for its own fields, starting with an underscore, are banded like the
/// ones a person made: they are what a table of contents is built from.
#[test]
fn hidden_bookmarks_are_banded_like_the_rest() {
    crate::test_fonts();
    let mut d = Document::from_text("Chapter one");
    mark(&mut d, 0, 0, 0, 7, "_Toc12345");
    let l = lay(&d);
    let b = bands(&d, &l, true);
    assert_eq!(b.len(), 1, "an underscore name is marked up as any other");
    let (x, w) = span(&l, 0, OBJ_W, 7 + OBJ_W);
    assert!((b[0].0 - x).abs() < 0.5 && (b[0].1 - w).abs() < 0.5, "over `Chapter`: {b:?} over {x}..{}", x + w);
}

/// Two bookmarks over the same text paint one band, not a darker one where they cross.
#[test]
fn overlapping_bookmarks_paint_one_band() {
    crate::test_fonts();
    let mut d = Document::from_text("Hello brave world");
    mark(&mut d, 0, 6, 0, 11, "brave");
    let to = text_len(&d, 0);
    mark(&mut d, 0, 0, 0, to, "all");
    let l = lay(&d);
    let b = bands(&d, &l, true);
    assert_eq!(b.len(), 1, "the joined range, not one band per bookmark");
    let (x, w) = span(&l, 0, OBJ_W, text_len(&d, 0));
    assert!((b[0].0 - x).abs() < 0.5 && (b[0].1 - w).abs() < 0.5, "over the whole line: {b:?} over {x}..{}", x + w);
}
