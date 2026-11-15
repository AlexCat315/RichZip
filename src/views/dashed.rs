//! A dashed rounded frame, drawn over a view's own frame.
//!
//! Cove paints solid outlines only — the rounded outline has to be walked as
//! short line runs. The empty state's drop plate needs exactly that: a dashed
//! edge reads as "put something here", where a solid hairline reads as a text
//! field. The rhythm matches Cove's own drop zones (6 on, 4 off) so the plate
//! speaks the same language as the window-level drop target behind it.

use cove_ui::composition::ViewModifier;
use cove_ui::core::scene::{Node, NodeWeak};
use cove_ui::core::{Color, DrawList, Point, Rect, RoundedRect};

pub struct DashedBorder {
    pub color: Color,
    pub width: f32,
    /// Radius of the frame being outlined; the modifier draws on the wrapper
    /// node, which does not carry the corner radius of its content.
    pub radius: f32,
}

impl ViewModifier for DashedBorder {
    fn apply_node(&self, node: &Node) {
        let color = self.color;
        let width = self.width;
        let radius = self.radius;
        let weak: NodeWeak = node.downgrade();
        node.update_overlay_draw((color, width, radius), move |list, origin| {
            let Some(node) = Node::upgrade(&weak) else {
                return;
            };
            let frame = node.frame();
            let rect = Rect::new(origin.x, origin.y, frame.width, frame.height);
            dashed_rounded_rect(list, rect, radius, width, color);
        });
    }
}

const DASH: f32 = 6.0;
const GAP: f32 = 4.0;

fn dashed_rounded_rect(list: &mut DrawList, rect: Rect, radius: f32, width: f32, color: Color) {
    if rect.width <= 0.0
        || rect.height <= 0.0
        || width <= 0.0
        || color.a <= 0.0
        || !rect.width.is_finite()
        || !rect.height.is_finite()
    {
        return;
    }
    // Keep the stroke inside the frame the plate was given.
    let inset = width * 0.5;
    let inner = Rect::new(
        rect.x + inset,
        rect.y + inset,
        (rect.width - width).max(0.0),
        (rect.height - width).max(0.0),
    );
    let radius = (radius.min(inner.width.min(inner.height) * 0.5) - inset).max(0.0);
    let segments = ((radius * 1.5) as usize).clamp(4, 24);
    let mut points = RoundedRect::new(inner, radius).outline(segments);
    if points.len() < 2 {
        return;
    }
    let first = points[0];
    points.push(first);

    let mut remaining = DASH;
    let mut drawing = true;
    for pair in points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt();
        if !length.is_finite() || length <= 0.0001 {
            continue;
        }
        let mut travelled = 0.0;
        while travelled < length {
            let step = remaining.min(length - travelled);
            if drawing && step > 0.0001 {
                let start = travelled / length;
                let end = (travelled + step) / length;
                let _ = list.add_line(
                    Point::new(from.x + (to.x - from.x) * start, from.y + (to.y - from.y) * start),
                    Point::new(from.x + (to.x - from.x) * end, from.y + (to.y - from.y) * end),
                    width,
                    color,
                );
            }
            travelled += step;
            remaining -= step;
            if remaining <= 0.0001 {
                drawing = !drawing;
                remaining = if drawing { DASH } else { GAP };
            }
        }
    }
}
