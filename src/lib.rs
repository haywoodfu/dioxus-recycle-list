//! A dynamic-height virtualized list component for Dioxus.
//!
//! Renders only the visible slice of a large list plus a configurable buffer,
//! using a virtual canvas (absolute positioning + translateY) to preserve the
//! correct total scroll height without top/bottom spacer hacks.

use dioxus::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn next_id() -> String {
    format!("recycle-list-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A single virtualized item with its computed pixel position.
#[derive(Debug, Clone, PartialEq)]
struct VirtualItem {
    index: usize,
    start: u32,
    size: u32,
}

impl VirtualItem {
    fn end(&self) -> u32 {
        self.start + self.size
    }
}

/// Parsed scroll message received from the JS bridge.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrollMsg {
    offset: u32,
    viewport: u32,
    is_scrolling: bool,
}

// ---------------------------------------------------------------------------
// Measurements
// ---------------------------------------------------------------------------

/// Build a flat list of `VirtualItem`s from the size cache.
///
/// Unmeasured items are sized by:
/// 1. The user-provided `estimate_size` callback (if any), or
/// 2. The running average of all measured items (adaptive estimation), or
/// 3. 100 px as a final fallback.
fn compute_measurements(
    count: usize,
    cache: &HashMap<usize, u32>,
    estimate_size: Option<&dyn Fn(usize) -> u32>,
) -> Vec<VirtualItem> {
    let adaptive = if estimate_size.is_none() && !cache.is_empty() {
        let sum: u64 = cache.values().map(|&v| v as u64).sum();
        Some(((sum / cache.len() as u64).max(1)) as u32)
    } else {
        None
    };

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let size = cache.get(&i).copied().unwrap_or_else(|| {
            estimate_size.map(|f| f(i)).unwrap_or(adaptive.unwrap_or(100))
        });
        let start = result.last().map(|m: &VirtualItem| m.end()).unwrap_or(0);
        result.push(VirtualItem { index: i, start, size });
    }
    result
}

