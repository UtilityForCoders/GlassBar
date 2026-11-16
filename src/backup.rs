//! Copia de la configuración de GlassBar en un solo archivo, para llevarla a otro
//! equipo o guardarla antes de reinstalar: los programas anclados, los estilos, el
//! orden de la bandeja y la vista del lanzador. El historial de apps abiertas y el
//! estado de las actualizaciones no se copian.
use std::path::{Path, PathBuf};

/// Archivos de `%LOCALAPPDATA%\GlassBar` que forman la copia. Al importar solo se
/// escriben estos: el nombre nunca sale del archivo importado tal cual.
const FILES: [&str; 4] = [
    "apps.json",
    "styles.json",
    "tray-order.json",
    "start-view.txt",
];

/// Nombre que se propone al exportar.
pub const FILE_NAME: &str = "glassbar-config.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct Bundle {
    /// Versión de GlassBar que hizo la copia.
    glassbar: String,
    /// Contenido de cada archivo, por su nombre.
    files: std::collections::BTreeMap<String, String>,
}

fn folder() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("GlassBar"))
}

/// La copia, como texto, con lo que `read` devuelve de cada archivo (los que
/// faltan no entran).
fn bundle(read: impl Fn(&str) -> Option<String>) -> String {
    let files = FILES
        .iter()
        .filter_map(|name| Some((name.to_string(), read(name)?)))
        .collect();
    let bundle = Bundle {
        glassbar: env!("CARGO_PKG_VERSION").into(),
        files,
    };
    serde_json::to_string_pretty(&bundle).unwrap_or_default()
}

/// Los archivos de una copia: nombre y contenido. Rechaza lo que no sea una copia
/// de GlassBar, los nombres que no son suyos y los JSON mal formados, para no
/// dejar la configuración a medias.
fn unbundle(text: &str) -> Result<Vec<(&'static str, String)>, String> {
    let bundle: Bundle = serde_json::from_str(text)
        .map_err(|_| "el archivo no es una copia de la configuración de GlassBar".to_string())?;
    let mut files = Vec::new();
    for (name, content) in bundle.files {
        let Some(known) = FILES.iter().find(|known| **known == name) else {
            return Err(format!("la copia trae un archivo desconocido ({name})"));
        };
        if known.ends_with(".json") && serde_json::from_str::<serde_json::Value>(&content).is_err()
        {
            return Err(format!("{name} está dañado en la copia"));
        }
        files.push((*known, content));
    }
    if files.is_empty() {
        return Err("la copia está vacía".into());
    }
    Ok(files)
}

/// Guarda la configuración actual en `path`.
pub fn export(path: &Path) -> Result<(), String> {
    let folder = folder().ok_or("LOCALAPPDATA no disponible")?;
    let text = bundle(|name| std::fs::read_to_string(folder.join(name)).ok());
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Sustituye la configuración por la de la copia `path`. GlassBar la lee al
/// arrancar, así que hay que reiniciarlo después.
pub fn import(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let files = unbundle(&text)?;
    let folder = folder().ok_or("LOCALAPPDATA no disponible")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for (name, content) in files {
        // Primero a un temporal: si falla a medias, el archivo anterior sigue entero.
        let pending = folder.join(format!("{name}.tmp"));
        std::fs::write(&pending, content).map_err(|e| e.to_string())?;
        std::fs::rename(&pending, folder.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Diálogo de Windows para elegir dónde guardar la copia (`save`) o cuál abrir.
/// `None` si se cancela.
pub fn pick(owner: windows::Win32::Foundation::HWND, save: bool) -> Option<PathBuf> {
    use windows::core::{w, Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
        SIGDN_FILESYSPATH,
    };
    unsafe {
        let dialog: IFileDialog = if save {
            CoCreateInstance::<_, IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                .ok()?
                .cast()
                .ok()?
        } else {
            CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .ok()?
                .cast()
                .ok()?
        };
        let filter = [COMDLG_FILTERSPEC {
            pszName: w!("Configuración de GlassBar (*.json)"),
            pszSpec: w!("*.json"),
        }];
        let _ = dialog.SetFileTypes(&filter);
        let _ = dialog.SetDefaultExtension(w!("json"));
        if save {
            let _ = dialog.SetFileName(&HSTRING::from(FILE_NAME));
        }
        dialog.Show(Some(owner)).ok()?;
        let raw = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        path.map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_round_trips_and_rejects_what_is_not_one() {
        let text = bundle(|name| match name {
            "apps.json" => Some(r#"[{"name":"Notas","command":"notepad.exe"}]"#.into()),
            "start-view.txt" => Some("grid".into()),
            _ => None,
        });
        let files = unbundle(&text).unwrap();
        assert_eq!(
            files,
            [
                (
                    "apps.json",
                    r#"[{"name":"Notas","command":"notepad.exe"}]"#.to_string()
                ),
                ("start-view.txt", "grid".to_string()),
            ]
        );

        let with = |name: &str, content: &str| {
            serde_json::json!({ "glassbar": "0.7.1", "files": { name: content } }).to_string()
        };
        // Ni otro JSON, ni nombres de fuera (rutas), ni un JSON roto, ni una copia vacía.
        assert!(unbundle("{}").is_err());
        assert!(unbundle("no es json").is_err());
        assert!(unbundle(&with(r"..\..\evil.json", "{}")).is_err());
        assert!(unbundle(&with("recent.json", "[]")).is_err());
        assert!(unbundle(&with("styles.json", "{roto")).is_err());
        assert!(unbundle(r#"{"glassbar":"0.7.1","files":{}}"#).is_err());
        assert!(unbundle(&with("styles.json", "{}")).is_ok());
    }
}
