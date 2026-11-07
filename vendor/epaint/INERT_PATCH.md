# Inert patch to epaint 0.37.0

Only change: `src/text/font_face.rs` creates `vello_cpu::RenderContext` with
`num_threads: 0` (single-threaded) instead of the default.

Why: Servo depends on `vello_cpu` with its `multithreading` feature. Cargo
unifies features, so epaint's `RenderContext::new` silently switched to the
multi-threaded dispatcher, which panics when rendering without `flush()`
("attempted to rasterize before flushing"). Remove this patch once epaint
sets the thread count explicitly upstream.
