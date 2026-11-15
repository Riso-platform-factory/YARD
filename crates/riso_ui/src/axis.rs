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
    shares: Rc<RefCell<Vec<AxisSize>>>,
    children: Vec<AnyElement>,
) -> Axis {
    Axis {
        id: id.into(),
        axis,
        shares,
        children,
        on_resized: None,
    }
}

/// A child's flexible share or preferred fixed size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisSize {
    Flex(f32),
    Fixed(Pixels),
}

/// The result of completing a resize gesture.
#[derive(Clone, Debug, PartialEq)]
pub enum Resized {
    Dragged(Vec<AxisSize>),
    Reset,
}

type ResizedHandler = Rc<dyn Fn(Resized, &mut Window, &mut App)>;

pub struct Axis {
    id: ElementId,
    axis: Direction,
    shares: Rc<RefCell<Vec<AxisSize>>>,
    children: Vec<AnyElement>,
    on_resized: Option<ResizedHandler>,
}

impl Axis {
    /// Called once when a drag that changed the shares ends, and once when a
    /// double click resets them, with the resulting sizes or an explicit reset.
    pub fn on_resized(mut self, f: impl Fn(Resized, &mut Window, &mut App) + 'static) -> Self {
        self.on_resized = Some(Rc::new(f));
        self
    }
}

struct DragState {
    shares: Weak<RefCell<Vec<AxisSize>>>,
    handle: Rc<RefCell<Option<usize>>>,
    start: Rc<RefCell<Vec<AxisSize>>>,
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
    drag_start: Rc<RefCell<Vec<AxisSize>>>,
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
            self.shares = Rc::new(RefCell::new(vec![AxisSize::Flex(1.); count]));
        }
        let shares_identity = Rc::downgrade(&self.shares);
        let (dragged_handle, drag_start) =
            window.with_element_state::<DragState, _>(global_id.unwrap(), |state, _| {
                // A new vector identifies a new child set. A positional drag
                // must never carry over to different children, even at the same count.
                let state = state
                    .filter(|state| state.shares.ptr_eq(&shares_identity))
                    .unwrap_or_else(|| DragState {
                        shares: shares_identity,
                        handle: Rc::new(RefCell::new(None)),
                        start: Rc::new(RefCell::new(Vec::new())),
                    });
                ((state.handle.clone(), state.start.clone()), state)
            });
        let shares = self.shares.borrow().clone();
        let mut layout = AxisLayout {
            children: Vec::new(),
            dragged_handle,
            drag_start,
        };
        if count == 0 {
            return layout;
        }
        let minimum = match self.axis {
            Direction::Horizontal => HORIZONTAL_MIN_SIZE,
            Direction::Vertical => VERTICAL_MIN_SIZE,
        };
        let length = f32::from(bounds.size.along(self.axis));
        let (lengths, missing) = layout_sizes(&shares, length, minimum);
        if missing > 0. {
            tracing::trace!(split = %self.id, missing, "fixed children shrunk");
        }
        let all_fixed = shares.iter().all(|size| matches!(size, AxisSize::Fixed(_)));
        let mut origin = bounds.origin;
        for (index, mut child) in std::mem::take(&mut self.children).into_iter().enumerate() {
            let mut child_size = bounds
                .size
                .apply_along(self.axis, |_| px(lengths[index]))
                .map(|dimension| dimension.round());
            if all_fixed && missing > 0. && length >= count as f32 * minimum {
                // Rounding an earlier child up must not consume the room
                // reserved for the remaining children's minimums.
                let remaining =
                    bounds.size.along(self.axis) - (origin - bounds.origin).along(self.axis);
                let available = remaining - px((count - index - 1) as f32 * minimum);
                child_size = child_size.apply_along(self.axis, |size| size.min(available));
            }
            if all_fixed && index + 1 == count {
                // Absorb the earlier children's rounding so fixed-only splits
                // fill their bounds, while retaining minimums when too small.
                let remaining =
                    bounds.size.along(self.axis) - (origin - bounds.origin).along(self.axis);
                child_size = child_size.apply_along(self.axis, |_| {
                    if missing > 0. {
                        remaining.max(px(minimum))
                    } else {
                        remaining.max(px(0.))
                    }
                });
            }
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
                let drag_start = layout.drag_start.clone();
                let shares = self.shares.clone();
                let hitbox = handle.hitbox.clone();
                let id = self.id.to_string();
                let on_resized = self.on_resized.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble() && hitbox.is_hovered(window) {
                        dragged_handle.replace(Some(index));
                        tracing::debug!(id, handle = index, "resize started");
                        if event.click_count >= 2 {
                            for size in shares.borrow_mut().iter_mut() {
                                if let AxisSize::Flex(share) = size {
                                    *share = 1.;
                                }
                            }
                            tracing::debug!(id, "shares reset");
                            if let Some(on_resized) = &on_resized {
                                on_resized(Resized::Reset, window, cx);
                            }
                        }
                        drag_start.replace(shares.borrow().clone());
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
                        if resize_sizes(
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
            let drag_start = layout.drag_start.clone();
            let shares = self.shares.clone();
            let id = self.id.to_string();
            let on_resized = self.on_resized.clone();
            move |_: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() && dragged_handle.replace(None).is_some() {
                    tracing::debug!(id, shares = ?*shares.borrow(), "resize ended");
                    let start = drag_start.take();
                    let shares = shares.borrow().clone();
                    if let Some(on_resized) = &on_resized
                        && shares != start
                    {
                        on_resized(Resized::Dragged(shares), window, cx);
                    }
                    window.refresh();
                }
            }
        });
    }
}

