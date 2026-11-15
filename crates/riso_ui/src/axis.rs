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
        can_collapse: None,
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
    pub collapsed: bool,
}

/// The result of completing a resize gesture.
#[derive(Clone, Debug, PartialEq)]
pub enum Resized {
    Dragged {
        sizes: Vec<AxisSize>,
        snapped_shut: Vec<usize>,
        snapped_open: Vec<usize>,
    },
    Reset,
}

type CollapseGuard = Rc<dyn Fn(usize, &App) -> bool>;

type ResizedHandler = Rc<dyn Fn(Resized, &mut Window, &mut App)>;

pub struct Axis {
    id: ElementId,
    axis: Direction,
    shares: Rc<RefCell<Vec<AxisChild>>>,
    children: Vec<AnyElement>,
    on_resized: Option<ResizedHandler>,
    can_collapse: Option<CollapseGuard>,
}

impl Axis {
    /// Chooses which children may snap shut; without a guard none may collapse.
    pub fn can_collapse(mut self, f: impl Fn(usize, &App) -> bool + 'static) -> Self {
        self.can_collapse = Some(Rc::new(f));
        self
    }

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
    start: Rc<RefCell<Vec<AxisChild>>>,
    open: Rc<RefCell<Option<(usize, bool)>>>,
    refused: Rc<RefCell<Vec<usize>>>,
}

struct HandleLayout {
    hitbox: Hitbox,
    divider: Bounds<Pixels>,
    open: Option<(usize, bool)>,
}

struct ChildLayout {
    bounds: Bounds<Pixels>,
    element: AnyElement,
    handle: Option<HandleLayout>,
}

