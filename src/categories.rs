//! Categorías de la sección "Todo" del lanzador, como la vista por categorías del
//! Inicio de Windows 11. Windows no expone la categoría de cada app, así que se
//! deduce por palabras del nombre y del programa.
use crate::apps::AppEntry;
use crate::catalog::search_key;

/// Una categoría necesita al menos tantas apps; si no, las suyas van a "Otros",
/// como hace Windows.
const MIN_APPS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Utilities,
    Productivity,
    Social,
    Creativity,
    Entertainment,
    Games,
    Developer,
    Reading,
    Other,
}

impl Kind {
    const ALL: [Kind; 9] = [
        Kind::Utilities,
        Kind::Productivity,
        Kind::Social,
        Kind::Creativity,
        Kind::Entertainment,
        Kind::Games,
        Kind::Developer,
        Kind::Reading,
        Kind::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Kind::Utilities => "Utilidades y herramientas",
            Kind::Productivity => "Productividad",
            Kind::Social => "Sociales",
            Kind::Creativity => "Creatividad",
            Kind::Entertainment => "Entretenimiento",
            Kind::Games => "Juegos",
            Kind::Developer => "Herramientas de desarrollo",
            Kind::Reading => "Información y lectura",
            Kind::Other => "Otros",
        }
    }
    /// Palabras (o frases) que la delatan, sin tildes y en minúscula. Se comparan
    /// con palabras enteras del nombre y del programa.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Kind::Developer => &[
                "visual studio",
                "vs code",
                "vscode",
                "code",
                "cursor",
                "windsurf",
                "terminal",
                "powershell",
                "simbolo del sistema",
                "command prompt",
                "cmd",
                "git",
                "github",
                "gitkraken",
                "sourcetree",
                "docker",
                "python",
                "idle",
                "node",
                "nodejs",
                "arduino",
                "android studio",
                "intellij",
                "pycharm",
                "webstorm",
                "phpstorm",
                "rider",
                "clion",
                "goland",
                "datagrip",
                "jetbrains",
                "postman",
                "insomnia",
                "warp",
                "sublime",
                "notepad++",
                "wsl",
                "ubuntu",
                "debian",
                "putty",
                "winscp",
                "dbeaver",
                "mysql",
                "postgresql",
                "pgadmin",
                "mongodb",
                "xampp",
                "unity",
                "unreal",
                "godot",
                "eclipse",
                "netbeans",
                "claude",
                "rust",
                "java",
            ],
            Kind::Social => &[
                "whatsapp",
                "telegram",
                "linkedin",
                "discord",
                "messenger",
                "instagram",
                "facebook",
                "twitter",
                "signal",
                "skype",
                "wechat",
                "line",
                "viber",
                "threads",
                "reddit",
                "tiktok",
                "pinterest",
                "mastodon",
                "bluesky",
            ],
            Kind::Games => &[
                "steam",
                "minecraft",
                "roblox",
                "battle net",
                "ea",
                "ea app",
                "origin",
                "ubisoft",
                "uplay",
                "gog",
                "riot",
                "league of legends",
                "valorant",
                "fortnite",
                "rockstar",
                "solitaire",
                "solitario",
                "buscaminas",
                "minesweeper",
                "candy crush",
                "game",
                "juego",
                "juegos",
            ],
            Kind::Entertainment => &[
                "spotify",
                "netflix",
                "vlc",
                "reproductor multimedia",
                "media player",
                "reproductor",
                "epic games",
                "xbox",
                "prime video",
                "disney",
                "youtube",
                "twitch",
                "peliculas",
                "movies",
                "itunes",
                "apple music",
                "apple tv",
                "groove",
                "deezer",
                "tidal",
                "plex",
                "kodi",
                "crunchyroll",
                "hbo",
                "max",
                "music",
                "musica",
                "podcast",
            ],
            Kind::Creativity => &[
                "fotos",
                "photos",
                "camara",
                "camera",
                "paint",
                "paint 3d",
                "clipchamp",
                "photoshop",
                "illustrator",
                "premiere",
                "after effects",
                "lightroom",
                "indesign",
                "creative cloud",
                "gimp",
                "inkscape",
                "blender",
                "figma",
                "canva",
                "obs",
                "obs studio",
                "davinci",
                "resolve",
                "capcut",
                "recortes",
                "snipping",
                "grabadora",
                "sound recorder",
                "krita",
                "audacity",
                "fl studio",
                "ableton",
                "sketchup",
                "autocad",
                "fusion 360",
                "shotcut",
                "kdenlive",
                "handbrake",
                "affinity",
                "designer",
                "editor de video",
                "video editor",
                "sharex",
                "greenshot",
            ],
            Kind::Productivity => &[
                "chrome",
                "google chrome",
                "edge",
                "microsoft edge",
                "firefox",
                "opera",
                "brave",
                "vivaldi",
                "arc",
                "zoom",
                "teams",
                "outlook",
                "word",
                "excel",
                "powerpoint",
                "onenote",
                "access",
                "publisher",
                "office",
                "microsoft 365",
                "notion",
                "slack",
                "explorador de archivos",
                "file explorer",
                "bloc de notas",
                "notepad",
                "to do",
                "todo",
                "calendario",
                "calendar",
                "correo",
                "mail",
                "thunderbird",
                "acrobat",
                "pdf",
                "drive",
                "google drive",
                "dropbox",
                "onedrive",
                "trello",
                "obsidian",
                "evernote",
                "libreoffice",
                "wps",
                "copilot",
                "chatgpt",
                "1password",
                "bitwarden",
                "keepass",
                "anydesk",
                "teamviewer",
                "webex",
                "meet",
            ],
            Kind::Reading => &[
                "tiempo",
                "weather",
                "clima",
                "noticias",
                "news",
                "kindle",
                "mapas",
                "maps",
                "reader",
                "lector",
                "calibre",
                "wikipedia",
                "feedly",
                "sumatra",
                "sumatrapdf",
                "bolsa",
                "money",
            ],
            Kind::Utilities => &[
                "configuracion",
                "settings",
                "calculadora",
                "calculator",
                "panel de control",
                "control panel",
                "administrador de tareas",
                "task manager",
                "ccleaner",
                "herramientas",
                "tools",
                "reloj",
                "clock",
                "alarmas",
                "alarms",
                "visor",
                "viewer",
                "monitor",
                "teclado",
                "keyboard",
                "accesibilidad",
                "narrador",
                "narrator",
                "lupa",
                "magnifier",
                "copia de seguridad",
                "backup",
                "powertoys",
                "everything",
                "vpn",
                "nvidia",
                "geforce",
                "amd",
                "radeon",
                "intel",
                "realtek",
                "logitech",
                "razer",
                "corsair",
                "driver",
                "impresora",
                "printer",
                "escaner",
                "scanner",
                "bluetooth",
                "informacion del sistema",
                "system information",
                "editor del registro",
                "registry editor",
                "desfragmentar",
                "liberador de espacio",
                "disk cleanup",
                "recovery",
                "recuperacion",
                "seguridad de windows",
                "windows security",
                "antivirus",
                "malwarebytes",
                "avast",
                "kaspersky",
                "eset",
                "rufus",
                "ventoy",
                "cpu z",
                "hwmonitor",
                "crystaldiskinfo",
                "msi afterburner",
                "obtener ayuda",
                "get help",
                "sugerencias",
                "tips",
                "centro de opiniones",
                "feedback hub",
                "vinculo movil",
                "phone link",
                "quick assist",
                "asistencia rapida",
                "mapa de caracteres",
                "character map",
                "escritorio remoto",
                "remote desktop",
                "uninstall",
                "desinstalar",
                "update",
                "updater",
                "filezilla",
                "7 zip",
                "7zip",
                "winrar",
                "peazip",
                "bandizip",
            ],
            Kind::Other => &[],
        }
    }
}

