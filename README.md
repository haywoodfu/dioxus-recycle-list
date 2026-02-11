# dioxus-recycle-list

Dynamic-height virtualization component for Dioxus.

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

A web demo binary is available in `preview/`, but it is opt-in and does not
participate in the default build.

Run it with Dioxus CLI:

```bash
dx serve --platform web --features preview --bin preview
```
