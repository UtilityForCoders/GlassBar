//! Fuentes TrueType leídas bajo demanda: el archivo se proyecta en memoria tal cual
//! y cada letra se dibuja la primera vez que hace falta. Así una fuente no ocupa
//! memoria propia (sus páginas se comparten con el resto del sistema), y no las
//! decenas de MB de tener todas sus letras ya preparadas.
//!
//! El antialias calcula el área exacta que cubre la letra en cada píxel, el mismo
//! método de `font-rs` y `fontdue`, así que el texto se ve igual que antes.
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// Posición y tamaño de una letra dibujada, en píxeles. `ymin` es el borde inferior
/// respecto a la línea base (hacia arriba es positivo).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub xmin: i32,
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
}

/// Altura de una línea: `descent` es negativo (por debajo de la línea base).
#[derive(Clone, Copy, Debug)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone)]
pub struct Font {
    face: Face<'static>,
}

impl Font {
    /// La fuente del archivo `path`, proyectado en memoria en lugar de leído: sus
    /// páginas se comparten con los demás programas que usan la misma fuente y no
    /// cuentan como memoria propia de GlassBar. Las fuentes viven mientras la app,
    /// así que la proyección no se cierra nunca.
    pub fn from_file(path: &std::path::Path) -> Option<Self> {
        let bytes = map_file(path)?;
        Some(Self {
            face: Face::parse(bytes, 0).ok()?,
        })
    }

    fn scale(&self, size: f32) -> f32 {
        size / self.face.units_per_em() as f32
    }

    fn glyph(&self, c: char) -> GlyphId {
        self.face.glyph_index(c).unwrap_or(GlyphId(0))
    }

    pub fn horizontal_line_metrics(&self, size: f32) -> Option<LineMetrics> {
        let scale = self.scale(size);
        Some(LineMetrics {
            ascent: self.face.ascender() as f32 * scale,
            descent: self.face.descender() as f32 * scale,
        })
    }

    pub fn metrics(&self, c: char, size: f32) -> Metrics {
        let id = self.glyph(c);
        let scale = self.scale(size);
        let advance_width = self.face.glyph_hor_advance(id).unwrap_or(0) as f32 * scale;
        match self.face.glyph_bounding_box(id) {
            Some(bbox) => bounds(
                bbox.x_min as f32 * scale,
                bbox.y_min as f32 * scale,
                bbox.x_max as f32 * scale,
                bbox.y_max as f32 * scale,
                advance_width,
            ),
            None => Metrics {
                advance_width,
                ..Default::default()
            },
        }
    }

    /// La letra `c` a `size` píxeles: su posición y la cobertura de cada píxel
    /// (0–255), fila a fila de arriba abajo.
    pub fn rasterize(&self, c: char, size: f32) -> (Metrics, Vec<u8>) {
        let id = self.glyph(c);
        let scale = self.scale(size);
        let mut outline = Outline::default();
        let Some(bbox) = self.face.outline_glyph(id, &mut outline) else {
            return (self.metrics(c, size), Vec::new());
        };
        let advance_width = self.face.glyph_hor_advance(id).unwrap_or(0) as f32 * scale;
        let metrics = bounds(
            bbox.x_min as f32 * scale,
            bbox.y_min as f32 * scale,
            bbox.x_max as f32 * scale,
            bbox.y_max as f32 * scale,
            advance_width,
        );
        if metrics.width == 0 || metrics.height == 0 {
            return (metrics, Vec::new());
        }
        // De unidades de la fuente (y hacia arriba) a píxeles de la imagen (y hacia abajo).
        let top = (metrics.ymin + metrics.height as i32) as f32;
        let left = metrics.xmin as f32;
        let to_pixels = |(x, y): (f32, f32)| (x * scale - left, top - y * scale);
        let mut raster = Raster::new(metrics.width, metrics.height);
        for contour in &outline.contours {
            let points: Vec<(f32, f32)> = contour.iter().map(|&p| to_pixels(p)).collect();
            for pair in points.windows(2) {
                raster.line(pair[0], pair[1]);
            }
        }
        (metrics, raster.coverage())
    }
}

/// Proyecta el archivo `path` en memoria, solo lectura, para toda la vida de la app.
fn map_file(path: &std::path::Path) -> Option<&'static [u8]> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileSizeEx, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ,
        OPEN_EXISTING,
    };
    use windows::Win32::System::Memory::{
        CreateFileMappingW, MapViewOfFile, FILE_MAP_READ, PAGE_READONLY,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let file = CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .ok()?;
        let mut size = 0i64;
        let mapping = GetFileSizeEx(file, &mut size)
            .ok()
            .filter(|_| size > 0)
            .and_then(|_| CreateFileMappingW(file, None, PAGE_READONLY, 0, 0, None).ok());
        // La proyección mantiene el archivo abierto por su cuenta.
        let _ = CloseHandle(file);
        let mapping = mapping?;
        let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
        let _ = CloseHandle(mapping);
        if view.Value.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(
            view.Value as *const u8,
            size as usize,
        ))
    }
}