pub struct AxisLayout {
    children: Vec<ChildLayout>,
    dragged_handle: Rc<RefCell<Option<usize>>>,
    drag_start: Rc<RefCell<Vec<AxisChild>>>,
    drag_open: Rc<RefCell<Option<(usize, bool)>>>,
    refused: Rc<RefCell<Vec<usize>>>,
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
                    collapsed: false,
                };
                count
            ]));
        }
        let shares_identity = Rc::downgrade(&self.shares);
        let (dragged_handle, drag_start, drag_open, refused) = window
            .with_element_state::<DragState, _>(global_id.unwrap(), |state, _| {
                // A new vector identifies a new child set. A positional drag
                // must never carry over to different children, even at the same count.
                let state = state
                    .filter(|state| state.shares.ptr_eq(&shares_identity))
                    .unwrap_or_else(|| DragState {
                        shares: shares_identity,
                        handle: Rc::new(RefCell::new(None)),
                        start: Rc::new(RefCell::new(Vec::new())),
                        open: Rc::new(RefCell::new(None)),
                        refused: Rc::new(RefCell::new(Vec::new())),
                    });
                (
                    (
                        state.handle.clone(),
                        state.start.clone(),
                        state.open.clone(),
                        state.refused.clone(),
                    ),
                    state,
                )
            });
        let (_, mins) = split_children(&self.shares.borrow());
        let mut layout = AxisLayout {
            children: Vec::new(),
            dragged_handle,
            drag_start,
            drag_open,
            refused,
        };
        if count == 0 {
            return layout;
        }
        tracing::trace!(split = %self.id, mins = ?mins, "child minimums");
        let length = f32::from(bounds.size.along(self.axis));
        let (lengths, missing) = child_lengths(&self.shares.borrow(), length);
        let collapsed: Vec<bool> = self
            .shares
            .borrow()
            .iter()
            .map(|child| child.collapsed)
            .collect();
        let last_shown = collapsed.iter().rposition(|collapsed| !collapsed);
        let minimum_at = |index: usize| {
            if collapsed[index] { 0. } else { mins[index] }
        };
        let total_minimum: f32 = (0..count).map(minimum_at).sum();
        if missing > 0. {
            tracing::trace!(split = %self.id, missing, "fixed children shrunk");
        }
        let all_fixed = self
            .shares
            .borrow()
            .iter()
            .all(|child| child.collapsed || matches!(child.size, AxisSize::Fixed(_)));
        let mut origin = bounds.origin;
        for (index, mut child) in std::mem::take(&mut self.children).into_iter().enumerate() {
            let mut child_size = bounds
                .size
                .apply_along(self.axis, |_| px(lengths[index]))
                .map(|dimension| dimension.round());
            if all_fixed && !collapsed[index] && missing > 0. && length >= total_minimum {
                // Rounding an earlier child up must not consume the room
                // reserved for the remaining children's minimums.
                let remaining =
                    bounds.size.along(self.axis) - (origin - bounds.origin).along(self.axis);
                let available = remaining - px((index + 1..count).map(minimum_at).sum::<f32>());
                child_size = child_size.apply_along(self.axis, |size| size.min(available));
            }
            if all_fixed && Some(index) == last_shown {
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
            if !self.shares.borrow()[index].collapsed {
                child.layout_as_root(child_size.into(), window, cx);
                child.prepaint_at(origin, window, cx);
            }
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
        for (index, child) in layout.children.iter_mut().enumerate() {
            // Only boundaries between shown neighbours have ordinary handles.
            if !collapsed[index] && !collapsed[index + 1..].iter().any(|hidden| !hidden) {
                continue;
            }
            let forward = collapsed[index + 1..].iter().any(|collapsed| !collapsed)
                || collapsed[..index].iter().all(|collapsed| *collapsed);
            let open = collapsed[index].then_some((index, forward));
            let edge = if open.is_some() {
                child.bounds.origin.along(self.axis)
                    + if forward {
                        child.bounds.size.along(self.axis)
                    } else {
                        px(0.)
                    }
            } else {
                child.bounds.origin.along(self.axis) + child.bounds.size.along(self.axis)
            };
            // Minimum-constrained children may extend beyond the split. An
            // outer line and its hitbox still belong to the visible split edge.
            let outer =
                open.is_some() && (!forward || collapsed[..index].iter().all(|hidden| *hidden));
            let edge = if open.is_some() && !forward {
                bounds.origin.along(self.axis) + bounds.size.along(self.axis)
            } else if open.is_some() && collapsed[..index].iter().all(|hidden| *hidden) {
                bounds.origin.along(self.axis)
            } else {
                edge
            };
            let following_hidden = collapsed[index + 1..]
                .iter()
                .take_while(|hidden| **hidden)
                .count();
            let hit_size = if outer {
                2. * HANDLE_HITBOX_SIZE
            } else {
                HANDLE_HITBOX_SIZE
            };
            let hit_start = if open.is_some() {
                // Keep the last hidden child nearest the boundary. Each earlier
                // line gets its own adjacent strip on the side it opens into.
                let offset = following_hidden as f32 * hit_size;
                if forward {
                    edge + px(offset)
                } else {
                    edge - px(offset + hit_size)
                }
            } else if following_hidden > 0 {
                edge - px(HANDLE_HITBOX_SIZE)
            } else {
                edge - px(HANDLE_HITBOX_SIZE / 2.)
            };
            let (hit_start, hit_size) = if outer {
                let start = bounds.origin.along(self.axis);
                let end = start + bounds.size.along(self.axis);
                let hit_end = (hit_start + px(hit_size)).clamp(start, end);
                let hit_start = hit_start.clamp(start, end);
                (hit_start, hit_end - hit_start)
            } else {
                (hit_start, px(hit_size))
            };
            let handle_bounds = Bounds {
                origin: child.bounds.origin.apply_along(self.axis, |_| hit_start),
                size: child.bounds.size.apply_along(self.axis, |_| hit_size),
            };
            let divider_edge = if outer {
                let start = bounds.origin.along(self.axis);
                let end = start + bounds.size.along(self.axis);
                if forward {
                    (start + px(1.)).min((end - px(DIVIDER_SIZE)).max(start))
                } else {
                    (end - px(1. + DIVIDER_SIZE)).max(start)
                }
            } else if open.is_some() {
                edge.min(
                    bounds.origin.along(self.axis) + bounds.size.along(self.axis)
                        - px(DIVIDER_SIZE),
                )
                .max(bounds.origin.along(self.axis))
            } else {
                edge
            };
            child.handle = Some(HandleLayout {
                hitbox: window.insert_hitbox(handle_bounds, HitboxBehavior::BlockMouse),
                open,
                divider: Bounds {
                    origin: child.bounds.origin.apply_along(self.axis, |_| divider_edge),
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
        for (index, child) in layout.children.iter_mut().enumerate() {
            if !self.shares.borrow()[index].collapsed {
                child.element.paint(window, cx);
            }
        }
        if layout.dragged_handle.borrow().is_some() {
            window.set_window_cursor_style(match self.axis {
                Direction::Horizontal => CursorStyle::ResizeColumn,
                Direction::Vertical => CursorStyle::ResizeRow,
            });
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
                let drag_open = layout.drag_open.clone();
                let refused = layout.refused.clone();
                let open = handle.open;
                let shares = self.shares.clone();
                let hitbox = handle.hitbox.clone();
                let id = self.id.to_string();
                let on_resized = self.on_resized.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble() && hitbox.is_hovered(window) {
                        dragged_handle.replace(Some(index));
                        drag_open.replace(open);
                        refused.borrow_mut().clear();
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
                        drag_start.replace(shares.borrow().clone());
                        window.refresh();
                        cx.stop_propagation();
                    }
                }
            });
        }
        window.on_mouse_event({
                let dragged_handle = layout.dragged_handle.clone();
                let shares = self.shares.clone();
                let drag_start = layout.drag_start.clone();
                let drag_open = layout.drag_open.clone();
                let can_collapse = self.can_collapse.clone();
                let refused = layout.refused.clone();
                let axis = self.axis;
                let id = self.id.to_string();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() && let Some(index) = *dragged_handle.borrow() {
                        let start = drag_start.borrow();
                        let mut children = shares.borrow_mut();
                        // A refusal belongs to this threshold crossing, even
                        // across redraws; crossing back clears it on this move.
                        let mut next_refused = Vec::new();
                        let next = preview_resize(
                            &start, index, *drag_open.borrow(),
                            bounds.size.along(axis).into(),
                            (event.position - bounds.origin).along(axis).into(),
                            |child| {
                                let allowed = !refused.borrow().contains(&child)
                                    && can_collapse.as_ref().is_some_and(|guard| guard(child, cx));
                                if !allowed {
                                    next_refused.push(child);
                                }
                                allowed
                            },
                        );
                        refused.replace(next_refused);
                        for (child, (before, after)) in children.iter().zip(&next).enumerate() {
                            if before.collapsed != after.collapsed {
                                tracing::debug!(split = id, child, shut = after.collapsed, "snap preview");
                            }
                        }
                        if *children != next {
                            *children = next;
                            tracing::trace!(id, shares = ?split_children(&children).0, "shares changed");
                            window.refresh();
                        }
                        cx.stop_propagation();
                    }
                }
            });
        window.on_mouse_event({
            let dragged_handle = layout.dragged_handle.clone();
            let drag_start = layout.drag_start.clone();
            let shares = self.shares.clone();
            let id = self.id.to_string();
            let on_resized = self.on_resized.clone();
            move |_: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() && dragged_handle.replace(None).is_some() {
                    let children = shares.borrow().clone();
                    let sizes = split_children(&children).0;
                    tracing::debug!(id, shares = ?sizes, "resize ended");
                    let start = drag_start.take();
                    if let Some(on_resized) = &on_resized
                        && children != start
                    {
                        on_resized(
                            Resized::Dragged {
                                sizes,
                                snapped_shut: children
                                    .iter()
                                    .zip(&start)
                                    .enumerate()
                                    .filter_map(|(i, (now, was))| {
                                        (now.collapsed && !was.collapsed).then_some(i)
                                    })
                                    .collect(),
                                snapped_open: children
                                    .iter()
                                    .zip(&start)
                                    .enumerate()
                                    .filter_map(|(i, (now, was))| {
                                        (!now.collapsed && was.collapsed).then_some(i)
                                    })
                                    .collect(),
                            },
                            window,
                            cx,
                        );
                    }
                    window.refresh();
                }
            }
        });
    }
}

