//! Catálogo local del menú Inicio y recientes de GlassBar, sin telemetría.
use crate::apps::AppEntry;
use std::path::{Path, PathBuf};

pub fn installed(pinned: &[AppEntry]) -> Vec<AppEntry> {
    let mut result = windows_apps();
    result.extend_from_slice(pinned);
    for variable in ["APPDATA", "PROGRAMDATA"] {
        if let Some(root) = std::env::var_os(variable) {
            scan(
                &PathBuf::from(root)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
                &mut result,
                0,
            );
        }
    }
    normalize(&mut result);
    result
}
/// AppsFolder incluye aplicaciones empaquetadas que no tienen archivo .lnk.
pub fn windows_apps() -> Vec<AppEntry> {
    use windows::core::{Interface, PWSTR};
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;
    unsafe fn take(value: PWSTR) -> String {
        let text = value.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(value.0.cast()));
        text
    }
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> windows::core::Result<Vec<AppEntry>> {
            let folder: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
            let mut result = Vec::new();
            loop {
                let mut batch = [None];
                let mut fetched = 0;
                items.Next(&mut batch, Some(&mut fetched))?;
                if fetched == 0 {
                    break;
                }
                if let Some(item) = batch[0].take() {
                    let Ok(display) = item.GetDisplayName(SIGDN_NORMALDISPLAY) else {
                        continue;
                    };
                    let name = take(display);
                    let Ok(item2) = item.cast::<IShellItem2>() else {
                        continue;
                    };
                    if let Ok(id) = item2.GetString(&PKEY_AppUserModel_ID) {
                        let id = take(id);
                        if !name.is_empty() && !id.is_empty() {
                            result.push(AppEntry {
                                name,
                                command: format!("shell:AppsFolder\\{id}"),
                            });
                        }
                    }
                }
            }
            Ok(result)
        })()
        .unwrap_or_default();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

pub fn search_key(text: &str) -> String {
    use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Coincidencias completas primero; admite palabras en cualquier orden y tildes.
pub fn search(apps: &[AppEntry], query: &str) -> Vec<usize> {
    let query = search_key(query.trim());
    let words: Vec<_> = query.split_whitespace().collect();
    let mut hits: Vec<_> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let name = search_key(&entry.name);
            if words.iter().all(|word| name.contains(word)) {
                let rank = if name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else {
                    2
                };
                Some((rank, name, i))
            } else {
                None
            }
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, _, i)| i).collect()
}

fn scan(folder: &Path, out: &mut Vec<AppEntry>, depth: usize) {
    if depth > 12 {
        return;
    }
    let Ok(files) = std::fs::read_dir(folder) else {
        return;
    };
    for item in files.flatten() {
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = item.path();
        if kind.is_dir() {
            scan(&path, out, depth + 1);
        } else if path.extension().is_some_and(|e| {
            e.eq_ignore_ascii_case("lnk")
                || e.eq_ignore_ascii_case("url")
                || e.eq_ignore_ascii_case("exe")
        }) {
            out.push(AppEntry {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                command: path.to_string_lossy().into_owned(),
            });
        }
    }
}
fn normalize(entries: &mut Vec<AppEntry>) {
    entries.sort_by_cached_key(|e| (e.name.to_lowercase(), e.command.to_lowercase()));
    let shell_names: std::collections::HashSet<_> = entries
        .iter()
        .filter(|e| e.command.starts_with("shell:AppsFolder\\"))
        .map(|e| search_key(&e.name))
        .collect();
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| {
        (e.command.starts_with("shell:AppsFolder\\") || !shell_names.contains(&search_key(&e.name)))
            && seen.insert(e.command.to_lowercase())
    });
}
fn history_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("recent.json"),
    )
}
pub fn recent() -> Vec<AppEntry> {
    history_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| serde_json::from_slice::<Vec<AppEntry>>(&data).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.command.starts_with("shell:AppsFolder\\") || Path::new(&e.command).is_file())
        .take(8)
        .collect()
}
pub fn record(entry: &AppEntry) -> std::io::Result<()> {
    let mut entries = recent();
    promote(&mut entries, entry);
    let path = history_path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA no disponible"))?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let pending = path.with_extension("tmp");
    std::fs::write(&pending, serde_json::to_vec_pretty(&entries)?)?;
    std::fs::rename(pending, path)
}
fn promote(entries: &mut Vec<AppEntry>, entry: &AppEntry) {
    entries.retain(|e| !e.command.eq_ignore_ascii_case(&entry.command));
    entries.insert(0, entry.clone());
    entries.truncate(8);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_handles_accents_word_order_and_ranks_exact_names() {
        let apps = vec![
            AppEntry {
                name: "Spotify Tools".into(),
                command: "tools.exe".into(),
            },
            AppEntry {
                name: "Spotify".into(),
                command: "shell:AppsFolder\\Store!Spotify".into(),
            },
            AppEntry {
                name: "Diseño de Audio".into(),
                command: "design.exe".into(),
            },
        ];
        assert_eq!(search(&apps, " SPOTIFY "), vec![1, 0]);
        assert_eq!(search(&apps, "audio diseno"), vec![2]);
        assert!(search(&apps, "unknown app").is_empty());
    }
    #[test]
    fn history_promotes_deduplicates_and_limits() {
        let mut entries = Vec::new();
        for i in 0..12 {
            promote(
                &mut entries,
                &AppEntry {
                    name: format!("App {i}"),
                    command: format!("{i}.exe"),
                },
            );
        }
        let oldest = entries.last().unwrap().clone();
        promote(&mut entries, &oldest);
        assert_eq!(entries.len(), 8);
        assert_eq!(entries[0].command, oldest.command);
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.command == oldest.command)
                .count(),
            1
        );
    }
    #[test]
    fn catalog_handles_unicode_and_duplicate_paths() {
        let mut entries = vec![
            AppEntry {
                name: "Diseño".into(),
                command: "C:\\App.exe".into(),
            },
            AppEntry {
                name: "Diseño".into(),
                command: "c:\\app.EXE".into(),
            },
        ];
        normalize(&mut entries);
        assert_eq!(entries.len(), 1);
    }
}