// Preferred fixed sizes remain caller-owned even when layout has to shrink them.
fn layout_sizes(sizes: &[AxisSize], length: f32, minimum: f32) -> (Vec<f32>, f32) {
    if sizes.iter().all(|size| matches!(size, AxisSize::Flex(_))) {
        let sum: f32 = sizes
            .iter()
            .map(|size| match size {
                AxisSize::Flex(share) => *share,
                _ => unreachable!(),
            })
            .sum();
        return (
            sizes
                .iter()
                .map(|size| match size {
                    AxisSize::Flex(share) => length * share / sum,
                    _ => unreachable!(),
                })
                .collect(),
            0.,
        );
    }
    let fixed: f32 = sizes
        .iter()
        .filter_map(|size| match size {
            AxisSize::Fixed(pixels) => Some(f32::from(*pixels)),
            AxisSize::Flex(_) => None,
        })
        .sum();
    let flex: f32 = sizes
        .iter()
        .filter_map(|size| match size {
            AxisSize::Flex(share) => Some(*share),
            AxisSize::Fixed(_) => None,
        })
        .sum();
    let flex_count = sizes
        .iter()
        .filter(|size| matches!(size, AxisSize::Flex(_)))
        .count();
    let missing = (fixed + flex_count as f32 * minimum - length).max(0.);
    let mut lengths: Vec<f32> = sizes
        .iter()
        .map(|size| match size {
            AxisSize::Fixed(pixels) => f32::from(*pixels),
            AxisSize::Flex(share) => {
                if missing > 0. {
                    minimum
                } else {
                    (length - fixed) * share / flex
                }
            }
        })
        .collect();
    if missing > 0. && fixed > 0. {
        // Freeze children that hit their minimum, then redistribute the rest.
        let mut available = length - flex_count as f32 * minimum;
        let mut weight = fixed;
        let mut pending: Vec<usize> = sizes
            .iter()
            .enumerate()
            .filter_map(|(index, size)| matches!(size, AxisSize::Fixed(_)).then_some(index))
            .collect();
        loop {
            let clamped: Vec<usize> = pending
                .iter()
                .copied()
                .filter(|index| lengths[*index] * available / weight < minimum)
                .collect();
            if clamped.is_empty() {
                for index in pending {
                    lengths[index] = (lengths[index] * available / weight).max(minimum);
                }
                break;
            }
            for index in clamped {
                available -= minimum;
                weight -= lengths[index];
                lengths[index] = minimum;
                pending.retain(|pending| *pending != index);
            }
            if pending.is_empty() {
                break;
            }
        }
    } else if flex_count == 0
        && let Some(last) = lengths.last_mut()
    {
        *last += length - fixed;
    }
    (lengths, if fixed > 0. { missing } else { 0. })
}

fn resize_sizes(
    sizes: &mut [AxisSize],
    handle: usize,
    length: f32,
    target: f32,
    minimum: f32,
) -> bool {
    if sizes.iter().all(|size| matches!(size, AxisSize::Flex(_))) {
        let mut shares: Vec<f32> = sizes
            .iter()
            .map(|size| match size {
                AxisSize::Flex(share) => *share,
                AxisSize::Fixed(_) => unreachable!(),
            })
            .collect();
        let changed = compute_resize(&mut shares, handle, length, target, minimum);
        for (size, share) in sizes.iter_mut().zip(shares) {
            *size = AxisSize::Flex(share);
        }
        return changed;
    }
    let (mut lengths, _) = layout_sizes(sizes, length, minimum);
    if lengths.iter().any(|size| *size < minimum - 1.) || length < minimum * sizes.len() as f32 {
        return false;
    }
    let original = lengths.clone();
    let change = target - lengths[handle];
    let forward = change > 0.;
    let receiver = if forward { handle } else { handle + 1 };
    let mut remaining = change.abs();
    let flexible_pair = matches!(sizes[handle], AxisSize::Flex(_))
        && matches!(sizes[handle + 1], AxisSize::Flex(_));
    let count = if !flexible_pair {
        1
    } else if forward {
        sizes.len() - handle - 1
    } else {
        handle + 1
    };
    for offset in 0..count {
        let donor = if forward {
            handle + 1 + offset
        } else {
            handle - offset
        };
        if flexible_pair && matches!(sizes[donor], AxisSize::Fixed(_)) {
            continue;
        }
        let transfer = remaining.min((lengths[donor] - minimum).max(0.));
        lengths[receiver] += transfer;
        lengths[donor] -= transfer;
        remaining -= transfer;
        if remaining <= 0. {
            break;
        }
    }
    if lengths == original {
        return false;
    }
    let share_sum: f32 = sizes
        .iter()
        .filter_map(|size| match size {
            AxisSize::Flex(share) => Some(*share),
            _ => None,
        })
        .sum();
    let flex_length: f32 = sizes
        .iter()
        .zip(&lengths)
        .filter_map(|(size, length)| matches!(size, AxisSize::Flex(_)).then_some(*length))
        .sum();
    for (index, size) in sizes.iter_mut().enumerate() {
        match size {
            AxisSize::Flex(share) => *share = lengths[index] * share_sum / flex_length,
            // Only the adjacent fixed children adopt their dragged shown sizes;
            // other fixed children keep their preferred sizes when cramped.
            AxisSize::Fixed(pixels) if index == handle || index == handle + 1 => {
                *pixels = px(lengths[index]);
            }
            AxisSize::Fixed(_) => {}
        }
    }
    true
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

#[cfg(test)]
mod fixed_tests;
