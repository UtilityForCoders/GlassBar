//! Genera `assets/icon.ico`, el ícono de GlassBar:
//! Una barra de cristal líquido (Liquid Glass) con reflejos especulares y 4 apps brillantes.
//! Se dibuja con tiny-skia.
//!
//! Para regenerarlo:
//! ```powershell
//! cargo run --example icon
//! ```

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];

fn main() {
    let images: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, bmp_entry(&draw(s)))).collect();
    let ico = ico_file(&images);
    std::fs::create_dir_all("assets").expect("no se pudo crear assets/");
    std::fs::write("assets/icon.ico", &ico).expect("no se pudo escribir assets/icon.ico");
    println!(
        "assets/icon.ico generado: {} tamaños, {} bytes",
        images.len(),
        ico.len()
    );
}

fn draw(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("tamaño inválido");
    let k = size as f32 / 96.0;

    // Fondo suave oscuro
    fill_rounded(
        &mut pixmap,
        [18, 22, 30, 255],
        4.0,
        4.0,
        88.0,
        88.0,
        22.0,
        k,
    );

    // Borde sutil del fondo
    stroke_rounded(
        &mut pixmap,
        [255, 255, 255, 30],
        4.0,
        4.0,
        88.0,
        88.0,
        22.0,
        1.5,
        k,
    );

    // Barra de cristal estilo liquid dock
    fill_rounded(
        &mut pixmap,
        [40, 52, 70, 220],
        10.0,
        30.0,
        76.0,
        36.0,
        18.0,
        k,
    );

    // Reflejo especular superior (estilo cristal)
    fill_rounded(
        &mut pixmap,
        [255, 255, 255, 60],
        12.0,
        31.0,
        72.0,
        14.0,
        12.0,
        k,
    );

    // Borde brillante de cristal
    stroke_rounded(
        &mut pixmap,
        [255, 255, 255, 140],
        10.0,
        30.0,
        76.0,
        36.0,
        18.0,
        1.2,
        k,
    );

    // 4 iconos brillantes (Azul, Cian, Violeta, Ámbar)
    let colors = [
        [50, 130, 250, 255], // Explorer (Azul)
        [20, 200, 180, 255], // Browser (Cian)
        [160, 90, 240, 255], // Terminal (Violeta)
        [255, 170, 40, 255], // Code / Notas (Ámbar)
    ];

    for (i, col) in colors.iter().enumerate() {
        let cx = 23.0 + (i as f32) * 16.5;
        let cy = 48.0;
        let r = 5.0;
        fill_circle(&mut pixmap, *col, cx, cy, r, k);

        // Brillo especular en cada app
        fill_circle(
            &mut pixmap,
            [255, 255, 255, 180],
            cx - 1.2,
            cy - 1.2,
            1.6,
            k,
        );
    }

    pixmap
}

fn fill_rounded(
    pixmap: &mut Pixmap,
    [r, g, b, a]: [u8; 4],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
    k: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, a);
    if let Some(path) = rounded_rect(x * k, y * k, w * k, h * k, rad * k) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn stroke_rounded(
    pixmap: &mut Pixmap,
    [r, g, b, a]: [u8; 4],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
    stroke_w: f32,
    k: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, a);
    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = stroke_w * k;
    if let Some(path) = rounded_rect(x * k, y * k, w * k, h * k, rad * k) {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn fill_circle(pixmap: &mut Pixmap, [r, g, b, a]: [u8; 4], cx: f32, cy: f32, radius: f32, k: f32) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, a);
    let r = radius * k;
    let x = cx * k;
    let y = cy * k;
    let mut pb = PathBuilder::new();
    const KAPPA: f32 = 0.552_284_8;
    let c = r * KAPPA;
    pb.move_to(x, y - r);
    pb.cubic_to(x + c, y - r, x + r, y - c, x + r, y);
    pb.cubic_to(x + r, y + c, x + c, y + r, x, y + r);
    pb.cubic_to(x - c, y + r, x - r, y + c, x - r, y);
    pb.cubic_to(x - r, y - c, x - c, y - r, x, y - r);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    const KAPPA: f32 = 0.552_284_8;
    let r = r.min(w / 2.0).min(h / 2.0);
    let c = r * KAPPA;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + c, y, x1, y + r - c, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + c, x1 - r + c, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - c, y1, x, y1 - r + c, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    pb.close();
    pb.finish()
}

fn bmp_entry(pixmap: &Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mask_row = w.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, w, h * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(w * h * 4 + mask_row * h).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);

    for row in (0..h).rev() {
        for col in 0..w {
            let c = pixmap
                .pixel(col, row)
                .expect("píxel fuera del lienzo")
                .demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
    }
    out.resize(out.len() + (mask_row * h) as usize, 0);
    out
}

fn ico_file(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}
