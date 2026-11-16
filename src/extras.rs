//! Resultados de la búsqueda del lanzador que no son programas: el resultado de una
//! operación, páginas de la Configuración de Windows, archivos y la búsqueda web.
//! Todo se calcula en el equipo; solo al elegir "Buscar en la web" se abre el
//! navegador con lo escrito.
use crate::apps::AppEntry;
use crate::catalog::search_key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Resultado de una operación; al elegirlo se copia.
    Calc,
    /// Página de la Configuración de Windows (`ms-settings:`).
    Setting,
    /// Archivo abierto hace poco o del escritorio, Documentos o Descargas.
    File,
    /// Lo escrito, en el buscador del navegador predeterminado.
    Web,
}

impl Kind {
    /// Título de su sección entre los resultados.
    pub fn header(self) -> &'static str {
        match self {
            Kind::Calc => "Calculadora",
            Kind::Setting => "Configuración",
            Kind::File => "Archivos",
            Kind::Web => "Web",
        }
    }
    /// Icono de Segoe Fluent Icons; los archivos llevan el suyo.
    pub fn glyph(self) -> Option<char> {
        match self {
            Kind::Calc => Some('\u{E8EF}'),
            Kind::Setting => Some('\u{E713}'),
            Kind::Web => Some('\u{E774}'),
            Kind::File => None,
        }
    }
}

/// Un resultado: `entry.name` es lo que se muestra y `entry.command` lo que se abre
/// (o, en la calculadora, lo que se copia).
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub kind: Kind,
    pub entry: AppEntry,
    /// Segunda línea: qué es.
    pub detail: String,
}

const MAX_SETTINGS: usize = 3;
const MAX_FILES: usize = 4;

/// Resultados para `query`, en el orden en que se muestran. `files` son los archivos
/// en los que se busca (ver `crate::catalog::searchable_files`).
pub fn search(query: &str, files: &[AppEntry]) -> Vec<Extra> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut list = Vec::new();
    if let Some(value) = calculate(query) {
        list.push(Extra {
            kind: Kind::Calc,
            entry: AppEntry {
                name: format!("= {value}"),
                command: value,
            },
            detail: "Enter copia el resultado".into(),
        });
    }
    let key = search_key(query);
    let words: Vec<&str> = key.split_whitespace().collect();
    let matches = |text: &str| {
        let text = search_key(text);
        words.iter().all(|word| text.contains(word))
    };
    // Con menos de dos letras coincidiría casi todo.
    if key.chars().count() >= 2 {
        list.extend(
            SETTINGS
                .iter()
                .filter(|(name, keywords, _)| matches(&format!("{name} {keywords}")))
                .take(MAX_SETTINGS)
                .map(|(name, _, page)| Extra {
                    kind: Kind::Setting,
                    entry: AppEntry {
                        name: (*name).into(),
                        command: format!("ms-settings:{page}"),
                    },
                    detail: "Configuración de Windows".into(),
                }),
        );
        list.extend(
            files
                .iter()
                .filter(|file| matches(&file.name))
                .take(MAX_FILES)
                .map(|file| Extra {
                    kind: Kind::File,
                    entry: file.clone(),
                    detail: "Archivo".into(),
                }),
        );
    }
    list.push(Extra {
        kind: Kind::Web,
        entry: AppEntry {
            name: format!("Buscar «{query}» en la web"),
            command: web_url(query),
        },
        detail: "Se abre en el navegador".into(),
    });
    list
}

