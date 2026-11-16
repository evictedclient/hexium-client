//! The client's icon, drawn in code so there is no binary asset to keep in the
//! repo: a coral hexagon with an "H" cut out of it, on the app's dark background.
//! Flat colours only.

const BACKGROUND: [u8; 3] = [22, 24, 28];
const CORAL: [u8; 3] = [255, 122, 89];
/// Sizes written to the `.ico`.
const ICO_SIZES: [u32; 4] = [16, 32, 48, 256];

/// What a point of the unit square shows.
fn sample(x: f32, y: f32) -> Option<[u8; 3]> {
    // Rounded square.
    let radius = 0.22;
    let (cx, cy) = (x.clamp(radius, 1.0 - radius), y.clamp(radius, 1.0 - radius));
    if (x - cx).powi(2) + (y - cy).powi(2) > radius * radius {
        return None;
    }
    // Pointy-top hexagon.
    let corners: Vec<(f32, f32)> = (0..6)
        .map(|index| {
            let angle = (90.0 + 60.0 * index as f32).to_radians();
            (0.5 + 0.36 * angle.cos(), 0.5 - 0.36 * angle.sin())
        })
        .collect();
    let inside_hexagon = (0..6).all(|index| {
        let (ax, ay) = corners[index];
        let (bx, by) = corners[(index + 1) % 6];
        (bx - ax) * (y - ay) - (by - ay) * (x - ax) <= 0.0
    });
    if !inside_hexagon {
        return Some(BACKGROUND);
    }
    // The "H".
    let in_box = |x0: f32, x1: f32, y0: f32, y1: f32| x >= x0 && x <= x1 && y >= y0 && y <= y1;
    let letter = in_box(0.37, 0.45, 0.34, 0.66)
        || in_box(0.55, 0.63, 0.34, 0.66)
        || in_box(0.45, 0.55, 0.465, 0.535);
    Some(if letter { BACKGROUND } else { CORAL })
}

/// Straight (not premultiplied) RGBA, row by row from the top, 4×4 supersampled.
pub fn rgba(size: u32) -> Vec<u8> {
    const STEPS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for row in 0..size {
        for column in 0..size {
            let (mut r, mut g, mut b, mut covered) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..STEPS {
                for sx in 0..STEPS {
                    let x = (column as f32 + (sx as f32 + 0.5) / STEPS as f32) / size as f32;
                    let y = (row as f32 + (sy as f32 + 0.5) / STEPS as f32) / size as f32;
                    if let Some([cr, cg, cb]) = sample(x, y) {
                        r += cr as u32;
                        g += cg as u32;
                        b += cb as u32;
                        covered += 1;
                    }
                }
            }
            if covered == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[
                    (r / covered) as u8,
                    (g / covered) as u8,
                    (b / covered) as u8,
                    (covered * 255 / (STEPS * STEPS)) as u8,
                ]);
            }
        }
    }
    out
}

/// A Windows `.ico` holding 32-bit images at several sizes.
pub fn ico() -> Vec<u8> {
    let images: Vec<Vec<u8>> = ICO_SIZES.iter().map(|size| dib(*size)).collect();
    let mut out = Vec::new();
    // ICONDIR
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(ICO_SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * ICO_SIZES.len() as u32;
    for (size, image) in ICO_SIZES.iter().zip(&images) {
        // ICONDIRENTRY: 0 means 256.
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(0); // palette
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(&image);
    }
    out
}

/// One icon image: BITMAPINFOHEADER, BGRA rows bottom-up, then the AND mask.
fn dib(size: u32) -> Vec<u8> {
    let pixels = rgba(size);
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // colour + mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // no compression, sizes and palette unused
    for row in (0..size).rev() {
        for column in 0..size {
            let at = ((row * size + column) * 4) as usize;
            out.extend_from_slice(&[pixels[at + 2], pixels[at + 1], pixels[at], pixels[at + 3]]);
        }
    }
    // AND mask: all zero (alpha does the work), rows padded to 32 bits.
    let mask_row = ((size + 31) / 32 * 4) as usize;
    out.extend(std::iter::repeat(0u8).take(mask_row * size as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_clear_and_the_hexagon_is_coral() {
        let size = 64;
        let pixels = rgba(size);
        let at = |x: u32, y: u32| &pixels[((y * size + x) * 4) as usize..((y * size + x) * 4 + 4) as usize];
        assert_eq!(at(0, 0)[3], 0);
        // Inside the hexagon, left of the letter.
        assert_eq!(&at(19, 32)[..3], &CORAL);
        // On the crossbar of the H.
        assert_eq!(&at(32, 32)[..3], &BACKGROUND);
    }

    #[test]
    fn ico_header_lists_every_size() {
        let bytes = ico();
        assert_eq!(&bytes[..6], &[0, 0, 1, 0, ICO_SIZES.len() as u8, 0]);
        assert_eq!(bytes[6], 16);
        assert_eq!(bytes[6 + 16 * 3], 0); // 256 is written as 0
    }
}
