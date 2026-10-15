//! Replicas that receive the same edits converge, no matter the delivery order.

use opendox_model::Document;
use proptest::prelude::*;

const REPLICAS: usize = 3;

/// Strings chosen to stress grapheme handling: combining marks that merge with whatever precedes
/// them, multi-char emoji, CR/LF pairs, and paragraph breaks.
const SNIPPETS: &[&str] = &[
    "a",
    "xyz",
    " ",
    "\n",
    "\r",
    "é",
    "e\u{301}",
    "\u{301}",
    "👨‍👩‍👧",
    "日本",
    "مرحبا",
];

#[derive(Debug, Clone)]
enum Op {
    /// Insert a snippet at a position chosen as a fraction of the replica's boundaries.
    Insert {
        replica: usize,
        at: f64,
        snippet: usize,
    },
    /// Delete between two boundaries chosen as fractions.
    Delete { replica: usize, from: f64, to: f64 },
    /// Deliver one previously produced update to a replica (any order, duplicates allowed).
    Deliver { update: f64, to: usize },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..REPLICAS, 0.0..=1.0, 0..SNIPPETS.len())
            .prop_map(|(replica, at, snippet)| Op::Insert { replica, at, snippet }),
        1 => (0..REPLICAS, 0.0..=1.0, 0.0..=1.0)
            .prop_map(|(replica, from, to)| Op::Delete { replica, from, to }),
        2 => (0.0..1.0, 0..REPLICAS).prop_map(|(update, to)| Op::Deliver { update, to }),
    ]
}

/// Picks a grapheme boundary of `doc` by walking from 0, `frac` of the way through.
fn boundary_at(doc: &Document, frac: f64) -> usize {
    let mut all = vec![0];
    let mut pos = 0;
    while let Some(next) = doc.next_grapheme_boundary(pos) {
        all.push(next);
        pos = next;
    }
    let i = ((all.len() - 1) as f64 * frac).round() as usize;
    all[i]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn replicas_converge(ops in prop::collection::vec(op(), 1..80)) {
        let docs: Vec<Document> = (0..REPLICAS)
            .map(|i| Document::with_replica_id(i as u64 + 1).unwrap())
            .collect();
        let mut updates: Vec<Vec<u8>> = Vec::new();

        for op in ops {
            match op {
                Op::Insert { replica, at, snippet } => {
                    let doc = &docs[replica];
                    let before = doc.version();
                    doc.insert(boundary_at(doc, at), SNIPPETS[snippet]).unwrap();
                    updates.push(doc.export_updates_since(&before).unwrap());
                }
                Op::Delete { replica, from, to } => {
                    let doc = &docs[replica];
                    let (a, b) = (boundary_at(doc, from), boundary_at(doc, to));
                    let before = doc.version();
                    doc.delete(a.min(b)..a.max(b)).unwrap();
                    updates.push(doc.export_updates_since(&before).unwrap());
                }
                Op::Deliver { update, to } => {
                    if !updates.is_empty() {
                        let i = (updates.len() as f64 * update) as usize;
                        docs[to].import(&updates[i]).unwrap();
                    }
                }
            }
        }

        // Finally every replica receives every update, in production order.
        for doc in &docs {
            for u in &updates {
                doc.import(u).unwrap();
            }
        }

        let expected = docs[0].text();
        for doc in &docs[1..] {
            prop_assert_eq!(doc.text(), expected.clone());
        }

        // A brand-new replica built only from the updates agrees too.
        let fresh = Document::new();
        for u in updates.iter().rev() {
            fresh.import(u).unwrap();
        }
        prop_assert_eq!(fresh.text(), expected);
    }
}
