//! Layout engine for opendox: shaping, line breaking, pagination and hit testing.
//!
//! Takes plain paragraphs and style runs in, returns geometry out. Must not depend on the CRDT
//! or `opendox-model`. All geometry is in points (1/72 inch) as `f32`; conversion to device
//! pixels happens in the renderer.
