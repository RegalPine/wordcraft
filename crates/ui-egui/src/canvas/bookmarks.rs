//! View › Show › Bookmarks: the brackets Word puts around every bookmark a person named — `[` at
//! its start, `]` at its end, an I-beam where it covers no text (screen only — the layout, the
//! print markup and every export ignore them). Names a program made for itself (`_Toc…`, `_Ref…`,
//! `_GoBack`) are hidden, as in Word's own list. Resting the pointer on a mark names the bookmark,
//! the way Word's own marks do.

use egui::{Align2, Color32, Pos2, Rect, Stroke, pos2, vec2};
use wordcraft_doc::BookmarkRange;

use super::{DocLayout, WordApp};
use crate::theme::{Tokens, regular};

/// Most marks drawn on one walk (a template-heavy or hostile file).
const MAX_MARKS: usize = 400;

/// One mark's screen rect: the page it is on mapped onto the canvas.
fn spot(app: &WordApp, layout: &DocLayout, rects: &[Rect], scale: f32, r: &BookmarkRange, at_end: bool) -> Option<Rect> {
    let at = if at_end { &r.end } else { &r.start };
    let c = layout.caret_on(at, app.session.page_hint)?;
    let pr = rects.get(c.page)?;
    Some(Rect::from_min_size(pos2(pr.min.x + c.x * scale, pr.min.y + c.top * scale), vec2(0.0, c.height.max(1.0) * scale)))
}

/// The caret sits inside this bookmark's range. A walk never closes a bookmark before it opens
/// one, so `start` is always at or before `end`.
fn holds(r: &BookmarkRange, caret: &wordcraft_doc::Pos) -> bool {
    r.start <= *caret && *caret <= r.end
}

/// The strip a mark's name is read from: a mark is a glyph's width at most, so the pointer gets a
/// little overhang around it to find it.
fn zone(m: &Rect) -> Rect {
    m.expand2(vec2(3.0, 1.0))
}

/// The line's middle, where a mark is anchored.
fn mid(s: &Rect) -> Pos2 {
    pos2(s.min.x, (s.min.y + s.max.y) / 2.0)
}

/// The bracket Word shows at one end of a bookmarked range: `[` before the text it opens, `]`
/// after the text it closes. It is painted over the page, so it takes no room in the layout.
fn bracket(painter: &egui::Painter, s: &Rect, at_end: bool, ink: Color32) -> Rect {
    let size = (s.height() * 0.8).max(5.0);
    let (anchor, ch) = if at_end { (Align2::LEFT_CENTER, "]") } else { (Align2::RIGHT_CENTER, "[") };
    painter.text(mid(s), anchor, ch, regular(size), ink)
}

/// Word's mark for a bookmark that covers no text: an I-beam, a vertical rule with a short serif
/// at each end.
fn i_beam(painter: &egui::Painter, s: &Rect, ink: Color32) -> Rect {
    let stroke = Stroke::new(1.2, ink);
    let cap = 3.5_f32.min(s.height() / 3.0);
    painter.line_segment([pos2(s.min.x, s.min.y), pos2(s.min.x, s.max.y)], stroke);
    for y in [s.min.y, s.max.y] {
        painter.line_segment([pos2(s.min.x - cap, y), pos2(s.min.x + cap, y)], stroke);
    }
    *s
}

/// Name the bookmark the pointer rests on. The area is deliberately not interactive: an
/// `ui.interact` hover would take the pointer off the canvas's own response, and the canvas needs
/// it to choose the caret cursor and to draw the marks (#1746).
fn tip(ui: &egui::Ui, at: Pos2, name: &str) {
    egui::Area::new(egui::Id::unique("bookmark_mark_tip"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(at + vec2(6.0, -2.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                // Unwrapped: the area keeps last frame's width, too narrow for a longer name.
                ui.add(egui::Label::new(name).extend());
            });
        });
}

/// Draw the marks, inside the canvas viewport after the selection and the content controls.
/// `hover` is the pointer, when the canvas owns it. The page's geometry (`layout`, `rects`,
/// `scale`) travels down from the canvas rather than being worked out again, which is why this
/// takes eight arguments.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    app: &mut WordApp,
    ui: &egui::Ui,
    painter: &egui::Painter,
    t: &Tokens,
    layout: &DocLayout,
    rects: &[Rect],
    scale: f32,
    hover: Option<Pos2>,
) {
    if !app.session.view.show_bookmarks || app.session.view.read_mode {
        return;
    }
    let caret = app.session.sel.focus.clone();
    let mut named = false;
    for r in app.session.doc.bookmark_ranges().into_iter().filter(|r| r.visible()).take(MAX_MARKS) {
        // The one the caret is in stands out; the rest stay quiet so a form of 60 fields is
        // still readable.
        let ink = if holds(&r, &caret) { t.accent } else { t.accent.gamma_multiply(0.55) };
        // A bookmark over nothing is a single I-beam; over text it is bracketed at both ends.
        let point = r.start == r.end;
        for at_end in [false, true] {
            if point && at_end {
                continue;
            }
            let Some(s) = spot(app, layout, rects, scale, &r, at_end) else { continue };
            let m = if point { i_beam(painter, &s, ink) } else { bracket(painter, &s, at_end, ink) };
            // One name a frame, the one nearest the start when two marks share a spot.
            if !named && hover.is_some_and(|p| zone(&m).contains(p)) {
                named = true;
                tip(ui, pos2(m.min.x, m.min.y), &r.name);
            }
        }
    }
}
