//! Salidas de audio y aplicaciones que lo usan, para el panel rápido: cambiar la
//! salida predeterminada y la de cada aplicación.
//!
//! Windows no ofrece a las apps una API pública para ninguna de las dos cosas. La
//! salida predeterminada se cambia con `IPolicyConfig` y la de cada aplicación con
//! `AudioPolicyConfig`, las interfaces internas que usan la configuración de sonido
//! de Windows y herramientas como EarTrumpet. Si una versión de Windows no las
//! tiene, el panel lo indica en lugar de fallar.
use std::ffi::c_void;
use tiny_skia::Pixmap;
use windows::core::{Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// Identificador de Windows del dispositivo ("{0.0.0.00000000}.{…}").
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    /// Identifica a la aplicación aunque tenga varios procesos: su AppUserModelID si
    /// es de la Microsoft Store, o la ruta de su ejecutable.
    pub path: String,
    pub pids: Vec<u32>,
    /// Salida elegida para ella; `None` si usa la predeterminada.
    pub output: Option<String>,
    pub icon: Option<Pixmap>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mixer {
    pub outputs: Vec<Output>,
    /// Salida predeterminada.
    pub default: Option<String>,
    pub apps: Vec<App>,
    /// Esta versión de Windows permite elegir la salida de cada aplicación.
    pub per_app: bool,
}

/// Salida de audio activa de cada dispositivo, y la predeterminada.
unsafe fn outputs(enumerator: &IMMDeviceEnumerator) -> (Vec<Output>, Option<String>) {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::System::Com::STGM_READ;
    let take = |raw: windows::core::PWSTR| {
        let text = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0 as *const _));
        text
    };
    let mut list = Vec::new();
    if let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
        for index in 0..devices.GetCount().unwrap_or(0) {
            let Ok(device) = devices.Item(index) else {
                continue;
            };
            let Ok(id) = device.GetId().map(take) else {
                continue;
            };
            let name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .map(|value| value.to_string())
                .unwrap_or_default();
            list.push(Output { id, name });
        }
    }
    let default = enumerator
        .GetDefaultAudioEndpoint(eRender, eMultimedia)
        .and_then(|d| d.GetId())
        .map(take)
        .ok();
    (list, default)
}

/// Lee salidas y aplicaciones. `icons` guarda los iconos ya cargados entre lecturas.
pub fn read(icons: &mut std::collections::HashMap<String, Option<Pixmap>>) -> Mixer {
    let _p = crate::profile::scope("audio.read");
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return Mixer::default();
        };
        let (outputs, default) = outputs(&enumerator);
        let policy = Policy::open();
        let parents = processes();
        let mut apps: Vec<App> = Vec::new();
        for output in &outputs {
            for pid in session_processes(&enumerator, &output.id) {
                // El audio de WebView2 se atribuye a la aplicación que lo usa.
                let owner = owner(pid, &parents);
                let Some((exe, app_id)) = crate::apps::process_info(owner) else {
                    continue;
                };
                // Las apps de la Store se reconocen por su paquete, no por su .exe.
                let shell = app_id.map(|id| format!("shell:AppsFolder\\{id}"));
                let key = shell.clone().unwrap_or_else(|| exe.clone());
                if let Some(app) = apps.iter_mut().find(|a| a.path == key) {
                    if !app.pids.contains(&pid) {
                        app.pids.push(pid);
                    }
                    continue;
                }
                let name = shell
                    .as_deref()
                    .and_then(crate::icons::display_name)
                    .or_else(|| crate::apps::file_description(&exe))
                    .unwrap_or_else(|| {
                        let file = exe.rsplit('\\').next().unwrap_or(&exe);
                        file.strip_suffix(".exe").unwrap_or(file).to_string()
                    });
                let icon = icons
                    .entry(key.clone())
                    .or_insert_with(|| {
                        crate::icons::load(&key).map(|i| crate::apps::downscale(&i, 48))
                    })
                    .clone();
                apps.push(App {
                    name,
                    output: policy.as_ref().and_then(|p| p.get(pid)),
                    path: key,
                    pids: vec![pid],
                    icon,
                });
            }
        }
        apps.sort_by_key(|a| a.name.to_lowercase());
        Mixer {
            outputs,
            default,
            apps,
            per_app: policy.is_some(),
        }
    }
}

