//! Image file codecs for the runtime: decoding PNG, GIF (animated) and BMP, and encoding PNG and
//! BMP. Written for this port because the `image` crate needs `std`. They produce the same RGBA8
//! output the original relied on (palette entries, `tRNS`, GIF transparency and frame disposal).

use super::{ImageError, Rgba, RgbaImage};

fn err(message: &'static str) -> ImageError {
    ImageError(message)
}

/// Decodes the first frame of a PNG, GIF or BMP file.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
    } else if bytes.starts_with(b"GIF8") {
        decode_gif(bytes)?.into_iter().next().ok_or_else(|| err("GIF has no frames"))
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else {
        Err(err("unsupported image format (PNG, GIF and BMP are supported)"))
    }
}

// ---------------------------------------------------------------- PNG

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlaced: bool,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    let mut at = 8;
    let mut header = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut alpha: Vec<u8> = Vec::new();
    let mut trns_key: Option<[u16; 3]> = None;
    let mut idat = Vec::new();
    while at + 8 <= bytes.len() {
        let length = be32(&bytes[at..]) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + length).ok_or_else(|| err("truncated PNG chunk"))?;
        match kind {
            b"IHDR" if length >= 13 => {
                header = Some(Header {
                    width: be32(body),
                    height: be32(&body[4..]),
                    depth: body[8],
                    color: body[9],
                    interlaced: body[12] == 1,
                });
            },
            b"PLTE" => palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => match header.as_ref().map(|h| h.color) {
                Some(3) => alpha = body.to_vec(),
                Some(0) if length >= 2 => {
                    let v = u16::from_be_bytes([body[0], body[1]]);
                    trns_key = Some([v, v, v]);
                },
                Some(2) if length >= 6 => {
                    trns_key = Some([
                        u16::from_be_bytes([body[0], body[1]]),
                        u16::from_be_bytes([body[2], body[3]]),
                        u16::from_be_bytes([body[4], body[5]]),
                    ])
                },
                _ => (),
            },
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => (),
        }
        at += 12 + length;
    }
    let h = header.ok_or_else(|| err("PNG without IHDR"))?;
    if h.width == 0 || h.height == 0 || h.width > 16384 || h.height > 16384 {
        return Err(err("PNG dimensions out of range"))
    }
    let channels = match h.color {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(err("invalid PNG color type")),
    };
    if !matches!(h.depth, 1 | 2 | 4 | 8 | 16) {
        return Err(err("invalid PNG bit depth"))
    }
    let bits_per_pixel = channels * usize::from(h.depth);
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).map_err(|_| err("PNG data is corrupt"))?;

    let mut out = RgbaImage::new(h.width, h.height);
    let passes: &[(u32, u32, u32, u32)] = if h.interlaced {
        &[(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    } else {
        &[(0, 0, 1, 1)]
    };
    let mut cursor = 0usize;
    for &(x0, y0, dx, dy) in passes {
        if x0 >= h.width || y0 >= h.height {
            continue
        }
        let pass_w = (h.width - x0 + dx - 1) / dx;
        let pass_h = (h.height - y0 + dy - 1) / dy;
        let row_bytes = (pass_w as usize * bits_per_pixel + 7) / 8;
        let bpp_bytes = (bits_per_pixel + 7) / 8;
        let mut previous = vec![0u8; row_bytes];
        for row in 0..pass_h {
            let filter = *raw.get(cursor).ok_or_else(|| err("PNG data ends early"))?;
            let line = raw.get(cursor + 1..cursor + 1 + row_bytes).ok_or_else(|| err("PNG data ends early"))?;
            cursor += 1 + row_bytes;
            let mut current = line.to_vec();
            for i in 0..row_bytes {
                let a = if i >= bpp_bytes { current[i - bpp_bytes] } else { 0 };
                let b = previous[i];
                let c = if i >= bpp_bytes { previous[i - bpp_bytes] } else { 0 };
                let add = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    4 => {
                        let (pa, pb, pc) = (
                            (i16::from(b) - i16::from(c)).abs(),
                            (i16::from(a) - i16::from(c)).abs(),
                            (i16::from(a) + i16::from(b) - 2 * i16::from(c)).abs(),
                        );
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    },
                    _ => return Err(err("invalid PNG filter")),
                };
                current[i] = current[i].wrapping_add(add);
            }
            for col in 0..pass_w {
                let sample = |index: usize| -> u16 {
                    let bit = (col as usize * channels + index) * usize::from(h.depth);
                    match h.depth {
                        16 => u16::from_be_bytes([current[bit / 8], current[bit / 8 + 1]]),
                        8 => u16::from(current[bit / 8]),
                        d => {
                            let shift = 8 - usize::from(d) - (bit % 8);
                            u16::from((current[bit / 8] >> shift) & ((1u8 << d) - 1))
                        },
                    }
                };
                let to8 = |v: u16| -> u8 {
                    match h.depth {
                        16 => (v >> 8) as u8,
                        8 => v as u8,
                        d => (u32::from(v) * 255 / ((1u32 << d) - 1)) as u8,
                    }
                };
                let pixel = match h.color {
                    0 => {
                        let v = sample(0);
                        let g = to8(v);
                        let a = if trns_key.map(|k| k[0] == v).unwrap_or(false) { 0 } else { 255 };
                        Rgba([g, g, g, a])
                    },
                    2 => {
                        let (r, g, b) = (sample(0), sample(1), sample(2));
                        let a = if trns_key.map(|k| k == [r, g, b]).unwrap_or(false) { 0 } else { 255 };
                        Rgba([to8(r), to8(g), to8(b), a])
                    },
                    3 => {
                        let index = sample(0) as usize;
                        let rgb = palette.get(index).copied().unwrap_or([0, 0, 0]);
                        Rgba([rgb[0], rgb[1], rgb[2], alpha.get(index).copied().unwrap_or(255)])
                    },
                    4 => {
                        let g = to8(sample(0));
                        Rgba([g, g, g, to8(sample(1))])
                    },
                    _ => Rgba([to8(sample(0)), to8(sample(1)), to8(sample(2)), to8(sample(3))]),
                };
                out.put_pixel(x0 + col * dx, y0 + row * dy, pixel);
            }
            previous = current;
        }
    }
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Encodes an RGBA8 PNG (no filtering, deflate by `miniz_oxide`).
pub fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let (width, height) = image.dimensions();
    let mut scan = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    let raw = image.as_raw();
    for row in 0..height as usize {
        scan.push(0);
        scan.extend_from_slice(&raw[row * width as usize * 4..(row + 1) * width as usize * 4]);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&scan, 6);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &compressed);
    chunk(b"IEND", &[]);
    out
}

