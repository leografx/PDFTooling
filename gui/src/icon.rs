//! The app icon, drawn in code (blue squircle with three white sliders), plus
//! a tiny PNG writer so `--write-icon` can produce the .app icon on macOS.

/// RGBA pixels, `size` × `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let inset = s * 0.085; // macOS icon grid: artwork inside ~83%
    let r = s * 0.185;
    let (x0, y0, x1, y1) = (inset, inset, s - inset, s - inset);
    let sliders = [(0.36, 0.32), (0.50, 0.66), (0.64, 0.44)]; // (y, knob x)
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded-rect coverage with 1px antialiasing
            let cx = fx.clamp(x0 + r, x1 - r);
            let cy = fy.clamp(y0 + r, y1 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - r;
            let inside = (0.5 - d).clamp(0.0, 1.0);
            if inside <= 0.0 {
                continue;
            }
            // vertical gradient #3FA2FF → #0062E0
            let t = ((fy - y0) / (y1 - y0)).clamp(0.0, 1.0);
            let mut c = [
                63.0 + (0.0 - 63.0) * t,
                162.0 + (98.0 - 162.0) * t,
                255.0 + (224.0 - 255.0) * t,
            ];
            let mut white = 0.0f32;
            for (ly, kx) in sliders {
                let ly = y0 + (y1 - y0) * ly;
                let (lx0, lx1) = (x0 + (x1 - x0) * 0.2, x0 + (x1 - x0) * 0.8);
                let kx = x0 + (x1 - x0) * kx;
                // track
                let th = s * 0.022;
                let tx = fx.clamp(lx0, lx1);
                let td = ((fx - tx).powi(2) + (fy - ly).powi(2)).sqrt() - th;
                white = white.max((0.5 - td).clamp(0.0, 1.0) * 0.75);
                // knob
                let kd = ((fx - kx).powi(2) + (fy - ly).powi(2)).sqrt() - s * 0.07;
                white = white.max((0.5 - kd).clamp(0.0, 1.0));
            }
            for ch in &mut c {
                *ch = *ch + (255.0 - *ch) * white;
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = c[0] as u8;
            px[i + 1] = c[1] as u8;
            px[i + 2] = c[2] as u8;
            px[i + 3] = (inside * 255.0) as u8;
        }
    }
    px
}

pub fn icon_data(size: u32) -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: rgba(size), width: size, height: size }
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Uncompressed (stored-deflate) RGBA PNG.
pub fn png(size: u32) -> Vec<u8> {
    let px = rgba(size);
    let mut raw = Vec::with_capacity(px.len() + size as usize);
    for row in px.chunks((size * 4) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