/// Dirección de la búsqueda web de `query`. Sin espacios: `launch_entry` separaría
/// la orden por ellos.
pub fn web_url(query: &str) -> String {
    let mut url = String::from("https://www.bing.com/search?q=");
    for byte in query.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                url.push(byte as char)
            }
            b' ' => url.push('+'),
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// Páginas de Configuración: nombre, otras palabras con las que se encuentra y su
/// dirección `ms-settings:`.
const SETTINGS: &[(&str, &str, &str)] = &[
    (
        "Pantalla",
        "resolucion escala brillo monitor display",
        "display",
    ),
    ("Luz nocturna", "luz azul noche", "nightlight"),
    ("Sonido", "audio volumen altavoces microfono", "sound"),
    ("Notificaciones", "avisos no molestar", "notifications"),
    ("Concentración", "focus no molestar", "quiethours"),
    (
        "Inicio/apagado y batería",
        "energia suspension bateria",
        "powersleep",
    ),
    ("Almacenamiento", "disco espacio liberar", "storagesense"),
    ("Multitarea", "ventanas acoplar escritorios", "multitasking"),
    ("Portapapeles", "historial copiar pegar", "clipboard"),
    (
        "Acerca de",
        "sistema version equipo nombre especificaciones",
        "about",
    ),
    (
        "Bluetooth y dispositivos",
        "emparejar auriculares",
        "bluetooth",
    ),
    ("Impresoras y escáneres", "impresora imprimir", "printers"),
    ("Mouse", "raton puntero cursor", "mousetouchpad"),
    ("Panel táctil", "touchpad gestos", "devices-touchpad"),
    ("Escritura", "teclado autocorreccion", "typing"),
    ("Wi-Fi", "red inalambrica wifi", "network-wifi"),
    ("Ethernet", "red cable", "network-ethernet"),
    ("VPN", "red privada", "network-vpn"),
    ("Modo avión", "avion radios", "network-airplanemode"),
    (
        "Zona con cobertura inalámbrica móvil",
        "hotspot compartir internet",
        "network-mobilehotspot",
    ),
    ("Proxy", "red", "network-proxy"),
    ("Red e Internet", "conexion estado", "network-status"),
    (
        "Fondo",
        "fondo de pantalla wallpaper escritorio",
        "personalization-background",
    ),
    (
        "Colores",
        "tema oscuro claro enfasis",
        "personalization-colors",
    ),
    ("Temas", "apariencia", "themes"),
    ("Pantalla de bloqueo", "bloqueo", "lockscreen"),
    ("Barra de tareas", "taskbar", "taskbar"),
    ("Fuentes", "tipografia letras", "fonts"),
    (
        "Aplicaciones instaladas",
        "desinstalar programas apps",
        "appsfeatures",
    ),
    (
        "Aplicaciones predeterminadas",
        "abrir con navegador predeterminado",
        "defaultapps",
    ),
    (
        "Aplicaciones de inicio",
        "arranque inicio automatico",
        "startupapps",
    ),
    ("Tu información", "cuenta usuario perfil", "yourinfo"),
    (
        "Opciones de inicio de sesión",
        "contrasena pin huella hello",
        "signinoptions",
    ),
    ("Fecha y hora", "reloj zona horaria", "dateandtime"),
    (
        "Idioma y región",
        "idioma formato regional teclado",
        "regionlanguage",
    ),
    ("Modo de juego", "juegos game", "gaming-gamemode"),
    ("Accesibilidad", "narrador lupa contraste", "easeofaccess"),
    (
        "Tamaño del texto",
        "accesibilidad letra grande",
        "easeofaccess-display",
    ),
    ("Privacidad y seguridad", "permisos", "privacy"),
    ("Micrófono", "permisos privacidad", "privacy-microphone"),
    ("Cámara", "permisos privacidad webcam", "privacy-webcam"),
    (
        "Ubicación",
        "permisos privacidad localizacion",
        "privacy-location",
    ),
    (
        "Seguridad de Windows",
        "antivirus defender firewall",
        "windowsdefender",
    ),
    (
        "Windows Update",
        "actualizaciones actualizar",
        "windowsupdate",
    ),
    ("Recuperación", "restablecer reset", "recovery"),
    ("Solucionar problemas", "diagnostico", "troubleshoot"),
    (
        "Para programadores",
        "desarrolladores developer finalizar tarea",
        "developers",
    ),
    (
        "Volumen y dispositivos de aplicaciones",
        "mezclador audio",
        "apps-volume",
    ),
];

/// El resultado de `text` si es una operación aritmética (con algún operador), ya
/// como texto; `None` si no lo es o no se puede calcular. Admite `+ - * / ^ %`,
/// paréntesis, `x` y `×` para multiplicar y la coma decimal.
pub fn calculate(text: &str) -> Option<String> {
    let comma = text.contains(',');
    let tokens: Vec<char> = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            ',' => '.',
            'x' | 'X' | '×' | '·' => '*',
            '÷' | ':' => '/',
            '−' => '-',
            other => other,
        })
        .collect();
    let digits = tokens.iter().any(char::is_ascii_digit);
    // Un número solo ("2026") o un negativo ("-5") no son una operación.
    let operator = tokens
        .iter()
        .skip(1)
        .any(|c| matches!(c, '+' | '-' | '*' | '/' | '^' | '%' | '('));
    if !digits || !operator || tokens.len() > 200 {
        return None;
    }
    let mut parser = Parser {
        tokens: &tokens,
        at: 0,
        depth: 0,
    };
    let value = parser.sum()?;
    if parser.at != tokens.len() || !value.is_finite() {
        return None;
    }
    Some(format_number(value, comma))
}

