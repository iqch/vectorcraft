//! Shared CPU worker policy for rasterization and NURBS masks.
use std::sync::atomic::{AtomicI32, Ordering};
static THREADS_OVERRIDE: AtomicI32 = AtomicI32::new(-1);
pub fn set_default_threads(n: Option<u16>) {
    THREADS_OVERRIDE.store(n.map_or(-1, i32::from), Ordering::Relaxed);
}
pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let n = THREADS_OVERRIDE.load(Ordering::Relaxed);
        if n >= 0 {
            return n.min(64) as u16;
        }
        if std::env::var_os("VECTORCRAFT_RENDER_THREADS").is_some() {
            return std::env::var("VECTORCRAFT_RENDER_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).min(8) as u16).unwrap_or(0)
    }
}
/// Keep small jobs serial, and assign at least 32K covered pixels to a worker.
pub fn workers(work: usize, requested: u16) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (work, requested);
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        (work / 32768).max(1).min(usize::from(requested).clamp(1, 64))
    }
}

/// Process disjoint complete row bands on the shared Rayon pool (serial on WASM).
pub fn for_each_band<T: Send>(data: &mut [T], stride: usize, workers: usize, draw: impl Fn(usize, &mut [T]) + Sync + Send) {
    if data.is_empty() {
        return;
    }
    assert!(stride > 0 && data.len().is_multiple_of(stride));
    let rows = data.len() / stride;
    let workers = workers.min(rows).max(1);
    if workers == 1 {
        draw(0, data);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let band_rows = rows.div_ceil(workers);
        data.par_chunks_mut(band_rows * stride).enumerate().for_each(|(i, band)| draw(i * band_rows, band));
    }
    #[cfg(target_arch = "wasm32")]
    {
        draw(0, data);
    }
}
/// The colors and coverage of a raster have matching disjoint bands.
pub fn for_each_band_pair<T: Send, U: Send>(
    data: &mut [T],
    coverage: &mut [U],
    stride: usize,
    workers: usize,
    draw: impl Fn(usize, &mut [T], &mut [U]) + Sync + Send,
) {
    assert_eq!(data.len(), coverage.len());
    if data.is_empty() {
        return;
    }
    assert!(stride > 0 && data.len().is_multiple_of(stride));
    let rows = data.len() / stride;
    let workers = workers.min(rows).max(1);
    if workers == 1 {
        draw(0, data, coverage);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let band_rows = rows.div_ceil(workers);
        data.par_chunks_mut(band_rows * stride)
            .zip(coverage.par_chunks_mut(band_rows * stride))
            .enumerate()
            .for_each(|(i, (band, covered))| draw(i * band_rows, band, covered));
    }
    #[cfg(target_arch = "wasm32")]
    {
        draw(0, data, coverage);
    }
}
