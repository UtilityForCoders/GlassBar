//! Lista local de programas; no se descargan ni recrean logos.
use crate::apps::{AppEntry, AppItem};
use std::path::PathBuf;

pub fn shortcut_target(path: &str) -> Option<String> {
    if !std::path::Path::new(path)
        .extension()?
        .eq_ignore_ascii_case("lnk")
    {
        return None;
    }
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let file: IPersistFile = link.cast().ok()?;
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
            let mut target = vec![0u16; 32768];
            link.GetPath(&mut target, std::ptr::null_mut(), 0).ok()?;
            let len = target.iter().position(|&c| c == 0)?;
            if len == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&target[..len]))
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// Respeta el recurso de icono personalizado del acceso directo.
pub fn shortcut_icon_source(path: &str) -> Option<(String, i32)> {
    if !std::path::Path::new(path)
        .extension()?
        .eq_ignore_ascii_case("lnk")
    {
        return None;
    }
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let file: IPersistFile = link.cast().ok()?;
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
            let mut resource = vec![0u16; 32768];
            let mut index = 0;
            link.GetIconLocation(&mut resource, &mut index).ok()?;
            let len = resource.iter().position(|&c| c == 0)?;
            if len > 0 {
                Some((String::from_utf16_lossy(&resource[..len]), index))
            } else {
                link.GetPath(&mut resource, std::ptr::null_mut(), 0).ok()?;
                let len = resource.iter().position(|&c| c == 0)?;
                if len == 0 {
                    None
                } else {
                    Some((String::from_utf16_lossy(&resource[..len]), 0))
                }
            }
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

fn config_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("apps.json"),
    )
}

pub fn load() -> Option<Vec<AppItem>> {
    let contents = std::fs::read(config_path()?).ok()?;
    let entries: Vec<AppEntry> = serde_json::from_slice(&contents).ok()?;
    Some(entries.into_iter().map(AppItem::new).collect())
}

pub fn save(apps: &[AppItem]) -> std::io::Result<()> {
    let path = config_path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA no disponible"))?;
    save_to(apps, &path)
}

fn save_to(apps: &[AppItem], path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let entries: Vec<_> = apps.iter().map(|a| &a.entry).collect();
    let pending = path.with_extension("tmp");
    std::fs::write(&pending, serde_json::to_vec_pretty(&entries)?)?;
    std::fs::rename(pending, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_accept_programs_and_reject_documents_or_missing_files() {
        let root = std::env::var("WINDIR").unwrap();
        assert!(from_drop(format!("{root}\\explorer.exe")).is_some());
        assert!(from_drop(format!("{root}\\win.ini")).is_none());
        assert!(from_drop(format!("{root}\\missing-glassbar-test.exe")).is_none());
    }
    #[test]
    fn saves_unicode_paths_and_can_replace_with_empty_list() {
        let path =
            std::env::temp_dir().join(format!("glassbar-pins-test-{}.json", std::process::id()));
        let apps = vec![AppItem::new(AppEntry {
            name: "Mi aplicación".into(),
            command: "C:\\Programas\\Diseño 工具\\app.exe".into(),
        })];
        save_to(&apps, &path).unwrap();
        let entries: Vec<AppEntry> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(entries[0].command, apps[0].entry.command);
        save_to(&[], &path).unwrap();
        let entries: Vec<AppEntry> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(entries.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}

/// Acepta solo programas y accesos directos existentes.
pub fn from_drop(path: String) -> Option<AppItem> {
    let file = std::path::Path::new(&path);
    if !file.is_file()
        || !file.extension().is_some_and(|e| {
            ["exe", "lnk", "url"]
                .iter()
                .any(|allowed| e.eq_ignore_ascii_case(allowed))
        })
    {
        return None;
    }
    let name = file.file_stem()?.to_string_lossy().into_owned();
    Some(AppItem::new(AppEntry {
        name,
        command: path,
    }))
}