/// Categoría con sus apps (índices del catálogo), en el orden en que se muestran.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub kind: Kind,
    pub apps: Vec<usize>,
}

/// Texto donde se buscan las palabras: el nombre y el programa (el archivo sin
/// extensión o, en las apps de la Store, el nombre de su paquete), con espacios
/// alrededor de cada palabra.
fn haystack(entry: &AppEntry) -> String {
    let command = entry
        .command
        .strip_prefix("shell:AppsFolder\\")
        .map(|id| id.split(['_', '!']).next().unwrap_or(id))
        .unwrap_or_else(|| {
            let file = entry.command.rsplit(['\\', '/']).next().unwrap_or("");
            file.rsplit_once('.').map_or(file, |(stem, _)| stem)
        });
    let text = search_key(&format!("{} {}", entry.name, command));
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '+'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    format!(" {} ", words.join(" "))
}

/// Categoría de una app: la primera cuya palabra aparece. Las de desarrollo se
/// miran antes para que "Visual Studio Code" no acabe en otra.
pub fn classify(entry: &AppEntry) -> Kind {
    let text = haystack(entry);
    const ORDER: [Kind; 8] = [
        Kind::Developer,
        Kind::Social,
        Kind::Games,
        Kind::Entertainment,
        Kind::Creativity,
        Kind::Reading,
        Kind::Productivity,
        Kind::Utilities,
    ];
    ORDER
        .into_iter()
        .find(|kind| {
            kind.keywords()
                .iter()
                .any(|word| text.contains(&format!(" {word} ")))
        })
        .unwrap_or(Kind::Other)
}

