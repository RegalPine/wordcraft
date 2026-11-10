//! Where the text a bookmark covers sits in each paragraph, for the band drawn over it.

use std::cell::OnceCell;

use wordcraft_doc::para::OBJ;

use super::{Document, HashMap, InlineObject, Path, Rgb, StoryRef};

/// The band drawn over bookmarked text. Our own colour, chosen to sit clear of the colours a
/// document can already carry: the field shading's yellow, the comment markup's lavender, and
/// Word's highlight palette.
pub(crate) const BOOKMARK_COLOR: Rgb = Rgb(0xD2, 0xEC, 0xF2);

/// How wide the mark for a bookmark that covers no text is: a point has nothing to stretch over,
/// so it gets a narrow block at its place instead.
pub(crate) const POINT_W: f32 = 3.0;

/// Most bookmark ranges one drawing keeps, and most bookmarks it pairs at once. Both come from
/// the file, so a document with more is marked up to the cap and left at that.
const MAX: usize = 4096;

/// Each paragraph's bookmark ranges, as byte ranges of its text, for the story it is drawn from.
///
/// A bookmark that runs over a paragraph leaves no mark inside it, so its band can only be found
/// by walking a story's paragraphs in order. That walk is done once for a page's drawing, when
/// its first paragraph asks for it, and not once for every paragraph the page draws.
pub(super) struct Marks<'a> {
    doc: &'a Document,
    by_story: OnceCell<HashMap<StoryRef, HashMap<Path, Vec<(usize, usize)>>>>,
}

impl<'a> Marks<'a> {
    pub(super) fn new(doc: &'a Document) -> Self {
        Marks { doc, by_story: OnceCell::new() }
    }

    /// The ranges paragraph `path` of `story` shows bookmarked: nothing when it holds no mark and
    /// no bookmark runs over it.
    pub(super) fn para(&self, story: StoryRef, path: &Path) -> &[(usize, usize)] {
        self.by_story
            .get_or_init(|| {
                // The parts are keyed in order, so the cap falls in the same place twice.
                let parts = self.doc.parts.keys().copied().map(StoryRef::Part);
                let mut left = MAX;
                let mut out: HashMap<StoryRef, HashMap<Path, Vec<(usize, usize)>>> = HashMap::new();
                for s in std::iter::once(StoryRef::Body).chain(parts) {
                    out.insert(s, ranges(self.doc, s, &mut left));
                    if left == 0 {
                        break;
                    }
                }
                out
            })
            .get(&story)
            .and_then(|m| m.get(path))
            .map_or(&[], Vec::as_slice)
    }
}

/// The bookmark ranges of every paragraph of `story`, in document order. `left` is the room the
/// caller has for ranges; it spends as they are kept, and a story walked when it is empty keeps
/// nothing (so a file with thousands of bookmarks stops the walk instead of filling the page).
fn ranges(doc: &Document, story: StoryRef, left: &mut usize) -> HashMap<Path, Vec<(usize, usize)>> {
    let mut out: HashMap<Path, Vec<(usize, usize)>> = HashMap::new();
    // The bookmarks open in the paragraph being walked, each with where its text begins here: at
    // the mark that opened it in this paragraph, at nothing for one that began further back.
    let mut open: Vec<(String, usize)> = Vec::new();
    for path in doc.para_paths(story) {
        crate::work::steps(1);
        if *left == 0 {
            return out;
        }
        let Some(p) = doc.para(story, &path) else { continue };
        let mut starts: Vec<usize> = vec![0; open.len()];
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for (off, o) in p.objects_with_offsets() {
            if *left == 0 {
                break;
            }
            match o {
                InlineObject::BookmarkStart { name } => {
                    if open.len() < MAX {
                        // The bookmark's text begins after the mark that opens it.
                        let from = off.saturating_add(OBJ.len_utf8());
                        open.push((name.clone(), from));
                        starts.push(from);
                    }
                }
                InlineObject::BookmarkEnd { name } => {
                    // An end closes the earliest start of its name still open, as Word pairs them.
                    // One with no start in this story (a file holding only the end, or a start
                    // past the cap) marks from the paragraph's start, as a comment's does.
                    let from = match open.iter().position(|(n, _)| n == name) {
                        Some(i) => {
                            open.remove(i);
                            starts.remove(i)
                        }
                        None => 0,
                    };
                    spans.push((from, off));
                    *left = left.saturating_sub(1);
                }
                _ => {}
            }
        }
        // What is still open runs to the end of the paragraph, and begins the next one at 0.
        for s in starts.iter() {
            if *left == 0 {
                break;
            }
            spans.push((*s, p.len()));
            *left = left.saturating_sub(1);
        }
        if spans.is_empty() {
            continue;
        }
        merge(&mut spans);
        out.insert(path, spans);
    }
    out
}

