//! AudioCraft's icon, drawn in code: a rounded dark tile with a smooth teal waveform. The same
//! drawing gives the window/taskbar icon (main.rs) and, through build.rs, the icon inside the
//! Windows .exe and the PNG / ICO / ICNS / .desktop files for Linux and macOS packages, so every
//! build shows the same logo. Only the standard library is used (build.rs includes this file).

/// The icon at `size` x `size` pixels, RGBA. Every pixel averages 4x4 samples, so the curves stay
/// smooth at any size.
pub fn rgba(size: u32) -> Vec<u8> {
    let n = size as usize;
    let mut rgba = vec![0u8; n * n * 4];
    let ss = 4usize;
    let wave_y = |x: f32| -> f32 {
        // a burst that swells in the middle, like a sound
        let u = x * 2.0 - 1.0;
        let env = (1.0 - u * u).max(0.0).powf(1.4);
        0.5 + 0.30 * env * (x * 26.0).sin() * (0.65 + 0.35 * (x * 7.0).cos())
    };
    for py in 0..n {
        for px in 0..n {
            let (mut a_bg, mut a_wave, mut a_glow) = (0.0f32, 0.0f32, 0.0f32);
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = (px as f32 + (sx as f32 + 0.5) / ss as f32) / n as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / ss as f32) / n as f32;
                    // rounded square tile
                    let r = 0.2f32;
                    let dx = (x - 0.5).abs() - (0.5 - r) + 0.02;
                    let dy = (y - 0.5).abs() - (0.5 - r) + 0.02;
                    let d =
                        (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0);
                    if d <= r {
                        a_bg += 1.0;
                        // distance to the wave line (vertical distance scaled by the slope)
                        if (0.12..=0.88).contains(&x) {
                            let xx = (x - 0.12) / 0.76;
                            let w = wave_y(xx);
                            let h = 1e-3;
                            let slope = (wave_y(xx + h) - wave_y(xx - h)) / (2.0 * h) / 0.76;
                            let dist = (y - w).abs() / (1.0 + slope * slope).sqrt();
                            if dist < 0.022 {
                                a_wave += 1.0;
                            } else if dist < 0.05 {
                                a_glow += 1.0 - (dist - 0.022) / 0.028;
                            }
                        }
                    }
                }
            }
            let k = (ss * ss) as f32;
            let (bg, wave, glow) = (a_bg / k, a_wave / k, a_glow / k);
            let y = py as f32 / n as f32;
            // tile: dark slate, a little lighter at the top
            let base = [
                34.0 + 14.0 * (1.0 - y),
                38.0 + 14.0 * (1.0 - y),
                46.0 + 16.0 * (1.0 - y),
            ];
            let wc = [46.0, 214.0, 170.0];
            let mut c = [0.0f32; 3];
            for i in 0..3 {
                let g = base[i] + (wc[i] - base[i]) * (glow * 0.35);
                c[i] = if bg > 0.0 {
                    g + (wc[i] - g) * (wave / bg).min(1.0)
                } else {
                    0.0
                };
            }
            let o = (py * n + px) * 4;
            rgba[o] = c[0].clamp(0.0, 255.0) as u8;
            rgba[o + 1] = c[1].clamp(0.0, 255.0) as u8;
            rgba[o + 2] = c[2].clamp(0.0, 255.0) as u8;
            rgba[o + 3] = (bg * 255.0).round() as u8;
        }
    }
    rgba
}

/// Sizes the file icons carry (Windows picks the best one for each view).
#[allow(dead_code)]
pub const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// The icon as a PNG file (uncompressed deflate blocks: simple and always valid).
#[allow(dead_code)]
pub fn png(size: u32) -> Vec<u8> {
    let px = rgba(size);
    let n = size as usize;
    let mut raw = Vec::with_capacity(n * (n * 4 + 1));
    for row in px.chunks(n * 4) {
        raw.push(0); // no filter
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// A Windows .ico file with every size in `SIZES` (PNG images inside).
#[allow(dead_code)]
pub fn ico() -> Vec<u8> {
    let imgs: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, png(s))).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(imgs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * imgs.len() as u32;
    for (s, data) in &imgs {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &imgs {
        out.extend_from_slice(data);
    }
    out
}

/// A macOS .icns file (PNG images at 32, 64, 128, 256 and 512 pixels).
#[allow(dead_code)]
pub fn icns() -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, s) in [
        (b"ic11", 32u32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
    ] {
        let p = png(s);
        body.extend_from_slice(kind);
        body.extend_from_slice(&((p.len() + 8) as u32).to_be_bytes());
        body.extend_from_slice(&p);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// A compiled Windows resource (.res) with the icon as the program's first icon group, which
/// Explorer, the taskbar and shortcuts show for the .exe. The MSVC linker takes it as it is.
#[allow(dead_code)]
pub fn windows_res() -> Vec<u8> {
    fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes()); // header size
        out.extend_from_slice(&[0xff, 0xff]);
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&[0xff, 0xff]);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // data version
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0x0409u16.to_le_bytes()); // language
        out.extend_from_slice(&0u32.to_le_bytes()); // version
        out.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    // the empty resource every .res file starts with
    let mut out = vec![0u8; 32];
    out[4] = 32;
    out[8] = 0xff;
    out[9] = 0xff;
    out[12] = 0xff;
    out[13] = 0xff;
    let imgs: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, png(s))).collect();
    for (i, (_, data)) in imgs.iter().enumerate() {
        entry(&mut out, 3, i as u16 + 1, 0x1010, data); // RT_ICON
    }
    let mut group = vec![0, 0, 1, 0];
    group.extend_from_slice(&(imgs.len() as u16).to_le_bytes());
    for (i, (s, data)) in imgs.iter().enumerate() {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        group.extend_from_slice(&[d, d, 0, 0]);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(data.len() as u32).to_le_bytes());
        group.extend_from_slice(&(i as u16 + 1).to_le_bytes());
    }
    entry(&mut out, 14, 1, 0x1030, &group); // RT_GROUP_ICON
    out
}

/// A Linux desktop entry for the program (installed beside the icon).
#[allow(dead_code)]
pub const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=AudioCraft\nComment=Audio editor and multitrack mixer\nExec=audiocraft %F\nIcon=audiocraft\nTerminal=false\nCategories=AudioVideo;Audio;AudioVideoEditing;\nMimeType=audio/wav;audio/x-wav;audio/flac;audio/mpeg;audio/ogg;\n";
