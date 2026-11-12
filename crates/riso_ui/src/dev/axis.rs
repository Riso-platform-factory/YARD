//! Adapted from Zed's `crates/workspace/src/pane_group.rs` at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.
//!
//! Keeps exact axis sizing, root layout, child prepaint and paint, and dividers.
//! Drops handles, hitboxes, dragging, flexes state, active panes, workspace,
//! overlays, and settings. Every child has a flex of 1.0.

use gpui::{
    Along, AnyElement, App, Axis as Direction, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, fill, px, relative, size,
};

const DIVIDER_SIZE: f32 = 1.0;

/// Draws elements in equal shares along an axis.
pub fn axis(axis: Direction, children: Vec<AnyElement>) -> Axis {
    Axis { axis, children }
}

pub struct Axis {
    axis: Direction,
    children: Vec<AnyElement>,
}

pub struct AxisLayout {
    children: Vec<(Bounds<Pixels>, AnyElement)>,
}

impl IntoElement for Axis {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Axis {
    type RequestLayoutState = ();
    type PrepaintState = AxisLayout;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: relative(0.).into(),
            size: size(relative(1.).into(), relative(1.).into()),
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> AxisLayout {
        let mut layout = AxisLayout {
            children: Vec::new(),
        };
        if self.children.is_empty() {
            return layout;
        }
        let space_per_flex = bounds.size.along(self.axis) / self.children.len() as f32;
        let mut origin = bounds.origin;
        for (index, mut child) in std::mem::take(&mut self.children).into_iter().enumerate() {
            let child_size = bounds
                .size
                .apply_along(self.axis, |_| space_per_flex * 1.0)
                .map(|dimension| dimension.round());
            tracing::trace!(
                index,
                width = %child_size.width,
                height = %child_size.height,
                "axis child size"
            );
            child.layout_as_root(child_size.into(), window, cx);
            child.prepaint_at(origin, window, cx);
            layout.children.push((
                Bounds {
                    origin,
                    size: child_size,
                },
                child,
            ));
            origin = origin.apply_along(self.axis, |value| value + child_size.along(self.axis));
        }
        layout
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        layout: &mut AxisLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (_, child) in &mut layout.children {
            child.paint(window, cx);
        }
        for (bounds, _) in layout
            .children
            .iter()
            .take(layout.children.len().saturating_sub(1))
        {
            let divider = Bounds {
                origin: bounds
                    .origin
                    .apply_along(self.axis, |origin| origin + bounds.size.along(self.axis)),
                size: bounds.size.apply_along(self.axis, |_| px(DIVIDER_SIZE)),
            };
            window.paint_quad(fill(divider, crate::palette(cx).border));
        }
    }
}
