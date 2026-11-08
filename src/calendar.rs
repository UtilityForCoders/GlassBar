//! Calendario que se abre al pulsar la hora del dock, con el estilo del panel
//! rápido: la fecha de hoy arriba, el mes con flechas para recorrerlo, la cuadrícula
//! de días (hoy resaltado con el color de énfasis) y, abajo, la configuración de
//! fecha y hora de Windows. Los nombres de meses y días salen del formato regional
//! del usuario.
use crate::apps::{fill_rounded, stroke_rounded};
use crate::clock::{
    draw_icon_aligned, draw_text_on_baseline, draw_text_semibold, fit_text, line_metrics,
    text_width,
};
use tiny_skia::Pixmap;

pub const WIDTH: f32 = 340.0;
const PAD: f32 = 12.0;
/// Fecha de hoy, en grande.
const TITLE_H: f32 = 48.0;
/// Mes mostrado y flechas.
const MONTH_H: f32 = 48.0;
/// Iniciales de los días de la semana.
const WEEKDAYS_H: f32 = 32.0;
const CELL_H: f32 = 40.0;
const WEEKS: usize = 6;
const FOOTER_H: f32 = 48.0;
const ARROW_W: f32 = 36.0;
const CELL_W: f32 = (WIDTH - PAD * 2.0) / 7.0;

pub const HEIGHT: f32 = TITLE_H + MONTH_H + WEEKDAYS_H + CELL_H * WEEKS as f32 + 8.0 + FOOTER_H;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// La fecha de hoy: vuelve al mes actual.
    Today,
    Previous,
    Next,
    /// Día de la cuadrícula, por su posición (0 a 41).
    Day(usize),
    /// "Configuración de fecha y hora".
    Settings,
}