/// Procesos con una sesión de audio viva en la salida `id` (sin los sonidos del
/// sistema).
unsafe fn session_processes(enumerator: &IMMDeviceEnumerator, id: &str) -> Vec<u32> {
    use windows::Win32::Media::Audio::{
        AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2,
    };
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let mut pids = Vec::new();
    let Ok(device) = enumerator.GetDevice(PCWSTR(wide.as_ptr())) else {
        return pids;
    };
    let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
        return pids;
    };
    let Ok(sessions) = manager.GetSessionEnumerator() else {
        return pids;
    };
    for index in 0..sessions.GetCount().unwrap_or(0) {
        let Ok(control) = sessions
            .GetSession(index)
            .and_then(|s| s.cast::<IAudioSessionControl2>())
        else {
            continue;
        };
        // S_OK (0) significa que es la sesión de sonidos del sistema.
        if control.IsSystemSoundsSession().0 == 0
            || control
                .GetState()
                .is_ok_and(|s| s == AudioSessionStateExpired)
        {
            continue;
        }
        if let Ok(pid) = control.GetProcessId() {
            if pid != 0 && !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Padre y nombre del ejecutable de cada proceso en marcha.
fn processes() -> std::collections::HashMap<u32, (u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut map = std::collections::HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    map
}

/// Proceso al que se atribuye el audio de `pid`: si es de WebView2 (el componente
/// con el que apps como WhatsApp o Teams muestran contenido web), la aplicación que
/// lo abrió, subiendo por sus padres; si no, el propio proceso.
fn owner(pid: u32, processes: &std::collections::HashMap<u32, (u32, String)>) -> u32 {
    let is_webview = |pid: u32| {
        processes
            .get(&pid)
            .is_some_and(|(_, n)| n.eq_ignore_ascii_case("msedgewebview2.exe"))
    };
    let mut current = pid;
    // Pocos niveles: proceso de audio → navegador de WebView2 → aplicación.
    for _ in 0..8 {
        if !is_webview(current) {
            return current;
        }
        match processes.get(&current) {
            Some(&(parent, _)) if parent != 0 && processes.contains_key(&parent) => {
                current = parent
            }
            _ => break,
        }
    }
    // Si la aplicación ya no está, queda el propio WebView2.
    if is_webview(current) {
        pid
    } else {
        current
    }
}

/// Cambia la salida predeterminada (para música y vídeo y para el resto, como hace
/// Windows al elegir "Dispositivo predeterminado").
pub fn set_default(id: &str) {
    // CLSID de PolicyConfigClient e IID de IPolicyConfig (Windows 7 en adelante).
    const CLSID: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    // SetDefaultEndpoint es el método 14 de la tabla: los 3 de IUnknown y 10 más.
    type SetDefault = unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT;
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let Ok(client) = CoCreateInstance::<_, windows::core::IUnknown>(&CLSID, None, CLSCTX_ALL)
        else {
            return;
        };
        let mut config = std::ptr::null_mut();
        if client.query(&IID, &mut config).is_err() || config.is_null() {
            return;
        }
        let set: SetDefault = std::mem::transmute(method(config, 13));
        for role in [eConsole, eMultimedia] {
            let _ = set(config, PCWSTR(wide.as_ptr()), role.0);
        }
        release(config);
    }
}

/// Elige la salida de los procesos de una aplicación; `None` vuelve a la
/// predeterminada. Windows lo recuerda para la próxima vez que se abra.
pub fn set_app_output(pids: &[u32], id: Option<&str>) {
    if let Some(policy) = Policy::open() {
        for &pid in pids {
            policy.set(pid, id);
        }
    }
}

/// Entrada `index` de la tabla de métodos de un objeto COM.
unsafe fn method(object: *mut c_void, index: usize) -> *const c_void {
    let table = *(object as *const *const *const c_void);
    *table.add(index)
}
unsafe fn release(object: *mut c_void) {
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    let release: Release = std::mem::transmute(method(object, 2));
    release(object);
}

/// Identificador de una salida tal como lo usa AudioPolicyConfig.
const DEVICE_PREFIX: &str = "\\\\?\\SWD#MMDEVAPI#";
const RENDER_SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";
pub fn policy_id(id: &str) -> String {
    format!("{DEVICE_PREFIX}{id}{RENDER_SUFFIX}")
}
/// Lo contrario: el identificador normal de la salida, o `None` si viene vacío
/// (la aplicación usa la predeterminada).
pub fn output_id(policy: &str) -> Option<String> {
    let id = policy.strip_prefix(DEVICE_PREFIX).unwrap_or(policy);
    let id = id.strip_suffix(RENDER_SUFFIX).unwrap_or(id);
    (!id.is_empty()).then(|| id.to_string())
}

/// Fábrica `Windows.Media.Internal.AudioPolicyConfig` (Windows 10 1803 en adelante).
struct Policy {
    factory: *mut c_void,
}
impl Policy {
    fn open() -> Option<Self> {
        windows_core::link!("combase.dll" "system" fn RoGetActivationFactory(class: *mut c_void, iid: *const GUID, factory: *mut *mut c_void) -> HRESULT);
        // La interfaz cambió de identificador en Windows 10 21H2; la tabla de métodos es
        // la misma.
        const IIDS: [GUID; 2] = [
            GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324),
            GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258),
        ];
        let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        for iid in IIDS {
            let mut factory = std::ptr::null_mut();
            let result = unsafe {
                RoGetActivationFactory(std::mem::transmute_copy(&class), &iid, &mut factory)
            };
            if result.is_ok() && !factory.is_null() {
                return Some(Self { factory });
            }
        }
        None
    }

    // Tras los 6 métodos de IInspectable vienen 19 que no se usan aquí; luego
    // SetPersistedDefaultAudioEndpoint (25) y GetPersistedDefaultAudioEndpoint (26).
    fn set(&self, pid: u32, id: Option<&str>) {
        type Set = unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut c_void) -> HRESULT;
        let device = id
            .map(|id| HSTRING::from(policy_id(id)))
            .unwrap_or_default();
        unsafe {
            let set: Set = std::mem::transmute(method(self.factory, 25));
            for role in [eConsole, eMultimedia] {
                let _ = set(
                    self.factory,
                    pid,
                    eRender.0,
                    role.0,
                    std::mem::transmute_copy(&device),
                );
            }
        }
    }
    fn get(&self, pid: u32) -> Option<String> {
        type Get =
            unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut *mut c_void) -> HRESULT;
        unsafe {
            let get: Get = std::mem::transmute(method(self.factory, 26));
            let mut raw = std::ptr::null_mut();
            if get(self.factory, pid, eRender.0, eMultimedia.0, &mut raw).is_err() {
                return None;
            }
            // El HSTRING devuelto pasa a ser nuestro y se libera al soltarlo.
            let text: HSTRING = std::mem::transmute(raw);
            output_id(&text.to_string())
        }
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        unsafe { release(self.factory) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_round_trip_through_the_policy_format() {
        let id = "{0.0.0.00000000}.{1b4a2c3d-0000-4e5f-8a9b-0c1d2e3f4a5b}";
        let policy = policy_id(id);
        assert_eq!(
            policy,
            "\\\\?\\SWD#MMDEVAPI#{0.0.0.00000000}.{1b4a2c3d-0000-4e5f-8a9b-0c1d2e3f4a5b}#{e6327cad-dcec-4949-ae8a-991e976a79d2}"
        );
        assert_eq!(output_id(&policy).as_deref(), Some(id));
        // Vacío: la aplicación usa la salida predeterminada.
        assert_eq!(output_id(""), None);
    }

    #[test]
    fn webview2_audio_belongs_to_the_app_that_opened_it() {
        let processes: std::collections::HashMap<u32, (u32, String)> = [
            (10, (1, "explorer.exe")),
            (20, (10, "WhatsApp.Root.exe")),
            (21, (20, "msedgewebview2.exe")),
            (22, (21, "msedgewebview2.exe")),
            (30, (10, "Spotify.exe")),
            // WebView2 cuyo padre ya terminó.
            (40, (99, "msedgewebview2.exe")),
        ]
        .into_iter()
        .map(|(pid, (parent, name))| (pid, (parent, name.to_string())))
        .collect();
        assert_eq!(owner(22, &processes), 20);
        assert_eq!(owner(21, &processes), 20);
        assert_eq!(owner(30, &processes), 30);
        assert_eq!(owner(40, &processes), 40);
        assert_eq!(owner(77, &processes), 77);
    }
}
