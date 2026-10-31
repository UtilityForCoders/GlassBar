//! Genera una vista previa del dock en `dock_preview.bmp` para comprobar el renderizado exacto.

// Incluye los módulos de la app con `#[path]` pero usa solo una parte: lo que solo
// usa la app principal no es código muerto, aunque aquí lo parezca.
#![allow(dead_code)]

use std::fs::File;
use std::io::Write;
use tiny_skia::Pixmap;

#[path = "../src/accent.rs"]
mod accent;
#[path = "../src/appmenu.rs"]
mod appmenu;
#[path = "../src/apps.rs"]
mod apps;
#[path = "../src/bar.rs"]
mod bar;
#[path = "../src/catalog.rs"]
mod catalog;
#[path = "../src/clock.rs"]
mod clock;
#[path = "../src/font.rs"]
mod font;
#[path = "../src/glass.rs"]
mod glass;
#[path = "../src/icons.rs"]
mod icons;
#[path = "../src/jumplist.rs"]
mod jumplist;
#[path = "../src/keyboard.rs"]
mod keyboard;
#[path = "../src/launcher.rs"]
mod launcher;
#[path = "../src/pins.rs"]
mod pins;
#[path = "../src/spring.rs"]
mod spring;
#[path = "../src/status.rs"]
mod status;
#[path = "../src/style.rs"]
mod style;
#[path = "../src/textfield.rs"]
mod textfield;
#[path = "../src/usage.rs"]
mod usage;

use bar::BarState;

