//! Adapted from Zed's `crates/workspace/src/pane_group.rs` at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.
//!
//! Keeps exact axis sizing, dividers, handle hitboxes, element drag state,
//! resize minimums and mouse events. Drops workspace, serialization, active
//! panes, overlays and settings. Shares are retained by the caller.

use gpui::{
    Along, AnyElement, App, Axis as Direction, Bounds, CursorStyle, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Style, Window, fill, px, relative, size,
};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

const DIVIDER_SIZE: f32 = 1.0;
const HANDLE_HITBOX_SIZE: f32 = 4.0;
const HORIZONTAL_MIN_SIZE: f32 = 80.;
const VERTICAL_MIN_SIZE: f32 = 100.;

/// Draws elements along an axis with caller-owned shares and resize handles.
pub fn axis(
    id: impl Into<ElementId>,
    axis: Direction,
    shares: Rc<RefCell<Vec<f32>>>,
    children: Vec<AnyElement>,
) -> Axis {
    Axis {
        id: id.into(),
        axis,
        shares,
        children,
    }
}

pub struct Axis {
    id: ElementId,
    axis: Direction,
    shares: Rc<RefCell<Vec<f32>>>,
    children: Vec<AnyElement>,
}

struct DragState {
    shares: Weak<RefCell<Vec<f32>>>,
    handle: Rc<RefCell<Option<usize>>>,
}

struct HandleLayout {
    hitbox: Hitbox,
    divider: Bounds<Pixels>,
}

struct ChildLayout {
    bounds: Bounds<Pixels>,
    element: AnyElement,
    handle: Option<HandleLayout>,
}

