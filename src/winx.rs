//! Menú del clic derecho en el botón de inicio, con las mismas opciones que el de
//! Windows (Win+X): herramientas del sistema, Terminal, Configuración, Ejecutar,
//! apagar o cerrar sesión y mostrar el escritorio.
use crate::appmenu::{AppMenu, Entry, Icon};
use std::path::PathBuf;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_COMMAND};

/// Órdenes de este menú: `COMMANDS + índice` en `Action::ALL`.
const COMMANDS: u32 = 500;

/// Órdenes que atiende la barra de Explorer (oculta, pero sigue funcionando): las
/// mismas que usa Windows para "Ejecutar" y "Escritorio".
const TRAY_RUN: usize = 401;
const TRAY_TOGGLE_DESKTOP: usize = 407;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    InstalledApps,
    Mobility,
    PowerOptions,
    EventViewer,
    System,
    DeviceManager,
    Network,
    DiskManagement,
    ComputerManagement,
    Terminal,
    TerminalAdmin,
    TaskManager,
    Settings,
    Explorer,
    Search,
    Run,
    SignOut,
    Sleep,
    Shutdown,
    Restart,
    Desktop,
}

impl Action {
    pub const ALL: [Action; 21] = [
        Action::InstalledApps,
        Action::Mobility,
        Action::PowerOptions,
        Action::EventViewer,
        Action::System,
        Action::DeviceManager,
        Action::Network,
        Action::DiskManagement,
        Action::ComputerManagement,
        Action::Terminal,
        Action::TerminalAdmin,
        Action::TaskManager,
        Action::Settings,
        Action::Explorer,
        Action::Search,
        Action::Run,
        Action::SignOut,
        Action::Sleep,
        Action::Shutdown,
        Action::Restart,
        Action::Desktop,
    ];

    pub fn command(self) -> u32 {
        COMMANDS + Self::ALL.iter().position(|&a| a == self).unwrap_or(0) as u32
    }

    pub fn from_command(command: u32) -> Option<Self> {
        Self::ALL
            .get(command.checked_sub(COMMANDS)? as usize)
            .copied()
    }

    fn label(self) -> &'static str {
        match self {
            Action::InstalledApps => "Aplicaciones instaladas",
            Action::Mobility => "Centro de movilidad",
            Action::PowerOptions => "Opciones de energía",
            Action::EventViewer => "Visor de eventos",
            Action::System => "Sistema",
            Action::DeviceManager => "Administrador de dispositivos",
            Action::Network => "Conexiones de red",
            Action::DiskManagement => "Administración de discos",
            Action::ComputerManagement => "Administración de equipos",
            Action::Terminal => "Terminal",
            Action::TerminalAdmin => "Terminal (Administrador)",
            Action::TaskManager => "Administrador de tareas",
            Action::Settings => "Configuración",
            Action::Explorer => "Explorador de archivos",
            Action::Search => "Buscar",
            Action::Run => "Ejecutar",
            Action::SignOut => "Cerrar sesión",
            Action::Sleep => "Suspender",
            Action::Shutdown => "Apagar",
            Action::Restart => "Reiniciar",
            Action::Desktop => "Escritorio",
        }
    }

    /// Glifo de Segoe Fluent Icons / MDL2 Assets.
    fn glyph(self) -> char {
        match self {
            Action::InstalledApps => '\u{E71D}',
            Action::Mobility => '\u{E7F8}',
            Action::PowerOptions => '\u{E83F}',
            Action::EventViewer => '\u{E81C}',
            Action::System => '\u{E770}',
            Action::DeviceManager => '\u{E772}',
            Action::Network => '\u{E839}',
            Action::DiskManagement => '\u{EDA2}',
            Action::ComputerManagement => '\u{E912}',
            Action::Terminal => '\u{E756}',
            Action::TerminalAdmin => '\u{E7EF}',
            Action::TaskManager => '\u{E9D9}',
            Action::Settings => '\u{E713}',
            Action::Explorer => '\u{EC50}',
            Action::Search => '\u{E721}',
            Action::Run => '\u{E8A7}',
            Action::SignOut => '\u{E748}',
            Action::Sleep => '\u{E708}',
            Action::Shutdown => '\u{E7E8}',
            Action::Restart => '\u{E777}',
            Action::Desktop => '\u{E7F4}',
        }
    }

    fn item(self) -> Entry {
        Entry::Item {
            icon: Icon::Glyph(self.glyph()),
            label: self.label().into(),
            command: self.command(),
        }
    }

    /// Lleva a cabo la opción. "Buscar" no está aquí: abre el lanzador de GlassBar,
    /// que es de `main`.
    pub fn run(self) -> bool {
        match self {
            Action::InstalledApps => open("ms-settings:appsfeatures", None, false),
            Action::Mobility => open("mblctr.exe", None, false),
            Action::PowerOptions => open("ms-settings:powersleep", None, false),
            Action::EventViewer => open("eventvwr.msc", None, false),
            Action::System => open("ms-settings:about", None, false),
            Action::DeviceManager => open("devmgmt.msc", None, false),
            Action::Network => open("ms-settings:network", None, false),
            // Como en Windows, pide permisos de administrador.
            Action::DiskManagement => open("diskmgmt.msc", None, true),
            Action::ComputerManagement => open("compmgmt.msc", None, false),
            Action::Terminal => terminal(false),
            Action::TerminalAdmin => terminal(true),
            Action::TaskManager => open("taskmgr.exe", None, false),
            Action::Settings => open("ms-settings:", None, false),
            Action::Explorer => open("explorer.exe", None, false),
            Action::Search => false,
            Action::Run => explorer_command(TRAY_RUN),
            Action::SignOut => open("shutdown.exe", Some("/l"), false),
            Action::Sleep => crate::run_power(crate::launcher::Power::Sleep),
            Action::Shutdown => crate::run_power(crate::launcher::Power::Shutdown),
            Action::Restart => crate::run_power(crate::launcher::Power::Restart),
            Action::Desktop => explorer_command(TRAY_TOGGLE_DESKTOP),
        }
    }
}