fn main() {
    let scale: f32 = std::env::var("SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let mut state = BarState::new();
    for command in std::env::args().skip(1) {
        if let Some(item) = pins::from_drop(command) {
            state.primary_apps.push(item);
        }
    }
    if std::env::args().any(|a| a == "--launcher" || a == "--launcher-power") {
        let pinned: Vec<_> = state.primary_apps.iter().map(|a| a.entry.clone()).collect();
        let started = std::time::Instant::now();
        let mut menu = launcher::LauncherState::new(&pinned);
        let opened = started.elapsed();
        let started = std::time::Instant::now();
        menu.preload_icons();
        let preloaded = started.elapsed();
        let arguments: Vec<String> = std::env::args().collect();
        if let Some(index) = arguments.iter().position(|a| a == "--search") {
            if let Some(query) = arguments.get(index + 1) {
                menu.insert_text(query);
                for i in catalog::search(&menu.apps, query) {
                    println!(
                        "Resultado: {} | {}",
                        menu.apps[i].name, menu.apps[i].command
                    );
                }
            }
        }
        menu.power_menu = std::env::args().any(|a| a == "--launcher-power");
        if menu.power_menu {
            menu.hovered = Some(launcher::Hit::PowerOption(launcher::Power::Shutdown));
        }
        let mut image = Pixmap::new(
            (launcher::WIDTH * scale) as u32,
            (launcher::HEIGHT * scale) as u32,
        )
        .unwrap();
        let started = std::time::Instant::now();
        menu.draw(&mut image, scale);
        let first = started.elapsed();
        // Lo que cuesta cada cambio de hover: iconos y glifos ya en caché.
        let started = std::time::Instant::now();
        menu.draw(&mut image, scale);
        let redraw = started.elapsed();
        std::fs::write("target/launcher-preview.bmp", bmp_entry(&image)).unwrap();
        println!(
            "Lanzador: {} aplicaciones; vista previa en target/launcher-preview.bmp",
            menu.apps.len()
        );
        println!("Tiempos: catálogo {opened:?}, precarga de iconos {preloaded:?}, primer dibujo {first:?}, redibujo {redraw:?}");
        return;
    }
    // Menú de una app con su jump list real (p. ej. `--appmenu Chrome`), en
    // target/appmenu-preview.bmp.
    if let Some(index) = arguments_of("--appmenu") {
        let id = std::env::args()
            .nth(index + 1)
            .unwrap_or_else(|| "Chrome".into());
        let side = (appmenu::ICON * scale).round() as u32;
        let small = |icon: Option<Pixmap>| icon.map(|i| apps::downscale(&i, side));
        let mut entries = Vec::new();
        let mut next = appmenu::CMD_JUMP;
        for section in jumplist::sections(&id) {
            entries.push(appmenu::Entry::Header(section.title));
            for item in section.items {
                let (path, number) = item.icon_source();
                entries.push(appmenu::Entry::Item {
                    icon: appmenu::Icon::Image(small(icons::load_resource(&path, number))),
                    label: item.name,
                    command: next,
                });
                next += 1;
            }
        }
        entries.push(appmenu::Entry::Separator);
        entries.push(appmenu::Entry::Item {
            icon: appmenu::Icon::Image(small(icons::load(&format!("shell:AppsFolder\\{id}")))),
            label: id.clone(),
            command: appmenu::CMD_APP_OPEN,
        });
        entries.push(appmenu::Entry::Item {
            icon: appmenu::Icon::Glyph(appmenu::GLYPH_UNPIN),
            label: "Desanclar de la barra de tareas".into(),
            command: appmenu::CMD_APP_UNPIN,
        });
        entries.push(appmenu::Entry::Item {
            icon: appmenu::Icon::Glyph(appmenu::GLYPH_END_TASK),
            label: "Finalizar tarea".into(),
            command: appmenu::CMD_APP_END_TASK,
        });
        entries.push(appmenu::Entry::Item {
            icon: appmenu::Icon::Glyph(appmenu::GLYPH_CLOSE),
            label: "Cerrar ventana".into(),
            command: appmenu::CMD_APP_CLOSE,
        });
        let mut menu = appmenu::AppMenu::new(entries);
        // La tercera opción resaltada, para ver el efecto al pasar el cursor.
        menu.move_hover(true);
        menu.move_hover(true);
        menu.move_hover(true);
        let (w, h) = menu.size();
        let mut image = Pixmap::new((w * scale).ceil() as u32, (h * scale).ceil() as u32).unwrap();
        menu.draw(&mut image, scale);
        std::fs::write("target/appmenu-preview.bmp", bmp_entry(&image)).unwrap();
        println!("Menú de {id}: target/appmenu-preview.bmp");
        return;
    }
    // Jump list de cualquier AppUserModelID (p. ej. `--jumplist-id Chrome`).
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(index) = arguments.iter().position(|a| a == "--jumplist-id") {
        for id in &arguments[index + 1..] {
            println!("{id}");
            for section in jumplist::sections(id) {
                println!("    [{}]", section.title);
                for item in section.items {
                    println!("      {} | {:?}", item.name, item.action);
                }
            }
        }
        return;
    }
    // Jump list de cada app del dock: su AppUserModelID y lo que daría el clic derecho.
    if std::env::args().any(|a| a == "--jumplist") {
        apps::update_running_apps(&mut state.primary_apps, &mut state.open_apps);
        for a in state.primary_apps.iter().chain(&state.open_apps) {
            let command = a.pin_entry().command;
            let id = jumplist::app_id(&command, &a.windows);
            println!("{} | id {:?}", a.entry.name, id);
            let sections = id.as_deref().map(jumplist::sections).unwrap_or_default();
            if sections.is_empty() {
                println!("    (sin jump list)");
            }
            for section in sections {
                println!("    [{}]", section.title);
                for item in section.items {
                    println!("      {} | {:?}", item.name, item.action);
                }
            }
        }
        return;
    }
    let open = std::env::args().any(|a| a == "--open");
    if open {
        apps::update_running_apps(&mut state.primary_apps, &mut state.open_apps);
        for w in apps::open_windows() {
            println!("Ventana: {} | {}", w.hwnd, w.exe);
        }
        for a in state.primary_apps.iter().chain(&state.open_apps) {
            println!(
                "Icono: {} | {} | abierto={} | icono={}",
                a.entry.name,
                a.entry.command,
                a.is_running,
                a.icon.is_some()
            );
        }
    }
    let (w, h) = state.window_size(scale);
    let mut pixmap = Pixmap::new(w, h).expect("pixmap failed");

    if let Some(app) = state.primary_apps.first_mut().filter(|_| !open) {
        app.is_running = true;
        state.hovered = Some(bar::HoverTarget::PrimaryApp(0));
    }

    state.draw(&mut pixmap, scale);

    let bmp = bmp_entry(&pixmap);
    let mut file = File::create("dock_preview.bmp").expect("failed to create dock_preview.bmp");
    file.write_all(&bmp).expect("failed to write bmp");
    println!("Vista previa guardada en dock_preview.bmp ({}x{})", w, h);
}

/// Posición del argumento `name`, si se pasó.
fn arguments_of(name: &str) -> Option<usize> {
    std::env::args().position(|a| a == name)
}

fn bmp_entry(pixmap: &Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut out = Vec::new();
    // BMP Header (14 bytes)
    out.extend_from_slice(b"BM");
    let file_size = 14 + 40 + (w * h * 4);
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes()); // offset to pixel data

    // BITMAPINFOHEADER (40 bytes)
    let bi_height = -(h as i32); // negativo = top-down
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&bi_height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bit count
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(w * h * 4).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);

    for row in 0..h {
        for col in 0..w {
            let c = pixmap
                .pixel(col, row)
                .expect("píxel fuera del lienzo")
                .demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
    }
    out
}
