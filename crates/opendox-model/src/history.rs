//! Local undo and redo.

use std::sync::{Arc, Mutex, MutexGuard};

use loro::cursor::Cursor;
use loro::{UndoItemMeta, UndoManager};

use crate::document::{Anchor, Document};
use crate::error::ModelError;
use crate::outline::Outline;

/// Edits closer together than this (ms) undo as one step, so undo removes a burst of typing
/// rather than one character.
const MERGE_MS: i64 = 1000;
const MAX_STEPS: usize = 1000;

/// Undo and redo for one replica's own edits.
///
/// Only edits made on this replica are undone. Other people's edits stay, and undoing still
/// works after they've edited around (or inside) the text being undone.
///
/// Each step remembers the selection from before it, and undoing the step puts it back. Call
/// [`History::before_edit`] before every local edit.
pub struct History {
    undo: UndoManager,
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Selection to store with the next new step.
    selection: Option<(Cursor, Cursor)>,
    /// Selection stored with the step just undone or redone.
    restored: Option<(Cursor, Cursor)>,
}

impl History {
    /// Starts recording `doc`'s local edits. Create it after the replica id is final.
    pub fn new(doc: &Document) -> Self {
        let mut undo = UndoManager::new(&doc.doc);
        undo.set_merge_interval(MERGE_MS);
        undo.set_max_undo_steps(MAX_STEPS);
        let shared = Arc::new(Mutex::new(Shared::default()));
        let push = shared.clone();
        undo.set_on_push(Some(Box::new(move |_, _, _| {
            let mut meta = UndoItemMeta::new();
            if let Some((a, f)) = &lock(&push).selection {
                meta.add_cursor(a);
                meta.add_cursor(f);
            }
            meta
        })));
        let pop = shared.clone();
        undo.set_on_pop(Some(Box::new(move |_, _, meta| {
            let mut c = meta.cursors.into_iter().map(|c| c.cursor);
            lock(&pop).restored = c.next().zip(c.next());
        })));
        Self { undo, shared }
    }

    /// Call before a local edit with the selection as flat positions per `outline`.
    ///
    /// With `merge`, an edit soon after the previous one joins its undo step (a burst of
    /// typing). Without, it starts a step of its own: use that for formatting, commands, and
    /// typing somewhere new.
    pub fn before_edit(
        &mut self,
        doc: &Document,
        outline: &Outline,
        anchor: usize,
        focus: usize,
        merge: bool,
    ) {
        let a = doc.anchor_at(outline, anchor).ok();
        let f = doc.anchor_at(outline, focus).ok();
        lock(&self.shared).selection = a.zip(f).map(|(a, f)| (a.0, f.0));
        // Loro decides merging when the edit commits, by time since the step began; a zero
        // window forces a new step.
        self.undo
            .set_merge_interval(if merge { MERGE_MS } else { 0 });
    }

    /// Forgets every step, e.g. after loading a document so its setup can't be undone.
    pub fn clear(&self) {
        self.undo.clear();
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Undoes the last local step. Returns `None` if there was nothing to undo, otherwise the
    /// selection from before that step (when it can still be resolved).
    ///
    /// `anchor` and `focus` are the current selection, saved for redo.
    pub fn undo(
        &mut self,
        doc: &Document,
        outline: &Outline,
        anchor: usize,
        focus: usize,
    ) -> Result<Option<Option<(Anchor, Anchor)>>, ModelError> {
        self.step(doc, outline, anchor, focus, true)
    }

    /// Redoes the last undone step; like [`History::undo`].
    pub fn redo(
        &mut self,
        doc: &Document,
        outline: &Outline,
        anchor: usize,
        focus: usize,
    ) -> Result<Option<Option<(Anchor, Anchor)>>, ModelError> {
        self.step(doc, outline, anchor, focus, false)
    }

    fn step(
        &mut self,
        doc: &Document,
        outline: &Outline,
        anchor: usize,
        focus: usize,
        undo: bool,
    ) -> Result<Option<Option<(Anchor, Anchor)>>, ModelError> {
        self.before_edit(doc, outline, anchor, focus, false);
        lock(&self.shared).restored = None;
        let done = if undo {
            self.undo.undo()
        } else {
            self.undo.redo()
        }
        .map_err(crdt)?;
        if !done {
            return Ok(None);
        }
        let restored = lock(&self.shared).restored.take();
        Ok(Some(restored.map(|(a, f)| (Anchor(a), Anchor(f)))))
    }
}

/// The callbacks only copy cursors in and out, so a poisoned lock holds consistent data.
fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
