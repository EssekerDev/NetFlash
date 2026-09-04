//! Write README gallery PNGs (run from workspace root).

use std::fs;
use std::io::BufWriter;
use std::path::Path;

use netflash_core::color_for_score;
use netflash_icon::{IconRenderer, RgbaIcon, Skin};

fn main() {
    let out = Path::new("docs/skins");
    fs::create_dir_all(out).expect("docs/skins");
    let color = color_for_score(0.70);
    let size = 128;
    let mut tiles = Vec::new();
    for skin in Skin::ALL {
        let icon = skin.render(color, 0.70, size);
        write_png(&out.join(format!("{}.png", skin.key())), &icon);
        tiles.push((skin, icon));
    }
    write_gallery(&out.join("gallery.png"), &tiles);
    eprintln!("wrote {}", out.display());
}

fn write_png(path: &Path, icon: &RgbaIcon) {
    let file = fs::File::create(path).expect("png");
    let mut enc = png::Encoder::new(BufWriter::new(file), icon.width, icon.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("header")
        .write_image_data(&icon.rgba)
        .expect("data");
}

fn write_gallery(path: &Path, tiles: &[(Skin, RgbaIcon)]) {
    let pad = 20u32;
    let gap = 16u32;
    let tile = tiles[0].1.width;
    let n = tiles.len() as u32;
    let w = pad * 2 + n * tile + (n - 1) * gap;
    let h = pad * 2 + tile;
    let bg = [0, 0, 0, 0];
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }
    for (i, (_skin, icon)) in tiles.iter().enumerate() {
        let ox = pad + i as u32 * (tile + gap);
        let oy = pad;
        blit(&mut rgba, w, icon, ox, oy);
    }
    let file = fs::File::create(path).expect("gallery");
    let mut enc = png::Encoder::new(BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("header")
        .write_image_data(&rgba)
        .expect("data");
}

fn blit(dst: &mut [u8], dst_w: u32, src: &RgbaIcon, ox: u32, oy: u32) {
    for y in 0..src.height {
        for x in 0..src.width {
            let si = ((y * src.width + x) * 4) as usize;
            if src.rgba[si + 3] == 0 {
                continue;
            }
            let di = (((oy + y) * dst_w + (ox + x)) * 4) as usize;
            dst[di..di + 4].copy_from_slice(&src.rgba[si..si + 4]);
        }
    }
}