/// Agrupa el catálogo. Dentro de cada categoría van primero las más usadas
/// (`score` mayor) y luego por nombre. Las categorías con pocas apps se juntan en
/// "Otros", que va al final.
pub fn group(apps: &[AppEntry], score: impl Fn(&AppEntry) -> u64) -> Vec<Category> {
    let mut groups: Vec<Category> = Kind::ALL
        .into_iter()
        .map(|kind| Category {
            kind,
            apps: Vec::new(),
        })
        .collect();
    for (index, entry) in apps.iter().enumerate() {
        let kind = classify(entry);
        groups[Kind::ALL.iter().position(|k| *k == kind).unwrap()]
            .apps
            .push(index);
    }
    let mut others = Vec::new();
    for group in &mut groups {
        if group.kind != Kind::Other && group.apps.len() < MIN_APPS {
            others.append(&mut group.apps);
        }
    }
    if let Some(other) = groups.iter_mut().find(|g| g.kind == Kind::Other) {
        other.apps.extend(others);
    }
    groups.retain(|g| !g.apps.is_empty());
    for group in &mut groups {
        group.apps.sort_by_cached_key(|&i| {
            (
                std::cmp::Reverse(score(&apps[i])),
                search_key(&apps[i].name),
            )
        });
    }
    groups
}

/// Índices del catálogo en orden alfabético, para las vistas de cuadrícula y lista.
pub fn alphabetical(apps: &[AppEntry]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..apps.len()).collect();
    order.sort_by_cached_key(|&i| search_key(&apps[i].name));
    order
}

/// Letra bajo la que va una app en la vista de lista: su inicial, o "#" si no
/// empieza por letra.
pub fn initial(name: &str) -> String {
    match search_key(name).chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
        _ => "#".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str, command: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            command: command.into(),
        }
    }

    #[test]
    fn known_apps_land_in_their_category() {
        let cases = [
            (
                "Visual Studio Code",
                "C:\\VS Code\\Code.exe",
                Kind::Developer,
            ),
            (
                "Terminal",
                "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
                Kind::Developer,
            ),
            (
                "WhatsApp",
                "shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App",
                Kind::Social,
            ),
            (
                "Telegram Desktop",
                "C:\\Telegram\\Telegram.exe",
                Kind::Social,
            ),
            ("Spotify", "C:\\Spotify\\Spotify.exe", Kind::Entertainment),
            ("Steam", "C:\\Steam\\steam.exe", Kind::Games),
            (
                "Google Chrome",
                "C:\\Chrome\\chrome.exe",
                Kind::Productivity,
            ),
            (
                "Fotos",
                "shell:AppsFolder\\Microsoft.Windows.Photos_8wekyb3d8bbwe!App",
                Kind::Creativity,
            ),
            (
                "Calculadora",
                "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                Kind::Utilities,
            ),
            (
                "Configuración",
                "shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!App",
                Kind::Utilities,
            ),
            (
                "El Tiempo",
                "shell:AppsFolder\\Microsoft.BingWeather_8wekyb3d8bbwe!App",
                Kind::Reading,
            ),
            ("Mi app rara", "C:\\Rara\\rara.exe", Kind::Other),
        ];
        for (name, command, kind) in cases {
            assert_eq!(classify(&entry(name, command)), kind, "{name}");
        }
    }

    #[test]
    fn words_match_whole_so_short_keywords_do_not_leak() {
        // "code" no debe atrapar "Decode" ni "ea" a "Earth".
        assert_eq!(
            classify(&entry("Decoder", "C:\\d\\decoder.exe")),
            Kind::Other
        );
        assert_eq!(
            classify(&entry("Google Earth", "C:\\g\\earth.exe")),
            Kind::Other
        );
    }

    #[test]
    fn small_categories_merge_into_others_and_order_follows_usage() {
        let apps = vec![
            entry("Spotify", "spotify.exe"),
            entry("Discord", "discord.exe"),
            entry("WhatsApp", "whatsapp.exe"),
            entry("Telegram", "telegram.exe"),
            entry("Rara", "rara.exe"),
        ];
        let groups = group(&apps, |e| if e.name == "Telegram" { 9 } else { 0 });
        let kinds: Vec<_> = groups.iter().map(|g| g.kind).collect();
        assert_eq!(kinds, [Kind::Social, Kind::Other]);
        // Telegram, la más usada, primero; las demás por nombre.
        assert_eq!(groups[0].apps, [3, 1, 2]);
        // Spotify (sola en Entretenimiento) pasa a Otros.
        assert_eq!(groups[1].apps, [4, 0]);
    }

    #[test]
    fn list_initials_and_alphabetical_order() {
        assert_eq!(initial("Órgano"), "O");
        assert_eq!(initial("7-Zip"), "#");
        let apps = vec![entry("b", "b"), entry("Á", "a"), entry("c", "c")];
        assert_eq!(alphabetical(&apps), [1, 0, 2]);
    }
}
