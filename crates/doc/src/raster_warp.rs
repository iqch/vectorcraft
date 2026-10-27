//! A raster mesh is sampled once per output pixel: adjacent triangles never composite alpha.
use crate::{ImageBlob, ImageObject, Node, NodeKind};
use std::{io::Cursor, sync::Arc};
use vectorcraft_geom::{Affine, Point, Rect};
const MAX_SIDE: u32 = 2048;
fn affine(s: [Point; 3], d: [Point; 3]) -> Option<Affine> {
    let basis = |p: [Point; 3]| Affine::new([p[1].x - p[0].x, p[1].y - p[0].y, p[2].x - p[0].x, p[2].y - p[0].y, p[0].x, p[0].y]);
    let b = basis(s);
    if b.determinant().abs() < 1e-16 {
        return None;
    }
    let a = basis(d) * b.inverse();
    a.is_finite().then_some(a)
}
pub(crate) fn image(n: &Node, im: &ImageObject, blob: Option<&ImageBlob>, map: &dyn Fn(Point) -> Point, cells: usize) -> Option<Node> {
    let (pw, ph) = (im.width as f64, im.height as f64);
    if pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    let at = |x: f64, y: f64| Point::new(pw * x, ph * y);
    let corner = [at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0)];
    let xf = affine(corner, corner.map(|p| map(im.xf * p)))?;
    if (0..=4).all(|y| {
        (0..=4).all(|x| {
            let p = at(x as f64 / 4.0, y as f64 / 4.0);
            (xf * p).distance(map(im.xf * p)) < 1e-7
        })
    }) {
        let mut out = n.clone();
        out.kind = NodeKind::Image(Box::new(ImageObject { xf, ..im.clone() }));
        return Some(out);
    }
    let blob = blob?;
    // Preserve CMYK ink data through the existing vector clip expansion.
    if blob.is_cmyk() {
        return None;
    }
    let pixels = blob.decoded()?;
    let k = cells.clamp(1, 64);
    let mut grid = Vec::with_capacity((k + 1) * (k + 1));
    let mut bounds = Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for y in 0..=k {
        for x in 0..=k {
            let p = map(im.xf * at(x as f64 / k as f64, y as f64 / k as f64));
            if !p.x.is_finite() || !p.y.is_finite() {
                return None;
            }
            bounds.x0 = bounds.x0.min(p.x);
            bounds.y0 = bounds.y0.min(p.y);
            bounds.x1 = bounds.x1.max(p.x);
            bounds.y1 = bounds.y1.max(p.y);
            grid.push(p);
        }
    }
    if bounds.width() <= 1e-9 || bounds.height() <= 1e-9 {
        return None;
    }
    let density = 2.0_f64.max(pixels.width() as f64 / bounds.width()).max(pixels.height() as f64 / bounds.height());
    let density = density.min(MAX_SIDE as f64 / bounds.width().max(bounds.height()));
    let width = (bounds.width() * density).ceil().clamp(1.0, MAX_SIDE as f64) as u32;
    let height = (bounds.height() * density).ceil().clamp(1.0, MAX_SIDE as f64) as u32;
    let to_pixel =
        Affine::scale_non_uniform(width as f64 / bounds.width(), height as f64 / bounds.height()) * Affine::translate((-bounds.x0, -bounds.y0));
    let mut output = image::RgbaImage::new(width, height);
    for y in 0..k {
        for x in 0..k {
            let ids = [y * (k + 1) + x, y * (k + 1) + x + 1, (y + 1) * (k + 1) + x + 1, (y + 1) * (k + 1) + x];
            let uv = [
                Point::new(x as f64 / k as f64, y as f64 / k as f64),
                Point::new((x + 1) as f64 / k as f64, y as f64 / k as f64),
                Point::new((x + 1) as f64 / k as f64, (y + 1) as f64 / k as f64),
                Point::new(x as f64 / k as f64, (y + 1) as f64 / k as f64),
            ];
            for tri in [[0, 1, 2], [0, 2, 3]] {
                let [a, b, c] = tri.map(|i| to_pixel * grid[ids[i]]);
                let ab = b - a;
                let ac = c - a;
                let det = ab.cross(ac);
                if det.abs() < 1e-12 {
                    continue;
                }
                let x0 = a.x.min(b.x).min(c.x).floor().clamp(0.0, width as f64) as u32;
                let x1 = a.x.max(b.x).max(c.x).ceil().clamp(0.0, width as f64) as u32;
                let y0 = a.y.min(b.y).min(c.y).floor().clamp(0.0, height as f64) as u32;
                let y1 = a.y.max(b.y).max(c.y).ceil().clamp(0.0, height as f64) as u32;
                for py in y0..y1 {
                    for px in x0..x1 {
                        let q = Point::new(px as f64 + 0.5, py as f64 + 0.5) - a;
                        let v = q.cross(ac) / det;
                        let w = ab.cross(q) / det;
                        let u = 1.0 - v - w;
                        if u >= -1e-9 && v >= -1e-9 && w >= -1e-9 {
                            let p = Point::new(
                                uv[tri[0]].x * u + uv[tri[1]].x * v + uv[tri[2]].x * w,
                                uv[tri[0]].y * u + uv[tri[1]].y * v + uv[tri[2]].y * w,
                            );
                            output.put_pixel(px, py, image::Rgba(sample(&pixels, p)));
                        }
                    }
                }
            }
        }
    }
    let mut png = Cursor::new(Vec::new());
    output.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let pixels = ImageBlob::png(png.into_inner());
    let key = format!("raster-warp-{}", pixels.content_key());
    let xf = Affine::translate((bounds.x0, bounds.y0)) * Affine::scale_non_uniform(bounds.width() / width as f64, bounds.height() / height as f64);
    let mut out = n.clone();
    out.kind = NodeKind::Image(Box::new(ImageObject { key, width, height, xf, link: None, pixels: Some(Arc::new(pixels)), ..im.clone() }));
    Some(out)
}
fn sample(im: &image::RgbaImage, p: Point) -> [u8; 4] {
    let x = (p.x * im.width() as f64 - 0.5).clamp(0.0, im.width().saturating_sub(1) as f64);
    let y = (p.y * im.height() as f64 - 0.5).clamp(0.0, im.height().saturating_sub(1) as f64);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(im.width() - 1), (y0 + 1).min(im.height() - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let mut sum = [0.0; 4];
    for (x, y, w) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)), (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] {
        let p = im.get_pixel(x, y).0;
        let a = p[3] as f64;
        sum[3] += a * w;
        for i in 0..3 {
            sum[i] += p[i] as f64 * a * w;
        }
    }
    if sum[3] <= 1e-12 {
        return [0; 4];
    }
    [(sum[0] / sum[3]).round() as u8, (sum[1] / sum[3]).round() as u8, (sum[2] / sum[3]).round() as u8, sum[3].round() as u8]
}
