#[cfg(target_arch = "wasm32")]
use dioxus::dioxus_core::use_drop;
use dioxus::prelude::*;
#[cfg(target_arch = "wasm32")]
use dioxus_web::WebEventExt;
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::closure::Closure;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlElement;

#[cfg(target_arch = "wasm32")]
type WindowScrollClosure = Closure<dyn FnMut(web_sys::Event)>;

pub struct RecycleListProps<'a, T, F>
where
    F: Fn(&T, usize) -> Element,
{
    pub items: &'a [T],
    pub buffer: usize,
    pub render_item: F,
}

/// create a cycle list for large datasets, will recylce items as you scroll
#[allow(non_snake_case)]
pub fn RecycleList<T: PartialEq + 'static, F>(props: RecycleListProps<'_, T, F>) -> Element
where
    F: Fn(&T, usize) -> Element,
{
    let RecycleListProps {
        items,
        buffer,
        render_item,
    } = props;
    let total = items.len();

    // 1. Subscribe to page scroll and keep list-relative scroll position in sync.
    // 2. Track dynamic item heights and rebuild prefix sums when heights change.
    // 3. Resolve visible render range from current scroll plus viewport and buffer.
    // 4. Compute top and bottom spacer heights to preserve total scroll height.
    // 5. Render only visible rows and update measured heights on row mount.

    // estimated each item height to 100px
    let estimated_item_height: u32 = 100;

    // Scroll position signal (relative to list top, in px)
    #[allow(unused_mut)]
    let mut scroll_top = use_signal(|| 0);
    #[cfg(target_arch = "wasm32")]
    let mut container_el: Signal<Option<HtmlElement>> = use_signal(|| None);
    #[cfg(target_arch = "wasm32")]
    let window_scroll_listener = use_signal::<Option<WindowScrollClosure>>(|| None);
    #[cfg(target_arch = "wasm32")]
    let mut container_page_top = use_signal::<Option<f64>>(|| None);
    #[cfg(target_arch = "wasm32")]
    let viewport_height = use_signal(|| estimated_item_height.saturating_mul(8));
    let mut measured_heights = use_signal(|| vec![estimated_item_height; total]);

    // Keep height cache length aligned with current items.
    if measured_heights.read().len() != total {
        measured_heights.with_mut(|heights| heights.resize(total, estimated_item_height));
    }

    // Get container viewport height and add scroll listener to get the scroll position
    #[cfg(target_arch = "wasm32")]
    {
        use_effect({
            let container_el = container_el.clone();
            let mut window_scroll_listener = window_scroll_listener.clone();
            let mut scroll_top = scroll_top.clone();
            let container_page_top = container_page_top.clone();
            let mut viewport_height = viewport_height.clone();
            move || {
                if container_el.read().is_none() || window_scroll_listener.read().is_some() {
                    return;
                }

                let Some(window) = web_sys::window() else {
                    return;
                };

                let viewport_px = window
                    .inner_height()
                    .ok()
                    .and_then(|h| h.as_f64())
                    .map(|h| h.max(1.0).round() as u32)
                    .unwrap_or(estimated_item_height.saturating_mul(8));
                viewport_height.set(viewport_px);

                // Sync once on mount in case page is already scrolled.
                if let Some(list_top_in_page) = *container_page_top.read() {
                    let scroll_y = window.scroll_y().unwrap_or(0.0);
                    let relative_scroll = (scroll_y - list_top_in_page).max(0.0) as u32;
                    if relative_scroll != *scroll_top.read() {
                        scroll_top.set(relative_scroll);
                    }
                }

                let container_page_top_for_cb = container_page_top.clone();
                let mut scroll_top_for_cb = scroll_top.clone();
                let mut viewport_height_for_cb = viewport_height.clone();
                let cb = Closure::wrap(Box::new(move |_evt: web_sys::Event| {
                    let Some(window) = web_sys::window() else {
                        return;
                    };
                    if let Some(viewport_px) = window
                        .inner_height()
                        .ok()
                        .and_then(|h| h.as_f64())
                        .map(|h| h.max(1.0).round() as u32)
                    {
                        if viewport_px != *viewport_height_for_cb.read() {
                            viewport_height_for_cb.set(viewport_px);
                        }
                    }
                    let Some(list_top_in_page) = *container_page_top_for_cb.read() else {
                        return;
                    };

                    let scroll_y = window.scroll_y().unwrap_or(0.0);
                    let relative_scroll = (scroll_y - list_top_in_page).max(0.0) as u32;
                    if relative_scroll != *scroll_top_for_cb.read() {
                        scroll_top_for_cb.set(relative_scroll);
                    }
                }) as Box<dyn FnMut(web_sys::Event)>);

                let _ =
                    window.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
                window_scroll_listener.set(Some(cb));
            }
        });

        use_drop({
            let mut window_scroll_listener = window_scroll_listener.clone();
            move || {
                if let (Some(window), Some(cb)) = (web_sys::window(), window_scroll_listener.take())
                {
                    let _ = window
                        .remove_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
                }
            }
        });
    }

    // Dynamic-height virtualization
    // Get the viewport height and buffer height. Height need to render = viewport + buffer.
    let current_scroll = *scroll_top.read();
    let viewport_px = {
        #[cfg(target_arch = "wasm32")]
        {
            (*viewport_height.read()).max(estimated_item_height)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            estimated_item_height.saturating_mul(8)
        }
    };
    let buffer_px = (buffer as u32).saturating_mul(estimated_item_height);

    // Rebuild prefix sums when measured heights change.
    // each item has a height, and the prefix sum is the cumulative height of all items up to that point.
    let prefix_and_total = use_memo({
        let measured_heights = measured_heights.clone();
        move || {
            let heights = measured_heights.read();
            let mut prefix: Vec<u32> = Vec::with_capacity(heights.len() + 1);
            prefix.push(0);
            for height in heights.iter() {
                let next = prefix
                    .last()
                    .copied()
                    .unwrap_or(0)
                    .saturating_add((*height).max(1));
                prefix.push(next);
            }
            let total_height = *prefix.last().unwrap_or(&0);
            (Arc::new(prefix), total_height)
        }
    });

    // Calculate visible range from scroll position using prefix sums.
    let (prefix, total_height) = prefix_and_total();
    let prefix: &[u32] = prefix.as_ref();

    let (render_start, mut end_idx) = if total == 0 {
        (0, 0)
    } else {
        // find the item at a given y position
        let item_at = |y: u32| prefix.partition_point(|&acc| acc <= y).saturating_sub(1);

        // find the index at the current scroll position
        let clamped_scroll = current_scroll.min(total_height.saturating_sub(1));
        let render_start = item_at(clamped_scroll.saturating_sub(buffer_px));

        // find the index at the end of the viewport + buffer
        let end_target = clamped_scroll
            .saturating_add(viewport_px)
            .saturating_add(buffer_px);
        let end_idx = prefix.partition_point(|&acc| acc < end_target).min(total);

        (render_start, end_idx)
    };

    if total > 0 && end_idx <= render_start {
        end_idx = (render_start + 1).min(total);
    }
    // set the top and bottom spacers, make scroll view as actual height as it should be
    let top_spacer = prefix[render_start];
    let bottom_spacer = total_height.saturating_sub(prefix[end_idx]);
    rsx! {
        div {
            class: "cycle-list-container",
            onmounted: move |_event: Event<MountedData>| {
                #[cfg(target_arch = "wasm32")]
                {
                    let element = _event.as_web_event();
                    if let Ok(html_el) = element.dyn_into::<HtmlElement>() {
                        if let Some(window) = web_sys::window() {
                            let scroll_y = window.scroll_y().unwrap_or(0.0);
                            let rect = html_el.get_bounding_client_rect();
                            container_page_top.set(Some(rect.top() + scroll_y));
                        }
                        container_el.set(Some(html_el));
                    }
                }
            },

            // Top spacer
            div { style: "height:{top_spacer}px; width:1px;" }

            // Render visible slice using the provided render_item
            {
                items
                    .iter()
                    .skip(render_start)
                    .take(end_idx - render_start)
                    .enumerate()
                    .map(|(i, item)| {
                        let idx = render_start + i;
                        let _measured_heights_for_item = measured_heights.clone();

                        rsx! {
                            div {
                                key: "{idx}",
                                onmounted: move |_event: Event<MountedData>| {
                                    #[cfg(target_arch = "wasm32")]
                                    {
                                        let mut measured_heights_for_item = _measured_heights_for_item.clone();
                                        spawn(async move {
                                            let rect = _event.get_client_rect().await.unwrap_or_default();
                                            let measured = rect.height().max(1.0).round() as u32;
                                            measured_heights_for_item
                                                .with_mut(|heights| {
                                                    if idx < heights.len() && heights[idx] != measured {
                                                        heights[idx] = measured;
                                                    }
                                                });
                                        });
                                    }
                                },
                                {render_item(item, idx)}
                            }
                        }
                    })
            }
            // Bottom spacer
            div { style: "height:{bottom_spacer}px; width:1px;" }
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
    })
}