/// Abre un programa, documento o enlace (`ms-settings:`…). `admin` pide elevarlo.
fn open(file: &str, args: Option<&str>, admin: bool) -> bool {
    let file = HSTRING::from(file);
    let args = args.map(HSTRING::from);
    unsafe {
        ShellExecuteW(
            None,
            if admin { w!("runas") } else { w!("open") },
            &file,
            args.as_ref()
                .map(|a| PCWSTR(a.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            None,
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
}

/// Windows Terminal y, si no está instalado, PowerShell.
fn terminal(admin: bool) -> bool {
    open("wt.exe", None, admin) || open("powershell.exe", None, admin)
}

/// Manda una orden a la barra de Explorer, que la atiende aunque esté oculta.
fn explorer_command(command: usize) -> bool {
    crate::shell::explorer_tray().is_some_and(|tray| unsafe {
        PostMessageW(Some(tray), WM_COMMAND, WPARAM(command), LPARAM(0)).is_ok()
    })
}

/// Si el equipo tiene Centro de movilidad (solo portátiles y tabletas).
fn has_mobility_center() -> bool {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .is_some_and(|root| root.join("System32").join("mblctr.exe").exists())
}

/// El menú, en el mismo orden que el de Windows.
pub fn menu() -> AppMenu {
    menu_with(has_mobility_center())
}

fn menu_with(mobility: bool) -> AppMenu {
    use Action::*;
    let mut entries = Vec::new();
    for action in [
        InstalledApps,
        Mobility,
        PowerOptions,
        EventViewer,
        System,
        DeviceManager,
        Network,
        DiskManagement,
        ComputerManagement,
        Terminal,
        TerminalAdmin,
    ] {
        if action != Mobility || mobility {
            entries.push(action.item());
        }
    }
    entries.push(Entry::Separator);
    for action in [TaskManager, Settings, Explorer, Search, Run] {
        entries.push(action.item());
    }
    entries.push(Entry::Separator);
    entries.push(Entry::Submenu {
        icon: Icon::Glyph(Shutdown.glyph()),
        label: "Apagar o cerrar sesión".into(),
        menu: Box::new(AppMenu::new(
            [SignOut, Sleep, Shutdown, Restart]
                .into_iter()
                .map(Action::item)
                .collect(),
        )),
    });
    entries.push(Desktop.item());
    AppMenu::new(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_go_and_come_back() {
        for action in Action::ALL {
            assert_eq!(Action::from_command(action.command()), Some(action));
        }
        assert_eq!(Action::from_command(COMMANDS - 1), None);
        assert_eq!(
            Action::from_command(COMMANDS + Action::ALL.len() as u32),
            None
        );
    }

    #[test]
    fn commands_do_not_clash_with_other_menus() {
        // Los estilos usan 200..300 y la jump list desde 100 en otro menú.
        let first = Action::ALL[0].command();
        let last = Action::ALL[Action::ALL.len() - 1].command();
        assert!(first >= 300 && last < 1000);
    }

    #[test]
    fn every_action_is_in_the_menu_once() {
        let menu = menu_with(true);
        let mut seen = Vec::new();
        let mut collect = |entries: &[Entry]| {
            for entry in entries {
                if let Entry::Item { command, .. } = entry {
                    seen.push(*command);
                }
            }
        };
        collect(&menu.entries);
        for entry in &menu.entries {
            if let Entry::Submenu { menu, .. } = entry {
                collect(&menu.entries);
            }
        }
        seen.sort();
        let mut all: Vec<u32> = Action::ALL.iter().map(|a| a.command()).collect();
        all.sort();
        assert_eq!(seen, all);
        // Sin Centro de movilidad, una opción menos.
        let without = menu_with(false);
        assert_eq!(without.entries.len(), menu.entries.len() - 1);
    }
}