/// Una fecha sin hora.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Día de la semana: 0 es lunes y 6 domingo.
pub fn weekday(date: Date) -> u32 {
    // Días desde el 1 de marzo del año 0 (algoritmo de Howard Hinnant).
    let (y, m) = if date.month <= 2 {
        (date.year - 1, date.month + 9)
    } else {
        (date.year, date.month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + date.day - 1;
    let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy as i64;
    let days = era as i64 * 146097 + doe - 719468; // 0 = 1 de enero de 1970, jueves
    (days + 3).rem_euclid(7) as u32
}

/// Mes anterior (`delta` negativo) o siguiente, como (año, mes).
pub fn add_months(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + delta;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

/// Las 42 fechas de la cuadrícula del mes: la semana del día 1, empezando por
/// `first_weekday` (0 lunes … 6 domingo), y las siguientes, con días de los meses
/// de al lado para completar.
pub fn grid(year: i32, month: u32, first_weekday: u32) -> [Date; WEEKS * 7] {
    let first = Date {
        year,
        month,
        day: 1,
    };
    let before = (weekday(first) + 7 - first_weekday % 7) % 7;
    let (prev_year, prev_month) = add_months(year, month, -1);
    let (next_year, next_month) = add_months(year, month, 1);
    let prev_days = days_in_month(prev_year, prev_month);
    let days = days_in_month(year, month);
    std::array::from_fn(|index| {
        let offset = index as i64 - before as i64;
        if offset < 0 {
            Date {
                year: prev_year,
                month: prev_month,
                day: (prev_days as i64 + offset + 1) as u32,
            }
        } else if offset < days as i64 {
            Date {
                year,
                month,
                day: offset as u32 + 1,
            }
        } else {
            Date {
                year: next_year,
                month: next_month,
                day: (offset - days as i64) as u32 + 1,
            }
        }
    })
}

/// Textos que dependen del formato regional del usuario.
#[derive(Clone, Debug, Default)]
pub struct Names {
    /// Fecha de hoy completa ("domingo, 8 de noviembre de 2026").
    pub today: String,
    /// Iniciales de los días, empezando por el primero de la semana ("LU", "MA"…).
    pub weekdays: [String; 7],
    /// Primer día de la semana (0 lunes … 6 domingo).
    pub first_weekday: u32,
}

pub struct CalendarState {
    pub today: Date,
    /// Mes mostrado.
    pub year: i32,
    pub month: u32,
    pub names: Names,
    /// Nombre del mes mostrado ("noviembre de 2026").
    pub title: String,
    pub hovered: Option<Hit>,
}

impl CalendarState {
    pub fn new(today: Date, names: Names) -> Self {
        let mut state = Self {
            today,
            year: today.year,
            month: today.month,
            names,
            title: String::new(),
            hovered: None,
        };
        state.title = month_title(state.year, state.month);
        state
    }

    pub fn dates(&self) -> [Date; WEEKS * 7] {
        grid(self.year, self.month, self.names.first_weekday)
    }

    /// Muestra otro mes (`delta` meses más adelante o atrás).
    pub fn shift(&mut self, delta: i32) {
        (self.year, self.month) = add_months(self.year, self.month, delta);
        self.title = month_title(self.year, self.month);
    }

    /// Vuelve al mes de hoy. Devuelve si cambió algo.
    pub fn go_today(&mut self) -> bool {
        let changed = (self.year, self.month) != (self.today.year, self.today.month);
        if changed {
            self.year = self.today.year;
            self.month = self.today.month;
            self.title = month_title(self.year, self.month);
        }
        changed
    }

    /// Pulsar un día de otro mes lleva a ese mes. Devuelve si cambió algo.
    pub fn choose_day(&mut self, index: usize) -> bool {
        let Some(date) = self.dates().get(index).copied() else {
            return false;
        };
        if (date.year, date.month) == (self.year, self.month) {
            return false;
        }
        self.year = date.year;
        self.month = date.month;
        self.title = month_title(self.year, self.month);
        true
    }

    fn grid_top() -> f32 {
        TITLE_H + MONTH_H + WEEKDAYS_H
    }
    fn footer_y() -> f32 {
        Self::grid_top() + CELL_H * WEEKS as f32 + 8.0
    }
    /// Centro de la flecha anterior (arriba) y siguiente (abajo), como en Windows.
    fn arrow_center(next: bool) -> (f32, f32) {
        let right = WIDTH - PAD - ARROW_W / 2.0;
        let x = if next { right } else { right - ARROW_W - 4.0 };
        (x, TITLE_H + MONTH_H / 2.0)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hit> {
        if !(0.0..WIDTH).contains(&x) || !(0.0..HEIGHT).contains(&y) {
            return None;
        }
        if y < TITLE_H {
            return Some(Hit::Today);
        }
        if y < TITLE_H + MONTH_H {
            for next in [false, true] {
                let (cx, cy) = Self::arrow_center(next);
                if (x - cx).abs() <= ARROW_W / 2.0 && (y - cy).abs() <= 16.0 {
                    return Some(if next { Hit::Next } else { Hit::Previous });
                }
            }
            return None;
        }
        let top = Self::grid_top();
        if y >= top && y < top + CELL_H * WEEKS as f32 && x >= PAD && x < WIDTH - PAD {
            let row = ((y - top) / CELL_H) as usize;
            let column = (((x - PAD) / CELL_W) as usize).min(6);
            return Some(Hit::Day(row * 7 + column));
        }
        (y >= Self::footer_y()).then_some(Hit::Settings)
    }

    pub fn draw(&self, image: &mut Pixmap, scale: f32) {
        image.fill(tiny_skia::Color::TRANSPARENT);
        let (w, h) = ((WIDTH - 2.0) * scale, (HEIGHT - 2.0) * scale);
        fill_rounded(
            image,
            crate::glass::panel(),
            scale,
            scale,
            w,
            h,
            12.0 * scale,
        );
        // Líneas bajo la fecha de hoy y sobre el pie, como en el panel rápido.
        for y in [TITLE_H, Self::footer_y()] {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                scale,
                (y * scale).round(),
                w,
                scale.max(1.0),
                0.0,
            );
        }
        stroke_rounded(
            image,
            [255, 255, 255, 34],
            scale,
            scale,
            w,
            h,
            12.0 * scale,
            scale * 0.75,
        );
        let hover = |hit: Hit| self.hovered == Some(hit);
        let size = 13.0 * scale;
        let (ascent, descent) = line_metrics(size);
        let centered = |cy: f32| cy * scale + (ascent - descent) / 2.0;

        // Fecha de hoy.
        if hover(Hit::Today) {
            fill_rounded(
                image,
                [255, 255, 255, 14],
                (PAD - 4.0) * scale,
                6.0 * scale,
                (WIDTH - PAD * 2.0 + 8.0) * scale,
                (TITLE_H - 12.0) * scale,
                6.0 * scale,
            );
        }
        let today = fit_text(&self.names.today, size, (WIDTH - PAD * 2.0 - 8.0) * scale);
        draw_text_on_baseline(
            image,
            &today,
            (PAD + 4.0) * scale,
            centered(TITLE_H / 2.0),
            size,
            0.95,
        );

        // Mes mostrado y flechas.
        let month_size = 14.0 * scale;
        draw_text_semibold(
            image,
            &self.title,
            (PAD + 4.0) * scale,
            (TITLE_H + MONTH_H / 2.0) * scale - month_size * 0.62,
            month_size,
            1.0,
        );
        for (next, icon) in [(false, '\u{E70E}'), (true, '\u{E70D}')] {
            let (cx, cy) = Self::arrow_center(next);
            let hit = if next { Hit::Next } else { Hit::Previous };
            if hover(hit) {
                fill_rounded(
                    image,
                    [255, 255, 255, 20],
                    (cx - ARROW_W / 2.0) * scale,
                    (cy - 16.0) * scale,
                    ARROW_W * scale,
                    32.0 * scale,
                    6.0 * scale,
                );
            }
            draw_icon_aligned(
                image,
                icon,
                cx * scale,
                cy * scale,
                11.0 * scale,
                if hover(hit) { 1.0 } else { 0.8 },
            );
        }

        // Iniciales de los días.
        let small = 12.0 * scale;
        let (small_ascent, small_descent) = line_metrics(small);
        let weekdays_cy = TITLE_H + MONTH_H + WEEKDAYS_H / 2.0;
        for (column, name) in self.names.weekdays.iter().enumerate() {
            let cx = PAD + CELL_W * (column as f32 + 0.5);
            draw_text_on_baseline(
                image,
                name,
                cx * scale - text_width(name, small) / 2.0,
                weekdays_cy * scale + (small_ascent - small_descent) / 2.0,
                small,
                0.85,
            );
        }

        // Días.
        let accent = crate::accent::current();
        let top = Self::grid_top();
        let radius = (CELL_H - 6.0).min(CELL_W - 6.0) / 2.0;
        for (index, date) in self.dates().iter().enumerate() {
            let cx = PAD + CELL_W * ((index % 7) as f32 + 0.5);
            let cy = top + CELL_H * ((index / 7) as f32 + 0.5);
            let is_today = *date == self.today;
            let this_month = (date.year, date.month) == (self.year, self.month);
            let circle = |image: &mut Pixmap, color: [u8; 4]| {
                fill_rounded(
                    image,
                    color,
                    (cx - radius) * scale,
                    (cy - radius) * scale,
                    radius * 2.0 * scale,
                    radius * 2.0 * scale,
                    radius * scale,
                );
            };
            if is_today {
                circle(
                    image,
                    if hover(Hit::Day(index)) {
                        accent.base
                    } else {
                        accent.dark
                    },
                );
            } else if hover(Hit::Day(index)) {
                circle(image, [255, 255, 255, 22]);
            }
            let text = date.day.to_string();
            draw_text_on_baseline(
                image,
                &text,
                cx * scale - text_width(&text, size) / 2.0,
                centered(cy),
                size,
                match (is_today, this_month) {
                    (true, _) => 1.0,
                    (false, true) => 0.92,
                    (false, false) => 0.4,
                },
            );
        }

        // Pie.
        let footer = Self::footer_y();
        if hover(Hit::Settings) {
            fill_rounded(
                image,
                [255, 255, 255, 20],
                (PAD - 4.0) * scale,
                (footer + 8.0) * scale,
                (WIDTH - PAD * 2.0 + 8.0) * scale,
                (FOOTER_H - 16.0) * scale,
                5.0 * scale,
            );
        }
        draw_text_on_baseline(
            image,
            "Configuración de fecha y hora",
            (PAD + 4.0) * scale,
            centered(footer + FOOTER_H / 2.0),
            size,
            0.9,
        );
    }
}

// ---------------------------------------------------------------------------
// FORMATO REGIONAL
// ---------------------------------------------------------------------------

fn system_time(date: Date) -> windows::Win32::Foundation::SYSTEMTIME {
    windows::Win32::Foundation::SYSTEMTIME {
        wYear: date.year as u16,
        wMonth: date.month as u16,
        wDay: date.day as u16,
        wDayOfWeek: ((weekday(date) + 1) % 7) as u16,
        ..Default::default()
    }
}

/// Fecha con el formato regional del usuario: `flags` de `GetDateFormatEx` o un
/// patrón propio ("ddd").
fn format(date: Date, flags: u32, pattern: Option<&str>) -> Option<String> {
    use windows::Win32::Globalization::{GetDateFormatEx, ENUM_DATE_FORMATS_FLAGS};
    let pattern: Option<Vec<u16>> = pattern.map(|p| p.encode_utf16().chain(Some(0)).collect());
    let mut buffer = [0u16; 128];
    let len = unsafe {
        GetDateFormatEx(
            windows::core::PCWSTR::null(),
            ENUM_DATE_FORMATS_FLAGS(flags),
            Some(&system_time(date)),
            pattern.as_ref().map_or(windows::core::PCWSTR::null(), |p| {
                windows::core::PCWSTR(p.as_ptr())
            }),
            Some(&mut buffer),
            windows::core::PCWSTR::null(),
        )
    };
    (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

/// "noviembre de 2026", con la primera letra en minúscula como en Windows.
fn month_title(year: i32, month: u32) -> String {
    use windows::Win32::Globalization::DATE_YEARMONTH;
    let date = Date {
        year,
        month,
        day: 1,
    };
    format(date, DATE_YEARMONTH.0, None).unwrap_or_else(|| format!("{month:02}/{year}"))
}

/// Primer día de la semana del usuario (0 lunes … 6 domingo).
fn first_weekday() -> u32 {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};
    let mut buffer = [0u16; 4];
    let len = unsafe {
        GetLocaleInfoEx(
            windows::core::PCWSTR::null(),
            LOCALE_IFIRSTDAYOFWEEK,
            Some(&mut buffer),
        )
    };
    if len < 2 {
        return 0;
    }
    String::from_utf16_lossy(&buffer[..len as usize - 1])
        .parse::<u32>()
        .map_or(0, |d| d.min(6))
}

/// Inicial de un día de la semana como la muestra Windows: dos letras en
/// mayúscula ("lun." → "LU").
pub fn short_weekday(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphabetic())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Hoy, según el reloj de Windows.
pub fn today() -> Date {
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Date {
        year: now.wYear as i32,
        month: now.wMonth as u32,
        day: now.wDay as u32,
    }
}

/// Textos del calendario en el idioma del usuario.
pub fn names(today: Date) -> Names {
    let first = first_weekday();
    // El 5 de enero de 2026 es lunes: de ahí sale cada día de la semana.
    let weekdays = std::array::from_fn(|column| {
        let day = 5 + (first + column as u32) % 7;
        let date = Date {
            year: 2026,
            month: 1,
            day,
        };
        format(date, 0, Some("ddd"))
            .map(|n| short_weekday(&n))
            .unwrap_or_default()
    });
    Names {
        today: crate::clock::today(true),
        weekdays,
        first_weekday: first,
    }
    .with_today(today)
}

impl Names {
    /// La fecha de hoy sin el año, como en Windows ("domingo, 8 de noviembre").
    fn with_today(mut self, today: Date) -> Self {
        let year = today.year.to_string();
        for suffix in [
            format!(" de {year}"),
            format!(", {year}"),
            format!(" {year}"),
        ] {
            if let Some(short) = self.today.strip_suffix(&suffix) {
                self.today = short.to_string();
                break;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn weekdays_and_month_lengths() {
        assert_eq!(weekday(date(2026, 11, 8)), 6); // domingo
        assert_eq!(weekday(date(2026, 1, 5)), 0); // lunes
        assert_eq!(weekday(date(2000, 2, 29)), 1); // martes
        assert_eq!(weekday(date(1970, 1, 1)), 3); // jueves
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 11, -23), (2024, 12));
    }

    #[test]
    fn grid_starts_on_the_first_day_of_the_week() {
        // Noviembre de 2026 empieza en domingo: con la semana en lunes, la primera
        // fila va del 26 de octubre al 1 de noviembre, como en Windows.
        let days = grid(2026, 11, 0);
        assert_eq!(days[0], date(2026, 10, 26));
        assert_eq!(days[6], date(2026, 11, 1));
        assert_eq!(days[35], date(2026, 11, 30));
        assert_eq!(days[41], date(2026, 12, 6));
        // Con la semana en domingo, el día 1 abre la cuadrícula.
        let days = grid(2026, 11, 6);
        assert_eq!(days[0], date(2026, 11, 1));
    }

    #[test]
    fn clicks_land_where_things_are_drawn() {
        let mut state = CalendarState {
            today: date(2026, 11, 8),
            year: 2026,
            month: 11,
            names: Names::default(),
            title: String::new(),
            hovered: None,
        };
        assert_eq!(state.hit_test(WIDTH / 2.0, TITLE_H / 2.0), Some(Hit::Today));
        let (x, y) = CalendarState::arrow_center(false);
        assert_eq!(state.hit_test(x, y), Some(Hit::Previous));
        let (x, y) = CalendarState::arrow_center(true);
        assert_eq!(state.hit_test(x, y), Some(Hit::Next));
        let top = CalendarState::grid_top();
        // Domingo 8: segunda fila, última columna.
        let cell = state.hit_test(PAD + CELL_W * 6.5, top + CELL_H * 1.5);
        assert_eq!(cell, Some(Hit::Day(13)));
        assert_eq!(state.dates()[13], date(2026, 11, 8));
        assert_eq!(
            state.hit_test(WIDTH / 2.0, HEIGHT - FOOTER_H / 2.0),
            Some(Hit::Settings)
        );
        assert_eq!(state.hit_test(WIDTH / 2.0, HEIGHT + 1.0), None);

        // Un día de octubre lleva a octubre; uno de noviembre no cambia nada.
        assert!(!state.choose_day(13));
        assert!(state.choose_day(0));
        assert_eq!((state.year, state.month), (2026, 10));
        assert!(state.go_today());
        assert_eq!((state.year, state.month), (2026, 11));
    }

    #[test]
    fn weekday_initials_look_like_windows() {
        assert_eq!(short_weekday("lun."), "LU");
        assert_eq!(short_weekday("mié."), "MI");
        assert_eq!(short_weekday("Sun"), "SU");
        let names = Names {
            today: "domingo, 8 de noviembre de 2026".into(),
            ..Default::default()
        }
        .with_today(date(2026, 11, 8));
        assert_eq!(names.today, "domingo, 8 de noviembre");
    }
}
