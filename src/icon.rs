//! Procedurally drawn app icon (a card with a gold border and an orange gem).
//! Shared by the window icon at runtime and by build.rs for the .exe icon.

/// Renders the icon as straight (non-premultiplied) RGBA8, `size` x `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4; // supersampling per axis
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) / s;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) / s;
                    if let Some([r, g, b, a]) = shade(px, py) {
                        acc[0] += r * a;
                        acc[1] += g * a;
                        acc[2] += b * a;
                        acc[3] += a;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let a = acc[3] / n;
            let i = ((y * size + x) * 4) as usize;
            if a > 0.0 {
                out[i] = (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 1] = (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 2] = (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    out
}

fn sd_round_rect(x: f32, y: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = x.abs() - hw + r;
    let qy = y.abs() - hh + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn shade(x: f32, y: f32) -> Option<[f32; 4]> {
    let d = sd_round_rect(x - 0.5, y - 0.5, 0.36, 0.47, 0.09);
    if d > 0.0 {
        return None;
    }
    // Gold border
    if d > -0.045 {
        let c = lerp([236.0, 196.0, 98.0], [184.0, 132.0, 42.0], y);
        return Some([c[0], c[1], c[2], 1.0]);
    }
    // Gem (diamond)
    let gx = (x - 0.5).abs() / 0.2;
    let gy = (y - 0.48).abs() / 0.28;
    if gx + gy <= 1.0 {
        let base = lerp([255.0, 196.0, 92.0], [226.0, 96.0, 30.0], (y - 0.2) / 0.56);
        let facet = if x < 0.5 { 0.86 } else { 1.0 };
        return Some([base[0] * facet, base[1] * facet, base[2] * facet, 1.0]);
    }
    // Card body
    let c = lerp([46.0, 52.0, 74.0], [20.0, 23.0, 36.0], y);
    Some([c[0], c[1], c[2], 1.0])
}

/// Encodes several icon sizes into a Windows .ico (32-bit BMP entries).
#[allow(dead_code)]
pub fn ico_bytes(sizes: &[u32]) -> Vec<u8> {
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &size in sizes {
        let px = rgba(size);
        let mut bmp = Vec::new();
        // BITMAPINFOHEADER
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(size as i32).to_le_bytes());
        bmp.extend_from_slice(&((size * 2) as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        let mask_row = size.div_ceil(32) * 4;
        let img_size = size * size * 4 + mask_row * size;
        bmp.extend_from_slice(&img_size.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 16]);
        // Pixels, bottom-up BGRA
        for y in (0..size).rev() {
            for x in 0..size {
                let i = ((y * size + x) * 4) as usize;
                bmp.extend_from_slice(&[px[i + 2], px[i + 1], px[i], px[i + 3]]);
            }
        }
        // AND mask (all zero, alpha channel does the work)
        bmp.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
        images.push(bmp);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, img) in sizes.iter().zip(&images) {
        let dim = if size >= 256 { 0u8 } else { size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}
