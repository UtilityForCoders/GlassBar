//! Juegos que Windows ya reconoce: la barra de juegos de Xbox guarda cada juego
//! que detecta en `HKCU\System\GameConfigStore\Children`, con la ruta de su
//! ejecutable. Con esa lista el dock y la barra superior se apartan en cualquier
//! juego a pantalla completa, aunque no ocupe el monitor como esperan las demás
//! comprobaciones (p. ej. un juego "sin bordes" encajado en el área de trabajo).
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// Cada cuánto se vuelve a leer la lista: Windows añade un juego la primera vez
/// que se abre.
const REFRESH: Duration = Duration::from_secs(120);

struct Cache {
    read: Instant,
    games: HashSet<String>,
    /// Último proceso preguntado y la respuesta: la ventana activa casi nunca
    /// cambia entre dos comprobaciones.
    last: Option<(u32, bool)>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// El proceso `pid` es un juego que Windows reconoce.
pub fn is_game(pid: u32) -> bool {
    let Ok(mut cache) = CACHE.lock() else {
        return false;
    };
    let stale = cache.as_ref().is_none_or(|c| c.read.elapsed() >= REFRESH);
    if stale {
        *cache = Some(Cache {
            read: Instant::now(),
            games: read_games(),
            last: None,
        });
    }
    let cache = cache.as_mut().expect("lista leída");
    if let Some((last, answer)) = cache.last {
        if last == pid {
            return answer;
        }
    }
    let answer = crate::apps::process_info(pid)
        .is_some_and(|(exe, _)| cache.games.contains(&normalize(&exe)));
    cache.last = Some((pid, answer));
    answer
}

/// Las rutas se comparan sin distinguir mayúsculas ni el tipo de barra.
fn normalize(path: &str) -> String {
    path.trim().replace('/', "\\").to_lowercase()
}

/// Rutas de los ejecutables de los juegos detectados por Windows.
fn read_games() -> HashSet<String> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        RRF_RT_REG_SZ,
    };
    let mut games = HashSet::new();
    unsafe {
        let mut key = HKEY::default();
        let root = HSTRING::from("System\\GameConfigStore\\Children");
        if RegOpenKeyExW(HKEY_CURRENT_USER, &root, None, KEY_READ, &mut key).is_err() {
            return games;
        }
        let value_name = HSTRING::from("MatchedExeFullPath");
        for index in 0.. {
            let mut name = [0u16; 64];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            let mut value = vec![0u16; 1024];
            let mut size = (value.len() * 2) as u32;
            let read = RegGetValueW(
                key,
                PCWSTR(name.as_ptr()),
                &value_name,
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut size),
            );
            let end = value.iter().position(|&c| c == 0).unwrap_or(0);
            if read.is_ok() && end > 0 {
                games.insert(normalize(&String::from_utf16_lossy(&value[..end])));
            }
        }
        let _ = RegCloseKey(key);
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_regardless_of_case_and_slashes() {
        assert_eq!(
            normalize("D:\\Epic Games\\rocketleague\\Binaries\\Win64\\RocketLeague.exe"),
            normalize("d:/epic games/RocketLeague/Binaries/Win64/rocketleague.exe ")
        );
    }
}
