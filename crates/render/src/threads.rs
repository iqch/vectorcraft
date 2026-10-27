use super::*;

/// A render context on the calling thread. vello_cpu's `RenderContext::new` defaults to a
/// multithreaded dispatcher, which panics on filter effects (glows, shadows, blur).
pub(crate) fn single_threaded_context(w: u16, h: u16) -> RenderContext {
    RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() })
}

/// Override the worker-thread count used by renderers created from now on (`None` = automatic).
pub fn set_default_threads(n: Option<u16>) {
    vectorcraft_doc::raster_parallel::set_default_threads(n);
}

/// Rasterizer worker threads: 0 on wasm; otherwise one per core up to 8, leaving a core free. On a
/// 10-core machine 8 workers render a 50,000-path page 1.4× faster than 4 (9 are slower again);
/// the pixels don't depend on the count (`pixels_do_not_depend_on_the_thread_count`).
pub fn default_threads() -> u16 {
    vectorcraft_doc::raster_parallel::default_threads()
}