// ---------------------------------------------------------------- BMP

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub fn decode_bmp(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    if bytes.len() < 54 {
        return Err(err("BMP too short"))
    }
    let data_offset = le32(&bytes[10..]) as usize;
    let header_size = le32(&bytes[14..]) as usize;
    if header_size < 40 {
        return Err(err("unsupported BMP header"))
    }
    let width = le32(&bytes[18..]) as i32;
    let raw_height = le32(&bytes[22..]) as i32;
    let bpp = le16(&bytes[28..]);
    let compression = le32(&bytes[30..]);
    let colors_used = le32(&bytes[46..]) as usize;
    if width <= 0 || raw_height == 0 || width > 16384 || raw_height.unsigned_abs() > 16384 {
        return Err(err("BMP dimensions out of range"))
    }
    if compression != 0 && compression != 3 {
        return Err(err("compressed BMP is not supported"))
    }
    let (width, height, top_down) = (width as u32, raw_height.unsigned_abs(), raw_height < 0);
    let palette_at = 14 + header_size;
    let palette_len = if bpp <= 8 { if colors_used == 0 { 1usize << bpp } else { colors_used } } else { 0 };
    let palette: Vec<[u8; 3]> = (0..palette_len)
        .filter_map(|i| bytes.get(palette_at + i * 4..palette_at + i * 4 + 4).map(|c| [c[2], c[1], c[0]]))
        .collect();
    let row_bytes = ((width as usize * usize::from(bpp) + 31) / 32) * 4;
    let mut out = RgbaImage::new(width, height);
    // 32-bit BMPs from editors often leave alpha at 0 for opaque images; treat all-zero alpha as opaque.
    let mut any_alpha = false;
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let row = bytes
            .get(data_offset + src_y as usize * row_bytes..data_offset + (src_y as usize + 1) * row_bytes)
            .ok_or_else(|| err("BMP pixel data ends early"))?;
        for x in 0..width {
            let xi = x as usize;
            let pixel = match bpp {
                1 | 4 | 8 => {
                    let bits = usize::from(bpp);
                    let bit = xi * bits;
                    let value = (row[bit / 8] >> (8 - bits - (bit % 8))) & ((1u16 << bits) - 1) as u8;
                    let rgb = palette.get(usize::from(value)).copied().unwrap_or([0, 0, 0]);
                    Rgba([rgb[0], rgb[1], rgb[2], 255])
                },
                16 => {
                    let v = le16(&row[xi * 2..]);
                    let expand = |c: u16| ((c << 3) | (c >> 2)) as u8;
                    Rgba([expand((v >> 10) & 31), expand((v >> 5) & 31), expand(v & 31), 255])
                },
                24 => Rgba([row[xi * 3 + 2], row[xi * 3 + 1], row[xi * 3], 255]),
                32 => {
                    let a = row[xi * 4 + 3];
                    any_alpha |= a != 0;
                    Rgba([row[xi * 4 + 2], row[xi * 4 + 1], row[xi * 4], a])
                },
                _ => return Err(err("unsupported BMP bit depth")),
            };
            out.put_pixel(x, y, pixel);
        }
    }
    if bpp == 32 && !any_alpha {
        for pixel in out.pixels_mut() {
            pixel.0[3] = 255;
        }
    }
    Ok(out)
}

