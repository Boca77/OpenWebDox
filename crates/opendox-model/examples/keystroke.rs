//! Times what a keystroke costs the model on a long document (~115 pages, 400k chars):
//! `cargo run --release -p opendox-model --example keystroke`.
#![allow(clippy::expect_used)] // a benchmark harness, like the tests

use std::time::Instant;

use opendox_model::{Document, FormatChange};

fn main() {
    let para =
        "Alice was beginning to get very tired of sitting by her sister on the bank, and of \
                having nothing to do: once or twice she had peeped into the book her sister was \
                reading, but it had no pictures or conversations in it. "
            .repeat(3);
    let text = vec![para; 600].join("\n");
    let d = Document::new();
    d.insert(0, &text).expect("insert");
    let o = d.outline();
    // Some formatting, so marks aren't trivially empty.
    for i in 0..200 {
        let at = i * 1500;
        d.format_at(&o, at, at + 20, &FormatChange::Bold(true))
            .expect("bold");
    }
    let median = |name: &str, f: &mut dyn FnMut()| {
        let mut t: Vec<f64> = (0..15)
            .map(|_| {
                let s = Instant::now();
                f();
                s.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        t.sort_by(f64::total_cmp);
        println!("{name:>22}: {:7.2} ms", t[7]);
    };
    let o = d.outline();
    let pos = 30;
    median("insert_at", &mut || {
        d.insert_at(&o, pos, "a").expect("insert");
    });
    let o = d.outline();
    median("outline", &mut || {
        let _ = d.outline();
    });
    median("formats", &mut || {
        let _ = d.formats(&o);
    });
    median("char_format_at", &mut || {
        let _ = d.char_format_at(&o, pos, pos);
    });
    median("paragraph_format_at", &mut || {
        let _ = d.paragraph_format_at(&o, pos);
    });
    median("anchor_at", &mut || {
        let _ = d.anchor_at(&o, pos);
    });
    median("text", &mut || {
        let _ = d.text();
    });
}
