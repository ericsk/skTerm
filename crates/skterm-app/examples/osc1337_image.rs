use base64::Engine;

fn build_gradient_bmp(width: u32, height: u32) -> Vec<u8> {
    let row_stride = (width * 3).div_ceil(4) * 4;
    let pixel_bytes = row_stride * height;
    let file_size = 54 + pixel_bytes;

    let mut bmp = Vec::with_capacity(file_size as usize);
    // BITMAPFILEHEADER (14 bytes)
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER (40 bytes)
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixel_bytes.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&2835u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());

    let pad = (row_stride - width * 3) as usize;
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width.max(1)) as u8;
            let g = (((height - 1 - y) * 255) / height.max(1)) as u8;
            let b = 210u8;
            bmp.extend_from_slice(&[b, g, r]); // BGR
        }
        bmp.extend(std::iter::repeat_n(0u8, pad));
    }
    bmp
}

fn main() -> anyhow::Result<()> {
    let (name, bytes) = if let Some(path) = std::env::args().nth(1) {
        let data = std::fs::read(&path)?;
        let fname = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        (fname, data)
    } else {
        ("gradient.bmp".to_string(), build_gradient_bmp(120, 64))
    };

    let name_b64 = base64::engine::general_purpose::STANDARD.encode(name.as_bytes());
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    println!(
        "\x1b]1337;File=name={};size={};inline=1:{}\x07",
        name_b64,
        bytes.len(),
        data_b64
    );
    println!("OSC 1337 Inline Image Displayed Successfully!");
    Ok(())
}