/// Hasta diez decimales, sin ceros de más; con coma si la operación la usaba.
fn format_number(value: f64, comma: bool) -> String {
    let rounded = (value * 1e10).round() / 1e10;
    let mut text = if rounded.abs() >= 1e15 {
        format!("{rounded:e}")
    } else {
        format!("{rounded:.10}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };
    if text == "-0" {
        text = "0".into();
    }
    if comma {
        text = text.replace('.', ",");
    }
    text
}

/// Analizador por descenso: suma → producto → potencia → signo → número o paréntesis.
struct Parser<'a> {
    tokens: &'a [char],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.tokens.get(self.at).copied()
    }
    fn take(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.at += 1;
        }
        found
    }
    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.take('+') {
                value += self.product()?;
            } else if self.take('-') {
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }
    fn product(&mut self) -> Option<f64> {
        let mut value = self.signed()?;
        loop {
            if self.take('*') {
                value *= self.signed()?;
            } else if self.take('/') {
                let divisor = self.signed()?;
                if divisor == 0.0 {
                    return None;
                }
                value /= divisor;
            } else {
                return Some(value);
            }
        }
    }
    fn signed(&mut self) -> Option<f64> {
        if self.take('-') {
            return Some(-self.signed()?);
        }
        if self.take('+') {
            return self.signed();
        }
        self.power()
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.take('^') {
            // A la derecha: 2^3^2 es 2^(3^2), y admite exponente negativo.
            return Some(base.powf(self.signed()?));
        }
        Some(base)
    }
    fn atom(&mut self) -> Option<f64> {
        let mut value = if self.take('(') {
            self.depth += 1;
            if self.depth > 32 {
                return None;
            }
            let inner = self.sum()?;
            if !self.take(')') {
                return None;
            }
            self.depth -= 1;
            inner
        } else {
            let start = self.at;
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                self.at += 1;
            }
            let text: String = self.tokens[start..self.at].iter().collect();
            text.parse::<f64>().ok()?
        };
        // Tanto por ciento: "50%" es 0,5.
        while self.take('%') {
            value /= 100.0;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_only_what_is_an_operation() {
        let result = |text: &str| calculate(text).unwrap_or_default();
        assert_eq!(result("2+2"), "4");
        assert_eq!(result("2 + 3 * 4"), "14");
        assert_eq!(result("(2 + 3) * 4"), "20");
        assert_eq!(result("10 / 4"), "2.5");
        assert_eq!(result("2^10"), "1024");
        assert_eq!(result("2^3^2"), "512");
        assert_eq!(result("-3 + 5"), "2");
        assert_eq!(result("3 x 4"), "12");
        assert_eq!(result("200 * 15%"), "30");
        assert_eq!(result("0.1 + 0.2"), "0.3");
        // Con coma decimal, el resultado también la lleva.
        assert_eq!(result("1,5 + 1"), "2,5");
        // Ni un número solo, ni un nombre, ni una operación a medias o imposible.
        for text in [
            "2026", "-5", "chrome", "7zip", "2 +", "(2 + 3", "1 / 0", "a + b", "",
        ] {
            assert_eq!(calculate(text), None, "{text}");
        }
    }

    #[test]
    fn search_adds_settings_files_and_the_web_after_the_result() {
        let files = [
            AppEntry {
                name: "Informe ventas.xlsx".into(),
                command: r"C:\Users\a\Recent\Informe ventas.xlsx.lnk".into(),
            },
            AppEntry {
                name: "notas.txt".into(),
                command: r"C:\Users\a\Desktop\notas.txt".into(),
            },
        ];
        assert!(search("  ", &files).is_empty());

        let kinds = |query: &str| -> Vec<Kind> {
            search(query, &files).into_iter().map(|e| e.kind).collect()
        };
        // Una operación: su resultado y, al final, la web.
        let math = search("12*12", &files);
        assert_eq!(math[0].entry.name, "= 144");
        assert_eq!(math[0].entry.command, "144");
        assert_eq!(kinds("12*12"), [Kind::Calc, Kind::Web]);

        // Configuración por nombre o por otra palabra, sin tildes ni mayúsculas.
        let found = search("BATERIA", &files);
        assert_eq!(found[0].kind, Kind::Setting);
        assert_eq!(found[0].entry.command, "ms-settings:powersleep");
        assert!(search("raton", &files)
            .iter()
            .any(|e| e.entry.command == "ms-settings:mousetouchpad"));
        assert!(
            kinds("pantalla")
                .iter()
                .filter(|k| **k == Kind::Setting)
                .count()
                <= MAX_SETTINGS
        );

        // Archivos por palabras en cualquier orden.
        let file = search("ventas informe", &files);
        assert_eq!(file[0].kind, Kind::File);
        assert_eq!(file[0].entry, files[0]);

        // Una sola letra solo ofrece la web.
        assert_eq!(kinds("n"), [Kind::Web]);
        let web = search("rust lang ñ", &files);
        assert_eq!(
            web.last().unwrap().entry.command,
            "https://www.bing.com/search?q=rust+lang+%C3%B1"
        );
        assert!(!web.last().unwrap().entry.command.contains(' '));
    }

    #[test]
    fn settings_pages_are_unique_and_have_no_spaces() {
        for (index, (name, _, page)) in SETTINGS.iter().enumerate() {
            assert!(!page.contains(' '), "{name}");
            assert!(
                SETTINGS[..index].iter().all(|(_, _, other)| other != page),
                "{name}"
            );
        }
    }
}