/// Sort and join the ranges of one paragraph, so overlapping bookmarks paint one band instead of
/// a darker band where they cross. A range with nothing in it (a point bookmark) joins a range
/// that ends where it sits, and keeps its own place otherwise.
fn merge(spans: &mut Vec<(usize, usize)>) {
    spans.sort_unstable();
    let mut kept: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (a, b) in spans.iter().copied() {
        match kept.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => kept.push((a, b)),
        }
    }
    *spans = kept;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_doc::{Paragraph, para_block};

    /// A body of `n` paragraphs, each holding `text`.
    fn body(n: usize, text: &str) -> Document {
        let mut d = Document::new();
        d.body = (0..n).map(|_| para_block(Paragraph::with_text(text, Default::default()))).collect();
        d
    }

    /// The marks at `at` in paragraph `para`, highest offset first so the ones already placed
    /// keep their place. Offsets count the text before any mark is added.
    fn mark(d: &mut Document, para: usize, at: &[(usize, InlineObject)]) {
        let p = d.para_mut(StoryRef::Body, &Path::top(para)).unwrap();
        for (off, o) in at {
            p.insert_object(*off, o.clone(), &Default::default()).unwrap();
        }
    }

    fn start(name: &str) -> InlineObject {
        InlineObject::BookmarkStart { name: name.into() }
    }

    fn end(name: &str) -> InlineObject {
        InlineObject::BookmarkEnd { name: name.into() }
    }

    /// The ranges of paragraph `para`, as the drawing sees them.
    fn ranges_of(d: &Document, para: usize) -> Vec<(usize, usize)> {
        Marks::new(d).para(StoryRef::Body, &Path::top(para)).to_vec()
    }

    #[test]
    fn a_bookmark_bands_the_text_between_its_marks() {
        let mut d = body(1, "abcd");
        mark(&mut d, 0, &[(2, end("in")), (1, start("in"))]);
        // The text is `a`, a mark, `b`, a mark, `cd`: the mark itself is three bytes.
        assert_eq!(ranges_of(&d, 0), vec![(4, 5)], "just the `b`, from after the start to the end");
    }

    #[test]
    fn nested_bookmarks_join_into_one_band() {
        let mut d = body(1, "abcd");
        mark(&mut d, 0, &[(3, end("out")), (2, end("in")), (1, start("in")), (0, start("out"))]);
        assert_eq!(ranges_of(&d, 0), vec![(3, 12)], "one band over the wider bookmark, not a darker one");
    }

    #[test]
    fn an_unclosed_bookmark_runs_to_the_end_of_each_paragraph_it_reaches() {
        let mut d = body(2, "abcd");
        mark(&mut d, 0, &[(1, start("on"))]);
        assert_eq!(ranges_of(&d, 0), vec![(4, 7)], "to the end of the paragraph it opens in");
        assert_eq!(ranges_of(&d, 1), vec![(0, 4)], "and the whole of the next one, markless though it is");
    }

    #[test]
    fn an_end_with_no_start_marks_from_the_paragraph_start() {
        let mut d = body(1, "abcd");
        mark(&mut d, 0, &[(2, end("gone"))]);
        assert_eq!(ranges_of(&d, 0), vec![(0, 2)], "as a comment without its start does");
    }

    /// A file carrying more bookmarks than the drawing keeps is marked up to the cap and left
    /// there, rather than the walk growing with the file.
    #[test]
    fn a_file_with_endless_bookmarks_stops_at_the_cap() {
        let mut d = body(MAX * 2, "ab");
        for i in 0..MAX * 2 {
            mark(&mut d, i, &[(1, end(&format!("b{i}"))), (0, start(&format!("b{i}")))]);
        }
        let m = Marks::new(&d);
        let paths = d.para_paths(StoryRef::Body);
        let kept: usize = paths.iter().map(|p| m.para(StoryRef::Body, p).len()).sum();
        assert_eq!(kept, MAX, "one range in each of the first {MAX} paragraphs, then the walk stops");
        assert!(m.para(StoryRef::Body, paths.last().unwrap()).is_empty(), "and nothing at the end of the file");
    }
}