/// Encodes a 32-bit BMP (BITMAPINFOHEADER, bottom-up, alpha preserved).
pub fn encode_bmp(image: &RgbaImage) -> Vec<u8> {
    let (width, height) = image.dimensions();
    let pixel_bytes = width as usize * height as usize * 4;
    let mut out = Vec::with_capacity(54 + pixel_bytes);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + pixel_bytes) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // BI_RGB
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..height).rev() {
        for x in 0..width {
            let p = image.get_pixel(x, y).0;
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

// ---------------------------------------------------------------- GIF

struct LzwReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl LzwReader<'_> {
    fn read(&mut self, bits: u32) -> Option<u32> {
        let mut value = 0u32;
        for i in 0..bits {
            let byte = *self.data.get(self.bit / 8)?;
            value |= u32::from((byte >> (self.bit % 8)) & 1) << i;
            self.bit += 1;
        }
        Some(value)
    }
}

fn lzw_decode(min_code_size: u8, data: &[u8], expected: usize) -> Vec<u8> {
    let clear = 1u32 << min_code_size;
    let end = clear + 1;
    let mut size = u32::from(min_code_size) + 1;
    let mut reader = LzwReader { data, bit: 0 };
    let mut prefix: Vec<u16> = vec![0; 4096];
    let mut suffix: Vec<u8> = vec![0; 4096];
    for i in 0..clear {
        suffix[i as usize] = i as u8;
    }
    let mut next = end + 1;
    let mut previous: Option<u32> = None;
    let mut out = Vec::with_capacity(expected);
    let mut stack = Vec::new();
    while let Some(code) = reader.read(size) {
        if code == clear {
            size = u32::from(min_code_size) + 1;
            next = end + 1;
            previous = None;
            continue
        }
        if code == end {
            break
        }
        let mut walk = code;
        stack.clear();
        match previous {
            None => {
                if code >= clear {
                    break
                }
                out.push(code as u8);
                previous = Some(code);
                continue
            },
            Some(prev) => {
                if code >= next {
                    // KwKwK case: the code being defined right now.
                    walk = prev;
                    while walk >= clear {
                        stack.push(suffix[walk as usize]);
                        walk = u32::from(prefix[walk as usize]);
                    }
                    let first = walk as u8;
                    stack.reverse();
                    let mut entry: Vec<u8> = vec![first];
                    entry.extend_from_slice(&stack);
                    entry.push(first);
                    out.extend_from_slice(&entry);
                    if next < 4096 {
                        prefix[next as usize] = prev as u16;
                        suffix[next as usize] = first;
                        next += 1;
                    }
                } else {
                    while walk >= clear {
                        stack.push(suffix[walk as usize]);
                        walk = u32::from(prefix[walk as usize]);
                    }
                    let first = walk as u8;
                    stack.push(first);
                    stack.reverse();
                    out.extend_from_slice(&stack);
                    if next < 4096 {
                        prefix[next as usize] = prev as u16;
                        suffix[next as usize] = first;
                        next += 1;
                    }
                }
            },
        }
        previous = Some(code);
        if next >= (1 << size) && size < 12 {
            size += 1;
        }
        if out.len() >= expected {
            break
        }
    }
    out
}

/// Decodes every frame of a GIF, composed onto the logical screen with the file's disposal
/// methods (transparent pixels keep the palette colour with alpha 0, like the `gif` crate).
pub fn decode_gif(bytes: &[u8]) -> Result<Vec<RgbaImage>, ImageError> {
    if bytes.len() < 13 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Err(err("not a GIF"))
    }
    let screen_w = u32::from(le16(&bytes[6..]));
    let screen_h = u32::from(le16(&bytes[8..]));
    if screen_w == 0 || screen_h == 0 || screen_w > 16384 || screen_h > 16384 {
        return Err(err("GIF dimensions out of range"))
    }
    let flags = bytes[10];
    let mut at = 13;
    let mut global: Vec<[u8; 3]> = Vec::new();
    if flags & 0x80 != 0 {
        let count = 2usize << (flags & 7);
        global = bytes
            .get(at..at + count * 3)
            .ok_or_else(|| err("GIF palette truncated"))?
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        at += count * 3;
    }
    let mut canvas = RgbaImage::new(screen_w, screen_h);
    let mut frames = Vec::new();
    let (mut disposal, mut transparent): (u8, Option<u8>) = (0, None);
    let mut restore: Option<RgbaImage> = None;
    while at < bytes.len() {
        match bytes[at] {
            0x21 => {
                let label = *bytes.get(at + 1).ok_or_else(|| err("GIF truncated"))?;
                at += 2;
                if label == 0xF9 && bytes.get(at) == Some(&4) {
                    let packed = bytes[at + 1];
                    disposal = (packed >> 2) & 7;
                    transparent = if packed & 1 != 0 { Some(bytes[at + 4]) } else { None };
                }
                while let Some(&size) = bytes.get(at) {
                    at += 1 + usize::from(size);
                    if size == 0 {
                        break
                    }
                }
            },
            0x2C => {
                let d = bytes.get(at + 1..at + 10).ok_or_else(|| err("GIF truncated"))?;
                let (fx, fy, fw, fh) =
                    (u32::from(le16(d)), u32::from(le16(&d[2..])), u32::from(le16(&d[4..])), u32::from(le16(&d[6..])));
                let packed = d[8];
                at += 10;
                let mut local = global.clone();
                if packed & 0x80 != 0 {
                    let count = 2usize << (packed & 7);
                    local = bytes
                        .get(at..at + count * 3)
                        .ok_or_else(|| err("GIF palette truncated"))?
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    at += count * 3;
                }
                let min_code = *bytes.get(at).ok_or_else(|| err("GIF truncated"))?;
                at += 1;
                let mut compressed = Vec::new();
                while let Some(&size) = bytes.get(at) {
                    at += 1;
                    if size == 0 {
                        break
                    }
                    compressed.extend_from_slice(bytes.get(at..at + usize::from(size)).ok_or_else(|| err("GIF truncated"))?);
                    at += usize::from(size);
                }
                if min_code == 0 || min_code > 11 {
                    return Err(err("invalid GIF code size"))
                }
                let indices = lzw_decode(min_code, &compressed, (fw * fh) as usize);
                if disposal == 3 {
                    restore = Some(canvas.clone());
                }
                let interlaced = packed & 0x40 != 0;
                let rows: Vec<u32> = if interlaced {
                    let mut order = Vec::new();
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        order.extend((start..fh).step_by(step));
                    }
                    order
                } else {
                    (0..fh).collect()
                };
                for (n, &row) in rows.iter().enumerate() {
                    for col in 0..fw {
                        let Some(&index) = indices.get(n * fw as usize + col as usize) else { continue };
                        let (x, y) = (fx + col, fy + row);
                        if x >= screen_w || y >= screen_h {
                            continue
                        }
                        let rgb = local.get(usize::from(index)).copied().unwrap_or([0, 0, 0]);
                        if Some(index) == transparent {
                            // Leave the canvas as it was (the frame is transparent here).
                            continue
                        }
                        canvas.put_pixel(x, y, Rgba([rgb[0], rgb[1], rgb[2], 255]));
                    }
                }
                frames.push(canvas.clone());
                match disposal {
                    2 => {
                        for row in fy..(fy + fh).min(screen_h) {
                            for col in fx..(fx + fw).min(screen_w) {
                                canvas.put_pixel(col, row, Rgba([0, 0, 0, 0]));
                            }
                        }
                    },
                    3 => {
                        if let Some(previous) = restore.take() {
                            canvas = previous;
                        }
                    },
                    _ => (),
                }
                disposal = 0;
                transparent = None;
            },
            0x3B => break,
            _ => return Err(err("invalid GIF block")),
        }
    }
    if frames.is_empty() {
        Err(err("GIF has no frames"))
    } else {
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RgbaImage {
        let mut raw = Vec::new();
        for y in 0..5u8 {
            for x in 0..7u8 {
                raw.extend_from_slice(&[x * 30, y * 40, 200 - x * 10, if (x + y) % 3 == 0 { 0 } else { 255 }]);
            }
        }
        RgbaImage::from_vec(7, 5, raw).unwrap()
    }

    #[test]
    fn png_round_trips() {
        let image = sample();
        let bytes = encode_png(&image);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(decode(&bytes).unwrap(), image);
    }

    #[test]
    fn bmp_round_trips_with_alpha() {
        let image = sample();
        let bytes = encode_bmp(&image);
        assert_eq!(decode(&bytes).unwrap(), image);
    }

    #[test]
    fn png_palette_with_transparency_decodes() {
        // 2x2, 8-bit palette: colours red, green; index 1 is transparent via tRNS.
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut add = |kind: &[u8; 4], body: &[u8]| {
            png.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let start = png.len();
            png.extend_from_slice(kind);
            png.extend_from_slice(body);
            let crc = crc32(&png[start..]);
            png.extend_from_slice(&crc.to_be_bytes());
        };
        add(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 3, 0, 0, 0]);
        add(b"PLTE", &[255, 0, 0, 0, 255, 0]);
        add(b"tRNS", &[255, 0]);
        let scan = [0u8, 0, 1, 0, 1, 0];
        add(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&scan, 6));
        add(b"IEND", &[]);
        let image = decode(&png).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0, 0]);
    }

    #[test]
    fn gif_two_frames_decode_with_transparency() {
        // Hand-built GIF89a, 2x1: palette [black, red, white]; frame 1 = red,black(transparent idx 0);
        // frame 2 = white at pixel 1 only (transparent elsewhere is not needed: full frame).
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[2, 0, 1, 0, 0x81, 0, 0]); // 2x1, global table of 4 entries
        gif.extend_from_slice(&[0, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 0]);
        // Graphic control: transparent index 0, disposal 1
        gif.extend_from_slice(&[0x21, 0xF9, 4, 0b0000_0101, 0, 0, 0, 0]);
        // Image 1: LZW min code 2, indices [1, 0]
        let frame = |gif: &mut Vec<u8>, indices: [u8; 2]| {
            gif.extend_from_slice(&[0x2C, 0, 0, 0, 0, 2, 0, 1, 0, 0]);
            gif.push(2);
            // code stream (size 3 bits): clear(4), i0, i1, end(5)
            let codes = [4u32, u32::from(indices[0]), u32::from(indices[1]), 5];
            let (mut acc, mut bits) = (0u32, 0u32);
            let mut packed = Vec::new();
            for code in codes {
                acc |= code << bits;
                bits += 3;
                while bits >= 8 {
                    packed.push((acc & 0xFF) as u8);
                    acc >>= 8;
                    bits -= 8;
                }
            }
            if bits > 0 {
                packed.push(acc as u8);
            }
            gif.push(packed.len() as u8);
            gif.extend_from_slice(&packed);
            gif.push(0);
        };
        frame(&mut gif, [1, 0]);
        gif.extend_from_slice(&[0x21, 0xF9, 4, 0b0000_0100, 0, 0, 0, 0]); // no transparency now
        frame(&mut gif, [0, 2]);
        gif.push(0x3B);
        let frames = decode_gif(&gif).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(frames[0].get_pixel(1, 0).0[3], 0, "transparent index leaves the canvas empty");
        assert_eq!(frames[1].get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(frames[1].get_pixel(1, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn rejects_unknown_formats() {
        assert!(decode(b"not an image").is_err());
        assert!(decode(&[]).is_err());
    }
}