pub struct AxisLayout {
    children: Vec<ChildLayout>,
    dragged_handle: Rc<RefCell<Option<usize>>>,
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
        Some(self.id.clone())
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
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> AxisLayout {
        let count = self.children.len();
        debug_assert_eq!(self.shares.borrow().len(), count, "one share per child");
        if self.shares.borrow().len() != count {
            // Keep the release fallback local: never reset the caller's shares.
            self.shares = Rc::new(RefCell::new(vec![1.; count]));
        }
        let shares_identity = Rc::downgrade(&self.shares);
        let dragged_handle =
            window.with_element_state::<DragState, _>(global_id.unwrap(), |state, _| {
                // A new vector identifies a new child set. A positional drag
                // must never carry over to different children, even at the same count.
                let state = state
                    .filter(|state| state.shares.ptr_eq(&shares_identity))
                    .unwrap_or_else(|| DragState {
                        shares: shares_identity,
                        handle: Rc::new(RefCell::new(None)),
                    });
                (state.handle.clone(), state)
            });
        let shares = self.shares.borrow().clone();
        let mut layout = AxisLayout {
            children: Vec::new(),
            dragged_handle,
        };
        if count == 0 {
            return layout;
        }
        let space_per_share = bounds.size.along(self.axis) / shares.iter().sum::<f32>();
        let mut origin = bounds.origin;
        for (index, mut child) in std::mem::take(&mut self.children).into_iter().enumerate() {
            let child_size = bounds
                .size
                .apply_along(self.axis, |_| space_per_share * shares[index])
                .map(|dimension| dimension.round());
            tracing::trace!(
                id = %self.id,
                index,
                width = %child_size.width,
                height = %child_size.height,
                "axis child size"
            );
            child.layout_as_root(child_size.into(), window, cx);
            child.prepaint_at(origin, window, cx);
            layout.children.push(ChildLayout {
                bounds: Bounds {
                    origin,
                    size: child_size,
                },
                element: child,
                handle: None,
            });
            origin = origin.apply_along(self.axis, |value| value + child_size.along(self.axis));
        }
        for child in layout.children.iter_mut().take(count - 1) {
            let handle_bounds = Bounds {
                origin: child.bounds.origin.apply_along(self.axis, |origin| {
                    origin + child.bounds.size.along(self.axis) - px(HANDLE_HITBOX_SIZE / 2.)
                }),
                size: child
                    .bounds
                    .size
                    .apply_along(self.axis, |_| px(HANDLE_HITBOX_SIZE)),
            };
            child.handle = Some(HandleLayout {
                hitbox: window.insert_hitbox(handle_bounds, HitboxBehavior::BlockMouse),
                divider: Bounds {
                    origin: child.bounds.origin.apply_along(self.axis, |origin| {
                        origin + child.bounds.size.along(self.axis)
                    }),
                    size: child
                        .bounds
                        .size
                        .apply_along(self.axis, |_| px(DIVIDER_SIZE)),
                },
            });
        }
        layout
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        layout: &mut AxisLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        for child in &mut layout.children {
            child.element.paint(window, cx);
        }
        for (index, child) in layout.children.iter().enumerate() {
            let Some(handle) = &child.handle else {
                continue;
            };
            let cursor = match self.axis {
                Direction::Horizontal => CursorStyle::ResizeColumn,
                Direction::Vertical => CursorStyle::ResizeRow,
            };
            if *layout.dragged_handle.borrow() == Some(index) {
                window.set_window_cursor_style(cursor);
            } else {
                window.set_cursor_style(cursor, &handle.hitbox);
            }
            window.paint_quad(fill(handle.divider, crate::palette(cx).border));
            window.on_mouse_event({
                let dragged_handle = layout.dragged_handle.clone();
                let shares = self.shares.clone();
                let hitbox = handle.hitbox.clone();
                let id = self.id.to_string();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble() && hitbox.is_hovered(window) {
                        dragged_handle.replace(Some(index));
                        tracing::debug!(id, handle = index, "resize started");
                        if event.click_count >= 2 {
                            let mut shares = shares.borrow_mut();
                            shares.fill(1.);
                            tracing::debug!(id, "shares reset");
                        }
                        window.refresh();
                        cx.stop_propagation();
                    }
                }
            });
            window.on_mouse_event({
                let dragged_handle = layout.dragged_handle.clone();
                let shares = self.shares.clone();
                let child_start = child.bounds.origin;
                let axis = self.axis;
                let id = self.id.to_string();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() && *dragged_handle.borrow() == Some(index) {
                        let minimum = match axis {
                            Direction::Horizontal => HORIZONTAL_MIN_SIZE,
                            Direction::Vertical => VERTICAL_MIN_SIZE,
                        };
                        let mut shares = shares.borrow_mut();
                        if compute_resize(
                            &mut shares,
                            index,
                            bounds.size.along(axis).into(),
                            (event.position - child_start).along(axis).into(),
                            minimum,
                        ) {
                            tracing::trace!(id, shares = ?*shares, "shares changed");
                            window.refresh();
                        }
                        cx.stop_propagation();
                    }
                }
            });
        }
        window.on_mouse_event({
            let dragged_handle = layout.dragged_handle.clone();
            let shares = self.shares.clone();
            let id = self.id.to_string();
            move |_: &MouseUpEvent, phase, window, _| {
                if phase.bubble() && dragged_handle.replace(None).is_some() {
                    tracing::debug!(id, shares = ?*shares.borrow(), "resize ended");
                    window.refresh();
                }
            }
        });
    }
}

// Zed's successor traversal, using the sum of the supplied shares.
// Transfer each successor's available space to the grabbed side so that the
// grabbed divider follows the pointer even after its neighbour hits the minimum.
fn compute_resize(
    shares: &mut [f32],
    handle: usize,
    length: f32,
    target: f32,
    minimum: f32,
) -> bool {
    let sum = shares.iter().sum::<f32>();
    let size = |index: usize, shares: &[f32]| length * shares[index] / sum;
    if shares
        .iter()
        .any(|share| length * share / sum < minimum - 1.)
    {
        return false;
    }
    let change = target - size(handle, shares);
    let forward = change > 0.;
    let receiver = if forward { handle } else { handle + 1 };
    let mut remaining = change.abs();
    let mut changed = false;
    let successor_count = if forward {
        shares.len() - handle - 1
    } else {
        handle + 1
    };
    for offset in 0..successor_count {
        let donor = if forward {
            handle + 1 + offset
        } else {
            handle - offset
        };
        if remaining <= 0. {
            break;
        }
        let pixels = remaining.min((size(donor, shares) - minimum).max(0.));
        if pixels > 0. {
            let share_change = pixels * sum / length;
            shares[receiver] += share_change;
            shares[donor] -= share_change;
            remaining -= pixels;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests;
