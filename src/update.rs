//! Actualizaciones desde las versiones publicadas en GitHub: mira si hay una más
//! nueva que la instalada y, con un clic, descarga su instalador, comprueba su suma
//! SHA-256 y lo ejecuta en silencio. Usa WinHTTP y la criptografía de Windows, sin
//! dependencias propias. Solo se pide la última versión publicada: no se envía
//! ningún dato del equipo ni del uso.
use std::path::{Path, PathBuf};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

/// Repositorio donde se publican las versiones.
const REPOSITORY: &str = "UtilityForCoders/GlassBar";
/// Cada cuánto se vuelve a mirar, además de al arrancar.
pub const CHECK_EVERY_MS: u32 = 6 * 60 * 60 * 1000;

/// Versión X.Y.Z; el sufijo de las preliminares (`-beta.1`) no cuenta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let core = text.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let version = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
    /// La de este ejecutable.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Una versión publicada con su instalador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    installer: String,
    sums: String,
}

#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// Nombre del instalador de `version`, como lo publica `release.yml`.
fn installer_name(version: Version) -> String {
    format!("glassbar-setup-{version}-x64.exe")
}

/// La versión de la respuesta de GitHub, si trae instalador y sumas.
fn parse_release(json: &[u8]) -> Option<Release> {
    let release: GithubRelease = serde_json::from_slice(json).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    let version = Version::parse(&release.tag_name)?;
    let url = |name: &str| {
        release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.browser_download_url.clone())
    };
    Some(Release {
        version,
        installer: url(&installer_name(version))?,
        sums: url("SHA256SUMS.txt")?,
    })
}

/// La última versión publicada, si es más nueva que esta. Bloquea mientras
/// pregunta: se llama desde un hilo aparte.
pub fn newer() -> Result<Option<Release>, String> {
    let release = latest()?;
    Ok((release.version > Version::current()).then_some(release))
}

/// La última versión publicada.
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let json = get(&url)?;
    parse_release(&json).ok_or_else(|| "la versión publicada no trae instalador".into())
}

/// Descarga el instalador de `release` y comprueba su suma. Devuelve dónde quedó.
pub fn download(release: &Release) -> Result<PathBuf, String> {
    let name = installer_name(release.version);
    let sums = String::from_utf8_lossy(&get(&release.sums)?).into_owned();
    let expected = expected_sum(&sums, &name).ok_or("SHA256SUMS.txt no trae el instalador")?;
    let installer = get(&release.installer)?;
    if sha256_hex(&installer)? != expected {
        return Err("el instalador descargado no coincide con su suma SHA-256".into());
    }
    let folder = std::env::temp_dir().join("GlassBar");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(name);
    std::fs::write(&path, installer).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Ejecuta el instalador en silencio. Cierra GlassBar si sigue abierto y lo vuelve
/// a abrir al terminar (ver `installer/glassbar.iss`).
pub fn install(installer: &Path) -> Result<(), String> {
    std::process::Command::new(installer)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// La suma de `name` en un SHA256SUMS.txt (`<suma>  <archivo>` por línea).
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && sum.len() == 64).then(|| sum.to_ascii_lowercase())
    })
}

/// SHA-256 en hexadecimal, con la criptografía de Windows.
fn sha256_hex(data: &[u8]) -> Result<String, String> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut digest = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut digest) };
    if status.is_err() {
        return Err("no se pudo calcular la suma SHA-256".into());
    }
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Cierra un identificador de WinHTTP al salir de su ámbito.
struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// GET por HTTPS. WinHTTP sigue las redirecciones (las descargas de GitHub pasan
/// a su almacén de archivos) y usa el proxy del sistema.
fn get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("solo se descarga por HTTPS")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let agent = HSTRING::from(format!("GlassBar/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = Handle(WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err("no se pudo abrir la conexión".into());
        }
        let connection = Handle(WinHttpConnect(
            session.0,
            &HSTRING::from(host),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if connection.0.is_null() {
            return Err(format!("no se pudo conectar con {host}"));
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err("no se pudo preparar la petición".into());
        }
        let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n"
            .encode_utf16()
            .collect();
        let _ = WinHttpAddRequestHeaders(request.0, &headers, WINHTTP_ADDREQ_FLAG_ADD);
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)
            .and_then(|_| WinHttpReceiveResponse(request.0, std::ptr::null_mut()))
            .map_err(|e| format!("sin respuesta de {host}: {}", e.message()))?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|e| e.message())?;
        if status != 200 {
            return Err(format!("{host} respondió {status}"));
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .map_err(|e| format!("descarga interrumpida: {}", e.message()))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok(body)
    }
}

/// Ajustes de las actualizaciones, en `%LOCALAPPDATA%\GlassBar\updates.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Buscar versiones nuevas al arrancar y cada 6 horas.
    pub check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { check: true }
    }
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        let path =
            settings_path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA no disponible"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
    }
}

fn settings_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("GlassBar")
            .join("updates.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse("1.10.3-beta.1"), Some(Version(1, 10, 3)));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("vX"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_release_needs_its_installer_and_sums() {
        let json = br#"{
            "tag_name": "v0.3.0",
            "draft": false,
            "prerelease": false,
            "assets": [
                {"name": "glassbar-0.3.0-x64.exe", "browser_download_url": "https://x/exe"},
                {"name": "glassbar-setup-0.3.0-x64.exe", "browser_download_url": "https://x/setup"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums"}
            ]
        }"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, Version(0, 3, 0));
        assert_eq!(release.installer, "https://x/setup");
        assert_eq!(release.sums, "https://x/sums");
        // Sin instalador, o preliminar, no sirve.
        let missing = br#"{"tag_name": "v0.3.0", "assets": []}"#;
        assert_eq!(parse_release(missing), None);
        let beta = String::from_utf8_lossy(json)
            .replace(r#""prerelease": false"#, r#""prerelease": true"#);
        assert_eq!(parse_release(beta.as_bytes()), None);
    }

    #[test]
    fn sums_are_found_by_file_name() {
        let sum = "a".repeat(64);
        let sums = format!(
            "{}  glassbar-0.3.0-x64.exe\n{sum} *glassbar-setup-0.3.0-x64.exe\n",
            "b".repeat(64)
        );
        assert_eq!(
            expected_sum(&sums, "glassbar-setup-0.3.0-x64.exe"),
            Some(sum)
        );
        assert_eq!(expected_sum(&sums, "otro.exe"), None);
    }

    #[test]
    fn sha256_matches_the_known_value() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
