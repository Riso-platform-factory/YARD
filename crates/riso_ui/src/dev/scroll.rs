//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    AnyElement, App, Bounds, Div, ElementId, Empty, InteractiveElement, IntoElement,
    ListSizingBehavior, ParentElement, Pixels, Point, RenderOnce, ScrollHandle,
    StatefulInteractiveElement, StyleRefinement, Styled, UniformListDecoration,
    UniformListScrollHandle, Window, div, uniform_list,
};
use std::ops::Range;
use ui::{ScrollAxes, Scrollbars, WithScrollbar};

type Rows = Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

/// A vertical scroll container with Zed's scrollbar.
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    handle: ScrollHandle,
    base: Div,
    children: Vec<AnyElement>,
}

impl ScrollArea {
    pub fn new(id: impl Into<ElementId>, handle: &ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle: handle.clone(),
            base: div(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scrollbar = Scrollbars::new(ScrollAxes::Vertical)
            .id((self.id.clone(), "scrollbar"))
            .tracked_scroll_handle(&self.handle)
            .notify_content();
        self.base
            .overflow_hidden()
            .child(
                div()
                    .id(self.id)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .children(self.children),
            )
            .custom_scrollbars(scrollbar, window, cx)
    }
}

struct RowsTrace {
    id: ElementId,
}

impl UniformListDecoration for RowsTrace {
    fn compute(
        &self,
        visible_range: Range<usize>,
        _: Bounds<Pixels>,
        _: Point<Pixels>,
        _: Pixels,
        _: usize,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        tracing::trace!(
            id = %self.id,
            first = visible_range.start,
            count = visible_range.len(),
            "rows drawn",
        );
        Empty.into_any_element()
    }
}

/// A virtualized vertical list of uniform-height rows with Zed's scrollbar.
#[derive(IntoElement)]
pub struct ScrollList {
    id: ElementId,
    count: usize,
    handle: UniformListScrollHandle,
    rows: Rows,
    base: Div,
}

impl ScrollList {
    pub fn new(
        id: impl Into<ElementId>,
        count: usize,
        handle: &UniformListScrollHandle,
        rows: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            count,
            handle: handle.clone(),
            rows: Box::new(rows),
            base: div(),
        }
    }
}

impl Styled for ScrollList {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for ScrollList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scrollbar = Scrollbars::new(ScrollAxes::Vertical)
            .id((self.id.clone(), "scrollbar"))
            .tracked_scroll_handle(&self.handle)
            .notify_content();
        self.base
            .overflow_hidden()
            .child(
                uniform_list(self.id.clone(), self.count, self.rows)
                    .with_sizing_behavior(ListSizingBehavior::Auto)
                    .size_full()
                    .track_scroll(&self.handle)
                    .with_decoration(RowsTrace { id: self.id }),
            )
            .custom_scrollbars(scrollbar, window, cx)
    }
}
