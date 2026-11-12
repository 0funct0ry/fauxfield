//! DataTable benchmark (M10.03): a scroll through a 100k-row Dataset, per frame, against the
//! SPEC §15 frame budget (8 ms, p95). Each frame does the table's own per-frame work on the
//! model side: derive the visible window, report it, and read and format every visible cell
//! the way `DatasetTable::render_td` does. Also times the one-off 100k-row sort.
//! Run: `cargo bench -p fauxfield-gui --bench data_table` (release; asserts only in release).
use std::time::Instant;

use criterion::{Criterion, black_box};
use fauxfield_gui::data_table::{Column, TableModel, TableOptions};
use fauxfield_gui::dataset_ui::{cell_text, sort_permutation};
use serde_json::{Value, json};

const ROWS: usize = 100_000;
const ROW_H: f32 = 24.0;
const VIEW_H: f32 = 900.0;
const ROLES: [&str; 3] = ["admin", "user", "guest"];

fn rows() -> Vec<Value> {
    (0..ROWS)
        .map(|i| {
            json!({
                "id": i,
                "name": format!("user {}", (i * 7919) % ROWS),
                "email": format!("u{i}@example.test"),
                "score": (i * 31) % 1000,
                "role": ROLES[i % 3],
                "active": i % 2 == 0,
            })
        })
        .collect()
}

fn model() -> TableModel {
    let mut m = TableModel::new(TableOptions::default());
    m.set_columns(
        ["id", "name", "email", "score", "role", "active"]
            .iter()
            .map(|k| Column::new(*k, *k).sortable().width(160.0))
            .collect(),
    );
    m.set_rows(ROWS);
    m
}

/// One frame at scroll offset `y`.
fn frame(m: &mut TableModel, data: &[Value], y: f32) -> usize {
    let first = (y / ROW_H) as usize;
    let last = (first + (VIEW_H / ROW_H) as usize + 2).min(ROWS);
    black_box(m.report_visible(first..last, 0..6));
    let mut bytes = 0;
    for row in data.iter().take(last).skip(first) {
        for c in &m.columns {
            bytes += cell_text(row.get(&c.key).unwrap_or(&Value::Null)).len();
        }
    }
    bytes
}

fn frame_p95(data: &[Value]) {
    let mut m = model();
    let frames = 20_000;
    let max_y = (ROWS as f32 - VIEW_H / ROW_H) * ROW_H;
    let mut s: Vec<u128> = Vec::with_capacity(frames);
    for i in 0..frames {
        // A fast flick: 600 rows per frame, wrapping.
        let y = (i as f32 * 600.0 * ROW_H) % max_y;
        let st = Instant::now();
        black_box(frame(&mut m, data, y));
        s.push(st.elapsed().as_nanos());
    }
    s.sort_unstable();
    let p95 = s[frames * 95 / 100];
    println!("data_table_scroll_frame_p95: {p95} ns (budget 8,000,000 ns)");
    if !cfg!(debug_assertions) {
        assert!(p95 < 8_000_000, "frame p95 {p95} ns >= 8 ms");
    }
}

fn main() {
    let data = rows();
    let mut c = Criterion::default().configure_from_args();
    c.bench_function("scroll_frame_100k", |b| {
        let mut m = model();
        let mut i = 0usize;
        b.iter(|| {
            i += 1;
            black_box(frame(
                &mut m,
                &data,
                ((i * 600) % (ROWS - 40)) as f32 * ROW_H,
            ))
        })
    });
    c.bench_function("sort_100k_by_name", |b| {
        b.iter(|| black_box(sort_permutation(&data, "name", false)))
    });
    c.final_summary();
    frame_p95(&data);
}