/// Return the virtual items that should be rendered given the current scroll
/// position, viewport size, and overscan (buffer in item counts).
fn get_virtual_items(
    measurements: &[VirtualItem],
    scroll_offset: u32,
    viewport_size: u32,
    buffer: usize,
) -> Vec<VirtualItem> {
    if measurements.is_empty() || viewport_size == 0 {
        return Vec::new();
    }

    // Binary-search for the first item at or before the scroll offset.
    let start_idx = measurements
        .binary_search_by(|item| item.start.cmp(&scroll_offset))
        .unwrap_or_else(|idx| idx.saturating_sub(1));

    let end_scroll = scroll_offset.saturating_add(viewport_size);
    let mut end_idx = start_idx;
    let last = measurements.len() - 1;
    while end_idx < last && measurements[end_idx].end() < end_scroll {
        end_idx += 1;
    }

    // Apply overscan buffer.
    let render_start = start_idx.saturating_sub(buffer);
    let render_end = (end_idx + buffer).min(last);

    measurements[render_start..=render_end].to_vec()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Props for [`RecycleList`].
pub struct RecycleListProps<'a, T, F>
where
    F: Fn(&T, usize) -> Element,
{
    /// The data slice to virtualize.
    pub items: &'a [T],
    /// Number of extra items to render above and below the visible viewport
    /// (overscan / buffer in item counts, not pixels).
    pub buffer: usize,
    /// Renders a single item given a reference to its data and its absolute index.
    pub render_item: F,
    /// Optional per-index height estimate (px) used before the item is measured.
    /// When omitted the component uses an adaptive average of measured heights.
    pub estimate_size: Option<fn(usize) -> u32>,
}

/// A dynamic-height virtualized list for Dioxus.
///
/// Only the visible slice plus `buffer` rows are present in the DOM.
/// The total scroll height is preserved with a virtual canvas (relative
/// container + absolute inner strip + `translateY`), so the browser's
/// scrollbar behaves identically to a fully-rendered list.
///
/// # Scroll corrections
/// When an item whose rendered height differs from its estimated height sits
/// *above* the current scroll position, the component adjusts `scrollTop`
/// automatically to prevent content from jumping.  Adjustments are deferred
/// during active scrolling and applied once the user stops.
///
/// # Accessibility
/// Each rendered row receives `aria-setsize` and `aria-posinset` attributes
/// so screen readers can announce the total list size even though only a
/// subset of items is in the DOM.
///
/// # Example
///
/// ```rust
/// use dioxus::prelude::*;
/// use dioxus_recycle_list::RecycleList;
///
/// #[component]
/// fn Demo() -> Element {
///     let items: Vec<String> = (0..10_000).map(|i| format!("Row {i}")).collect();
///     rsx! {
///         RecycleList {
///             items: &items,
///             buffer: 8,
///             render_item: |item: &String, _idx| rsx! { div { "{item}" } },
///         }
///     }
/// }
/// ```
#[allow(non_snake_case)]
pub fn RecycleList<T: PartialEq + 'static, F>(props: RecycleListProps<'_, T, F>) -> Element
where
    F: Fn(&T, usize) -> Element,
{
    let RecycleListProps { items, buffer, render_item, estimate_size } = props;
    let count = items.len();

    // Stable container ID – never changes after first render.
    let container_id = use_memo(|| next_id());

    // --- Reactive scroll / viewport state ---
    let mut scroll_offset = use_signal(|| 0u32);
    let mut viewport_size = use_signal(|| 600u32);
    let mut is_scrolling = use_signal(|| false);

    // Frozen total size while scrolling to stop the scrollbar from drifting.
    let mut stable_total_size: Signal<Option<u32>> = use_signal(|| None);

    // Accumulated correction for items that resize above the viewport.
    let mut scroll_adjustments = use_signal(|| 0i32);
    // Adjustments deferred until scrolling stops.
    let mut deferred_adjustments = use_signal(|| 0i32);

    // Measured item sizes, keyed by index.
    let mut size_cache: Signal<HashMap<usize, u32>> = use_signal(HashMap::new);

    // Keep count reactive so the memo re-runs when items slice length changes.
    let mut count_sig = use_signal(|| count);
    if *count_sig.peek() != count {
        count_sig.set(count);
        // Prune cache entries that are no longer valid.
        size_cache.with_mut(|c| c.retain(|&k, _| k < count));
    }

    // --- Measurements memo ---
    // Recomputes whenever count or the size cache changes.
    let measurements = use_memo(move || {
        let n = count_sig();
        let cache = size_cache.read();
        compute_measurements(n, &cache, estimate_size.map(|f| f as &dyn Fn(usize) -> u32))
    });

    // --- JS scroll bridge ---
    // Attaches a scroll listener to the container element via dioxus eval.
    // Sends { offset, viewport, isScrolling } messages to the Rust side.
    // The script blocks on a second recv() for cleanup (called on drop).
    use_effect(move || {
        let script = r#"
            const container = document.getElementById(await dioxus.recv());
            if (!container) return;

            let scrollEndTimer = null;
            let lastOffset = null;

            function publish(isScrolling) {
                const scroll = Math.round(container.scrollTop);
                // Deduplicate: skip if offset hasn't changed and we're not scrolling
                if (!isScrolling && scroll === lastOffset) return;
                lastOffset = scroll;
                const viewport = Math.min(container.clientHeight, window.innerHeight) || 600;
                dioxus.send({ offset: scroll, viewport: viewport, isScrolling: isScrolling });
            }

            function onScroll() {
                if (scrollEndTimer !== null) clearTimeout(scrollEndTimer);
                publish(true);
                // Debounce scroll-end detection (150 ms after last event)
                scrollEndTimer = setTimeout(() => {
                    scrollEndTimer = null;
                    publish(false);
                }, 150);
            }

            // Initial publish (handles page already scrolled on mount)
            publish(false);

            container.addEventListener("scroll", onScroll, { passive: true });
            window.addEventListener("resize", () => publish(false), { passive: true });

            // Wait for the Rust side to signal teardown (eval dropped on unmount)
            await dioxus.recv();
            if (scrollEndTimer !== null) clearTimeout(scrollEndTimer);
            container.removeEventListener("scroll", onScroll);
        "#;

        let mut eval = document::eval(script);
        let _ = eval.send(container_id.peek().clone());

        spawn(async move {
            while let Ok(msg) = eval.recv::<ScrollMsg>().await {
                let was_scrolling = *is_scrolling.peek();

                if msg.is_scrolling && !was_scrolling {
                    // New scroll gesture: reset correction accumulators and freeze
                    // total size so the scrollbar length stays stable.
                    scroll_adjustments.set(0);
                    deferred_adjustments.set(0);
                    let frozen = measurements.peek().last().map(|m| m.end()).unwrap_or(0);
                    stable_total_size.set(Some(frozen));
                }

                if !msg.is_scrolling && was_scrolling {
                    // Scroll ended: unfreeze total size.
                    stable_total_size.set(None);

                    // Apply any corrections that were deferred during the gesture.
                    let deferred = *deferred_adjustments.peek();
                    if deferred != 0 {
                        let new_scroll = (msg.offset as i32 + deferred).max(0) as u32;
                        deferred_adjustments.set(0);
                        viewport_size.set(msg.viewport);
                        is_scrolling.set(false);
                        scroll_offset.set(new_scroll);
                        let cid = container_id.peek().clone();
                        sync_container_scroll(cid, new_scroll).await;
                        continue;
                    }
                }

                viewport_size.set(msg.viewport);
                is_scrolling.set(msg.is_scrolling);
                scroll_offset.set(msg.offset);
            }
        });
    });

    // --- Render ---

    // Reading scroll_offset and viewport_size subscribes this component so it
    // re-renders on scroll.  Reading measurements subscribes on height changes.
    let current_scroll = *scroll_offset.read();
    let current_viewport = *viewport_size.read();
    let m = measurements.read();

    let total_size = match *stable_total_size.read() {
        Some(frozen) => frozen,
        None => m.last().map(|i| i.end()).unwrap_or(0),
    };
    let canvas_height = total_size.max(current_viewport);

    let virtual_items = get_virtual_items(&m, current_scroll, current_viewport, buffer);
    let top_offset = virtual_items.first().map(|i| i.start).unwrap_or(0);
    let set_size = count.to_string();

    // onresize callback: measures the actual rendered height of each row and
    // stores it in the size cache.  If the measured height differs from the
    // estimate and the item sits above the viewport, adjusts scrollTop to
    // prevent content from jumping.
    let onresize = move |idx: usize| {
        move |event: Event<ResizeData>| {
            let rect = event.data().get_content_box_size().unwrap_or_default();
            let new_size = rect.height.max(1.0).round() as u32;

            let m_peek = measurements.peek();
            let Some(item) = m_peek.get(idx) else { return };

            let old_size = {
                let cache = size_cache.peek();
                cache.get(&idx).copied().unwrap_or(item.size)
            };

            let delta = new_size as i32 - old_size as i32;
            // Ignore sub-pixel noise (<= 2 px) to avoid render loops.
            if delta.abs() <= 2 {
                return;
            }

            let item_start = item.start;
            drop(m_peek);

            size_cache.write().insert(idx, new_size);

            // Only adjust scroll when the resized item is above the viewport.
            let adjusted_scroll =
                (*scroll_offset.peek() as i32 + *scroll_adjustments.peek()).max(0) as u32;
            let is_above = item_start < adjusted_scroll;
            let scrolling_now = *is_scrolling.peek();

            if is_above && !scrolling_now {
                let adj = *scroll_adjustments.peek();
                scroll_adjustments.set(adj + delta);
                let new_scroll = (*scroll_offset.peek() as i32 + delta).max(0) as u32;
                scroll_offset.set(new_scroll);
                let cid = container_id.peek().clone();
                spawn(async move {
                    sync_container_scroll(cid, new_scroll).await;
                });
            } else if is_above && scrolling_now {
                let deferred = *deferred_adjustments.peek();
                deferred_adjustments.set(deferred + delta);
            }
        }
    };

    rsx! {
        div {
            id: container_id,
            class: "recycle-list-container",
            role: "list",
            tabindex: "0",

            // Virtual canvas: full scroll height, relative positioning root.
            div {
                style: "position: relative; height: {canvas_height}px; width: 100%;",

                // Visible strip: absolute, shifted down by translateY instead of
                // a top spacer so the browser can GPU-composite the transform.
                div {
                    style: "position: absolute; inset: 0 auto auto 0; width: 100%; transform: translateY({top_offset}px); will-change: transform;",

                    {virtual_items.iter().map(|item| {
                        let idx = item.index;
                        rsx! {
                            div {
                                key: "{idx}",
                                role: "listitem",
                                "data-virtual-index": "{idx}",
                                "aria-setsize": "{set_size}",
                                "aria-posinset": "{idx + 1}",
                                onresize: onresize(idx),
                                {render_item(&items[idx], idx)}
                            }
                        }
                    })}
                }
            }
        }
    }
}

/// Backward-compatible helper with the previous positional-argument API.
pub fn recycle_list<T: PartialEq + 'static, F>(
    items: &[T],
    buffer: usize,
    render_item: F,
) -> Element
where
    F: Fn(&T, usize) -> Element,
{
    RecycleList(RecycleListProps {
        items,
        buffer,
        render_item,
        estimate_size: None,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Programmatically set `scrollTop` on the container without firing a scroll
/// event (the container's own listener will pick it up naturally on the next
/// paint, which is fine).
async fn sync_container_scroll(container_id: String, scroll_top: u32) {
    let eval = document::eval(
        r#"
        const id = await dioxus.recv();
        const targetScroll = await dioxus.recv();
        const container = document.getElementById(id);
        if (container) container.scrollTop = targetScroll;
        "#,
    );
    let _ = eval.send(container_id);
    let _ = eval.send(scroll_top);
}
