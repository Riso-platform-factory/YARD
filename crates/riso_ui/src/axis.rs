//! Adapted from Zed's `crates/workspace/src/pane_group.rs` at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.
//!
//! Keeps exact axis sizing, dividers, handle hitboxes, element drag state,
//! resize minimums, given per child by the caller, and mouse events. Drops
//! workspace, serialization, active panes, overlays and settings. Shares are
//! retained by the caller.

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

/// Draws elements along an axis with caller-owned shares and resize handles.
pub fn axis(
    id: impl Into<ElementId>,
    axis: Direction,
    shares: Rc<RefCell<Vec<AxisChild>>>,
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

/// A child's size and the smallest length the axis gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisChild {
    pub size: AxisSize,
    pub min: Pixels,
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
    shares: Rc<RefCell<Vec<AxisChild>>>,
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
    shares: Weak<RefCell<Vec<AxisChild>>>,
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
            self.shares = Rc::new(RefCell::new(vec![
                AxisChild {
                    size: AxisSize::Flex(1.),
                    min: px(0.),
                };
                count
            ]));
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
        let (shares, mins) = split_children(&self.shares.borrow());
        let mut layout = AxisLayout {
            children: Vec::new(),
            dragged_handle,
            drag_start,
        };
        if count == 0 {
            return layout;
        }
        tracing::trace!(split = %self.id, mins = ?mins, "child minimums");
        let length = f32::from(bounds.size.along(self.axis));
        let (lengths, missing) = layout_sizes(&shares, length, &mins);
        let total_minimum: f32 = mins.iter().sum();
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
            if all_fixed && missing > 0. && length >= total_minimum {
                // Rounding an earlier child up must not consume the room
                // reserved for the remaining children's minimums.
                let remaining =
                    bounds.size.along(self.axis) - (origin - bounds.origin).along(self.axis);
                let available = remaining - px(mins[index + 1..].iter().sum::<f32>());
                child_size = child_size.apply_along(self.axis, |size| size.min(available));
            }
            if all_fixed && index + 1 == count {
                // Absorb the earlier children's rounding so fixed-only splits
                // fill their bounds, while retaining minimums when too small.
                let remaining =
                    bounds.size.along(self.axis) - (origin - bounds.origin).along(self.axis);
                child_size = child_size.apply_along(self.axis, |_| {
                    if missing > 0. {
                        remaining.max(px(mins[index]))
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
                            for child in shares.borrow_mut().iter_mut() {
                                if let AxisSize::Flex(share) = &mut child.size {
                                    *share = 1.;
                                }
                            }
                            tracing::debug!(id, "shares reset");
                            if let Some(on_resized) = &on_resized {
                                on_resized(Resized::Reset, window, cx);
                            }
                        }
                        drag_start.replace(split_children(&shares.borrow()).0);
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
                        let mut children = shares.borrow_mut();
                        let (mut sizes, mins) = split_children(&children);
                        if resize_sizes(
                            &mut sizes,
                            index,
                            bounds.size.along(axis).into(),
                            (event.position - child_start).along(axis).into(),
                            &mins,
                        ) {
                            for (child, size) in children.iter_mut().zip(&sizes) {
                                child.size = *size;
                            }
                            tracing::trace!(id, shares = ?sizes, "shares changed");
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
                    let shares = split_children(&shares.borrow()).0;
                    tracing::debug!(id, shares = ?shares, "resize ended");
                    let start = drag_start.take();
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

fn split_children(children: &[AxisChild]) -> (Vec<AxisSize>, Vec<f32>) {
    children
        .iter()
        .map(|child| (child.size, f32::from(child.min)))
        .unzip()
}

// Shares `available` among `indices` by their weights, freezing each child
// that would fall below its own minimum at that minimum.
fn share_out(
    lengths: &mut [f32],
    indices: &[usize],
    weights: &[f32],
    mins: &[f32],
    available: f32,
) {
    let mut available = available;
    let mut pending: Vec<usize> = indices.to_vec();
    loop {
        let weight: f32 = pending.iter().map(|index| weights[*index]).sum();
        let clamped: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|index| available * weights[*index] / weight < mins[*index])
            .collect();
        if clamped.is_empty() {
            for index in pending {
                lengths[index] = available * weights[index] / weight;
            }
            return;
        }
        for index in clamped {
            available -= mins[index];
            lengths[index] = mins[index];
            pending.retain(|pending| *pending != index);
        }
        if pending.is_empty() {
            return;
        }
    }
}

// Preferred fixed sizes remain caller-owned even when layout has to shrink them.
// A fixed size below its child's minimum is laid out at the minimum.
fn layout_sizes(sizes: &[AxisSize], length: f32, mins: &[f32]) -> (Vec<f32>, f32) {
    let sizes: Vec<AxisSize> = sizes
        .iter()
        .zip(mins)
        .map(|(size, minimum)| match size {
            AxisSize::Fixed(pixels) => AxisSize::Fixed((*pixels).max(px(*minimum))),
            AxisSize::Flex(share) => AxisSize::Flex(*share),
        })
        .collect();
    let sizes = sizes.as_slice();
    let shares: Vec<f32> = sizes
        .iter()
        .map(|size| match size {
            AxisSize::Flex(share) => *share,
            AxisSize::Fixed(_) => 0.,
        })
        .collect();
    let flexible: Vec<usize> = sizes
        .iter()
        .enumerate()
        .filter_map(|(index, size)| matches!(size, AxisSize::Flex(_)).then_some(index))
        .collect();
    if flexible.len() == sizes.len() {
        let mut lengths = vec![0.; sizes.len()];
        if length >= mins.iter().sum::<f32>() {
            share_out(&mut lengths, &flexible, &shares, mins, length);
        } else {
            // Nothing fits every minimum: keep the shares' proportions.
            let sum: f32 = shares.iter().sum();
            for (length_of, share) in lengths.iter_mut().zip(&shares) {
                *length_of = length * share / sum;
            }
        }
        return (lengths, 0.);
    }
    let fixed: f32 = sizes
        .iter()
        .filter_map(|size| match size {
            AxisSize::Fixed(pixels) => Some(f32::from(*pixels)),
            AxisSize::Flex(_) => None,
        })
        .sum();
    let flex_minimum: f32 = flexible.iter().map(|index| mins[*index]).sum();
    let missing = (fixed + flex_minimum - length).max(0.);
    let mut lengths: Vec<f32> = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| match size {
            AxisSize::Fixed(pixels) => f32::from(*pixels),
            AxisSize::Flex(_) => mins[index],
        })
        .collect();
    if missing == 0. {
        share_out(&mut lengths, &flexible, &shares, mins, length - fixed);
    }
    if missing > 0. && fixed > 0. {
        // Freeze children that hit their minimum, then redistribute the rest.
        let mut available = length - flex_minimum;
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
                .filter(|index| lengths[*index] * available / weight < mins[*index])
                .collect();
            if clamped.is_empty() {
                for index in pending {
                    lengths[index] = (lengths[index] * available / weight).max(mins[index]);
                }
                break;
            }
            for index in clamped {
                available -= mins[index];
                weight -= lengths[index];
                lengths[index] = mins[index];
                pending.retain(|pending| *pending != index);
            }
            if pending.is_empty() {
                break;
            }
        }
    } else if flexible.is_empty()
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
    mins: &[f32],
) -> bool {
    if sizes.iter().all(|size| matches!(size, AxisSize::Flex(_))) {
        let mut shares: Vec<f32> = sizes
            .iter()
            .map(|size| match size {
                AxisSize::Flex(share) => *share,
                AxisSize::Fixed(_) => unreachable!(),
            })
            .collect();
        let sum: f32 = shares.iter().sum();
        let (lengths, _) = layout_sizes(sizes, length, mins);
        if length > 0.
            && lengths
                .iter()
                .zip(&shares)
                .any(|(shown, share)| *shown != length * share / sum)
        {
            // A child held at its minimum is shown larger than its share:
            // resize from what is shown.
            for (share, shown) in shares.iter_mut().zip(&lengths) {
                *share = shown * sum / length;
            }
        }
        let changed = compute_resize(&mut shares, handle, length, target, mins);
        for (size, share) in sizes.iter_mut().zip(shares) {
            *size = AxisSize::Flex(share);
        }
        return changed;
    }
    let (mut lengths, _) = layout_sizes(sizes, length, mins);
    if lengths
        .iter()
        .zip(mins)
        .any(|(size, minimum)| *size < minimum - 1.)
        || length < mins.iter().sum::<f32>()
    {
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
        let transfer = remaining.min((lengths[donor] - mins[donor]).max(0.));
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
            // Flexible children with no length left keep their shares.
            AxisSize::Flex(_) if flex_length <= 0. => {}
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
// grabbed divider follows the pointer even after its neighbour hits its minimum.
fn compute_resize(
    shares: &mut [f32],
    handle: usize,
    length: f32,
    target: f32,
    mins: &[f32],
) -> bool {
    let sum = shares.iter().sum::<f32>();
    let size = |index: usize, shares: &[f32]| length * shares[index] / sum;
    if shares
        .iter()
        .zip(mins)
        .any(|(share, minimum)| length * share / sum < minimum - 1.)
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
        let pixels = remaining.min((size(donor, shares) - mins[donor]).max(0.));
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