// Collapsed children take no layout space; their lines overlay the boundary.
fn child_lengths(children: &[AxisChild], length: f32) -> (Vec<f32>, f32) {
    let shown: Vec<_> = children
        .iter()
        .filter(|child| !child.collapsed)
        .copied()
        .collect();
    let (sizes, mins) = split_children(&shown);
    let (lengths, missing) = layout_sizes(&sizes, length, &mins);
    let mut shown = lengths.into_iter();
    (
        children
            .iter()
            .map(|child| {
                if child.collapsed {
                    0.
                } else {
                    shown.next().unwrap()
                }
            })
            .collect(),
        missing,
    )
}

// Every move is evaluated from the mouse-down snapshot, so reversing a preview
// restores both the original hidden state and the preferred sizes.
fn preview_resize(
    start: &[AxisChild],
    handle: usize,
    open: Option<(usize, bool)>,
    length: f32,
    target: f32,
    mut can_collapse: impl FnMut(usize) -> bool,
) -> Vec<AxisChild> {
    let mut result = start.to_vec();
    let (original_lengths, _) = child_lengths(start, length);
    if start
        .iter()
        .zip(&original_lengths)
        .any(|(child, length)| !child.collapsed && *length < f32::from(child.min) - 1.)
    {
        return result;
    }
    let mut active: Vec<usize> = start
        .iter()
        .enumerate()
        .filter_map(|(index, child)| (!child.collapsed).then_some(index))
        .collect();
    let mut lengths: Vec<f32> = active
        .iter()
        .map(|index| original_lengths[*index])
        .collect();
    let mut shut = None;
    let (handle, local_target) = if let Some((child, forward)) = open {
        let origin: f32 = original_lengths[..child].iter().sum();
        let desired = if forward {
            target - origin
        } else {
            origin - target
        };
        if desired <= f32::from(start[child].min) / 2. {
            return result;
        }
        result[child].collapsed = false;
        if active.is_empty() {
            return result;
        }
        // Hidden entries never become donors.
        let position = active.partition_point(|index| *index < child);
        let desired = desired.max(start[child].min.into());
        active.insert(position, child);
        lengths.insert(position, 0.);
        if forward && position + 1 < active.len() {
            (position, desired)
        } else {
            (position - 1, lengths[position - 1] - desired)
        }
    } else {
        let Some(position) = active.iter().position(|index| *index == handle) else {
            return result;
        };
        if position + 1 >= active.len() {
            return result;
        }
        let origin: f32 = original_lengths[..handle].iter().sum();
        let local = target - origin;
        let pair = lengths[position] + lengths[position + 1];
        let donor = if local < lengths[position] {
            position
        } else {
            position + 1
        };
        let child = active[donor];
        let raw = if donor == position {
            local
        } else {
            pair - local
        };
        if raw < f32::from(start[child].min) / 2. && can_collapse(child) {
            result[child].collapsed = true;
            shut = Some(child);
            (
                position,
                if donor == position {
                    local.min(0.)
                } else {
                    pair - raw.min(0.)
                },
            )
        } else {
            // Keep the established ordinary resize arithmetic, including
            // successor traversal and minimum-constrained share conversion.
            let mut sizes: Vec<_> = active.iter().map(|index| start[*index].size).collect();
            let mins: Vec<_> = active
                .iter()
                .map(|index| start[*index].min.into())
                .collect();
            if resize_sizes(&mut sizes, position, length, local, &mins) {
                for (index, size) in active.iter().zip(sizes) {
                    result[*index].size = size;
                }
            }
            return result;
        }
    };
    let before_transfer = lengths.clone();
    let forward = local_target > lengths[handle];
    let receiver = if forward { handle } else { handle + 1 };
    let mut remaining = (local_target - lengths[handle]).abs();
    let flexible_pair = open.is_none()
        && matches!(start[active[handle]].size, AxisSize::Flex(_))
        && matches!(start[active[handle + 1]].size, AxisSize::Flex(_));
    let count = if forward {
        active.len() - handle - 1
    } else {
        handle + 1
    };
    for offset in 0..count {
        let donor = if forward {
            handle + 1 + offset
        } else {
            handle - offset
        };
        let index = active[donor];
        if flexible_pair && matches!(start[index].size, AxisSize::Fixed(_)) {
            continue;
        }
        let min = if Some(index) == shut {
            0.
        } else {
            f32::from(start[index].min)
        };
        let transfer = remaining.min((lengths[donor] - min).max(0.));
        lengths[donor] -= transfer;
        lengths[receiver] += transfer;
        remaining -= transfer;
        if remaining <= 0. {
            break;
        }
    }
    // TODO(snap-hide-rounding): Roundoff can refuse reopening when the donor
    // has exactly the minimum room. A focused rounding fix unblocks it.
    if let Some((child, _)) = open
        && lengths[active.iter().position(|index| *index == child).unwrap()]
            < f32::from(start[child].min)
    {
        return start.to_vec();
    }
    let weight: f32 = active
        .iter()
        .filter_map(|index| match start[*index].size {
            AxisSize::Flex(weight) if !result[*index].collapsed => Some(weight),
            _ => None,
        })
        .sum();
    let flexible_length: f32 = active
        .iter()
        .zip(&lengths)
        .filter_map(|(index, length)| {
            (matches!(start[*index].size, AxisSize::Flex(_)) && !result[*index].collapsed)
                .then_some(*length)
        })
        .sum();
    for (position, index) in active.into_iter().enumerate() {
        if result[index].collapsed {
            continue;
        }
        result[index].size = match start[index].size {
            AxisSize::Flex(_) if flexible_length > 0. => {
                AxisSize::Flex(lengths[position] * weight / flexible_length)
            }
            AxisSize::Fixed(_)
                if position == handle
                    || position == handle + 1
                    || lengths[position] != before_transfer[position] =>
            {
                AxisSize::Fixed(px(lengths[position]))
            }
            size => size,
        };
    }
    result
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
