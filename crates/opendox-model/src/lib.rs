//! Document model for opendox, built on the Loro CRDT.
//!
//! Loro is an implementation detail: nothing outside this crate should name Loro types, so the
//! CRDT can be swapped without touching layout or the editor.

#[cfg(test)]
mod tests {
    use loro::LoroDoc;

    /// M0 smoke test: Loro builds on this toolchain and two replicas converge.
    #[test]
    fn loro_replicas_converge() {
        let a = LoroDoc::new();
        let b = LoroDoc::new();
        a.get_text("body").insert(0, "hello").unwrap();
        b.get_text("body").insert(0, "world").unwrap();

        b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        a.import(&b.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();

        let text_a = a.get_text("body").to_string();
        assert_eq!(text_a, b.get_text("body").to_string());
        assert_eq!(text_a.len(), 10);
    }
}