/// Caja de píxeles enteros que contiene la letra.
fn bounds(xmin: f32, ymin: f32, xmax: f32, ymax: f32, advance_width: f32) -> Metrics {
    let (left, bottom) = (xmin.floor(), ymin.floor());
    Metrics {
        xmin: left as i32,
        ymin: bottom as i32,
        width: (xmax.ceil() - left).max(0.0) as usize,
        height: (ymax.ceil() - bottom).max(0.0) as usize,
        advance_width,
    }
}

/// Contornos de una letra convertidos en polilíneas cerradas (las curvas se
/// dividen en tramos rectos), en unidades de la fuente.
#[derive(Default)]
struct Outline {
    contours: Vec<Vec<(f32, f32)>>,
    current: Vec<(f32, f32)>,
}

impl Outline {
    fn last(&self) -> (f32, f32) {
        self.current.last().copied().unwrap_or((0.0, 0.0))
    }
    /// Tramos según lo curva que sea, en unidades de la fuente (~2048 por em): de
    /// sobra para que no se noten a tamaño de interfaz.
    fn steps(deviation: f32) -> usize {
        ((deviation / 8.0).sqrt().ceil() as usize).clamp(1, 16)
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.current.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push((x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let deviation = (x0 - 2.0 * x1 + x).hypot(y0 - 2.0 * y1 + y);
        let n = Self::steps(deviation);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.current.push((
                u * u * x0 + 2.0 * u * t * x1 + t * t * x,
                u * u * y0 + 2.0 * u * t * y1 + t * t * y,
            ));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let deviation = (x0 - 2.0 * x1 + x2)
            .hypot(y0 - 2.0 * y1 + y2)
            .max((x1 - 2.0 * x2 + x).hypot(y1 - 2.0 * y2 + y));
        let n = Self::steps(deviation * 1.5);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.current.push((
                a * x0 + b * x1 + c * x2 + d * x,
                a * y0 + b * y1 + c * y2 + d * y,
            ));
        }
    }
    fn close(&mut self) {
        if let Some(&first) = self.current.first() {
            if self.last() != first {
                self.current.push(first);
            }
            self.contours.push(std::mem::take(&mut self.current));
        }
    }
}

/// Rasterizador por área exacta (el de `font-rs`): cada segmento suma en cada
/// píxel la parte que deja a su derecha, y luego cada fila se acumula de izquierda
/// a derecha. Las filas llevan dos píxeles de margen para que nada se salga.
struct Raster {
    width: usize,
    height: usize,
    stride: usize,
    area: Vec<f32>,
}

impl Raster {
    fn new(width: usize, height: usize) -> Self {
        let stride = width + 2;
        Self {
            width,
            height,
            stride,
            area: vec![0.0; stride * height + 2],
        }
    }

    fn add(&mut self, row: usize, x: i32, value: f32) {
        let x = x.clamp(0, self.stride as i32 - 1) as usize;
        self.area[row * self.stride + x] += value;
    }

    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if (p0.1 - p1.1).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let first = p0.1.max(0.0) as usize;
        let last = (p1.1.ceil().max(0.0) as usize).min(self.height);
        for row in first..last {
            let dy = ((row + 1) as f32).min(p1.1) - (row as f32).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0floor = x0.floor();
            let x0i = x0floor as i32;
            let x1ceil = x1.ceil();
            let x1i = x1ceil as i32;
            if x1i <= x0i + 1 {
                // Todo el tramo cae en un píxel.
                let xmf = 0.5 * (x + xnext) - x0floor;
                self.add(row, x0i, d - d * xmf);
                self.add(row, x0i + 1, d * xmf);
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.add(row, x0i, d * a0);
                if x1i == x0i + 2 {
                    self.add(row, x0i + 1, d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.add(row, x0i + 1, d * (a1 - a0));
                    for xi in x0i + 2..x1i - 1 {
                        self.add(row, xi, d * s);
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.add(row, x1i - 1, d * (1.0 - a2 - am));
                }
                self.add(row, x1i, d * am);
            }
            x = xnext;
        }
    }

    fn coverage(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height);
        for row in 0..self.height {
            let mut sum = 0.0;
            for x in 0..self.width {
                sum += self.area[row * self.stride + x];
                out.push((sum.abs().min(1.0) * 255.0 + 0.5) as u8);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Raster;

    #[test]
    fn coverage_is_the_exact_area_inside_the_shape() {
        // Cuadrado de (1, 1) a (3, 3) en una imagen de 4 × 4: lleno dentro, vacío fuera.
        let mut raster = Raster::new(4, 4);
        let square = [(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0), (1.0, 1.0)];
        for pair in square.windows(2) {
            raster.line(pair[0], pair[1]);
        }
        let image = raster.coverage();
        for y in 0..4 {
            for x in 0..4 {
                let inside = (1..3).contains(&x) && (1..3).contains(&y);
                assert_eq!(image[y * 4 + x], if inside { 255 } else { 0 }, "({x}, {y})");
            }
        }

        // Un borde a mitad de píxel lo cubre a medias; el sentido no importa.
        let mut raster = Raster::new(2, 1);
        let half = [(0.0, 0.0), (0.0, 1.0), (1.5, 1.0), (1.5, 0.0), (0.0, 0.0)];
        for pair in half.windows(2) {
            raster.line(pair[0], pair[1]);
        }
        assert_eq!(raster.coverage(), vec![255, 128]);
    }
}
