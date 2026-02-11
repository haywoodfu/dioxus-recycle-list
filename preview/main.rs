use dioxus::prelude::*;
use dioxus_recycle_list::{RecycleList, RecycleListProps};

#[derive(Clone, PartialEq)]
struct DemoRow {
    title: String,
    summary: String,
    extra_lines: usize,
}

fn build_rows() -> Vec<DemoRow> {
    (0..2000)
        .map(|i| DemoRow {
            title: format!("Item {}", i + 1),
            summary: format!("This is a preview row for virtualization. Index = {}", i),
            extra_lines: (i % 6) + 1,
        })
        .collect()
}

fn main() {
    dioxus::launch(App);
}

const PREVIEW_CSS: &str = r#"
    body {
        margin: 0;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        background: #f6f7fb;
        color: #1d2433;
    }
    .page {
        max-width: 860px;
        margin: 0 auto;
        padding: 24px 16px 48px;
    }
    .headline {
        margin: 0 0 8px;
        font-size: 28px;
        font-weight: 700;
        letter-spacing: -0.02em;
    }
    .sub {
        margin: 0 0 18px;
        color: #56607a;
        font-size: 14px;
    }
    .card {
        background: #ffffff;
        border: 1px solid #dde3f0;
        border-radius: 12px;
        margin: 8px 0;
        padding: 12px 14px;
        box-shadow: 0 2px 12px rgba(20, 34, 68, 0.06);
    }
    .card h3 {
        margin: 0 0 6px;
        font-size: 16px;
    }
    .card p {
        margin: 0;
        font-size: 14px;
        line-height: 1.45;
    }
    .card p + p {
        margin-top: 8px;
    }
"#;

#[allow(non_snake_case)]
fn App() -> Element {
    let rows = use_memo(build_rows);
    let rows_ref = rows.read();

    rsx! {
        style {
            "{PREVIEW_CSS}"
        }

        div { class: "page",
            h1 { class: "headline", "dioxus-recycle-list Preview" }
            p { class: "sub", "Scroll to verify dynamic-height virtualization with 2000 rows." }

            {
                RecycleList(RecycleListProps {
                    items: rows_ref.as_slice(),
                    buffer: 12,
                    render_item: move |row, idx| {
                        let extra_text = "Extra content to change row height. "
                            .repeat(row.extra_lines);
                        rsx! {
                            article { class: "card", key: "{idx}",
                                h3 { "#{idx + 1} - {row.title}" }
                                p { "{row.summary}" }
                                p { "{extra_text}" }
                            }
                        }
                    },
                })
            }
        }
    }
}
