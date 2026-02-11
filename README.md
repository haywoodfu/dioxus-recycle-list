# dioxus-recycle-list

`dioxus-recycle-list` is a dynamic-height virtualized list component for Dioxus.

## Why Recycle List

By default, a normal list renders all items at once when the page loads.  
With large datasets(>300), this can cause noticeable startup lag, especially on lower-performance mobile devices.

To improve rendering performance, this project applies the `recycle list` (virtualized list) approach:

- Render only a limited number of items around the visible viewport
- Recycle and reuse item nodes while scrolling
- Keep the user experience consistent with a normal list (same scrolling and content behavior)

## Preview

![dioxus-recycle-list preview](./preview/preview.webp)

## Usage

```rust
use dioxus_recycle_list::{RecycleList, RecycleListProps};

let view = RecycleList(RecycleListProps {
    items: &rows,
    buffer: 8,
    render_item: |row, idx| {
        rsx! { div { key: "{idx}", "{row.title}" } }
    },
});
```

You can also use the compatibility helper:

```rust
use dioxus_recycle_list::recycle_list;

let view = recycle_list(&rows, 8, |row, idx| {
    rsx! { div { key: "{idx}", "{row.title}" } }
});
```

- `items: &[T]`: borrowed row data.
- `buffer: usize`: extra row count rendered before/after viewport.
- `render_item: Fn(&T, usize) -> Element`: row renderer.

## Preview demo

`preview/` provides an optional demo and is excluded from the default build.

Run it with Dioxus CLI:

```bash
dx serve --platform web --features preview --bin preview
```
