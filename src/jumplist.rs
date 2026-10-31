//! La jump list de cada app, lo mismo que la barra de Windows muestra con clic
//! derecho, igual para todas las apps:
//!
//! - Recientes y frecuentes, con la API pública de Windows (`IApplicationDocumentLists`).
//! - Las categorías propias y las tareas que declara cada app (p. ej. "Nueva ventana
//!   de incógnito" de Chrome), con la interfaz interna que usa la barra
//!   (`IInternalCustomDestinationList`, sin documentar; la usa también Open-Shell).
//!   Si en otra versión de Windows no está, el menú se queda con los recientes.
//!
//! Windows guarda cada jump list por el AppUserModelID de la app: el que declara
//! (Store, Chrome, VS Code…) o, si no declara ninguno, uno sacado de la ruta de su
//! ejecutable.

// Los métodos de la interfaz interna llevan los nombres de Windows.
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::OnceLock;
use windows::core::{
    IUnknown, IUnknown_Vtbl, Interface, BOOL, GUID, HRESULT, HSTRING, PCWSTR, PWSTR,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::EnhancedStorage::{
    PKEY_AppUserModel_ID, PKEY_AppUserModel_IsDestListSeparator, PKEY_Link_TargetParsingPath,
    PKEY_Title,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantToBoolean, PropVariantToStringAlloc,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::Common::IObjectArray;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::*;

/// Qué hace un elemento al elegirlo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Un archivo o carpeta (recientes, frecuentes): se abre con la app.
    Open(String),
    /// Una tarea o enlace de la app: su programa, argumentos y carpeta de trabajo.
    Run {
        file: String,
        args: String,
        dir: String,
    },
}

/// Un elemento de la jump list: lo que se ve y lo que hace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpItem {
    pub name: String,
    pub action: Action,
    /// Icono propio de la tarea (archivo y número de icono), como el de incógnito
    /// de Chrome. Sin él se usa el del archivo o programa que abre.
    pub icon: Option<(String, i32)>,
}

impl JumpItem {
    /// De dónde sacar su icono: el suyo propio si existe (Chrome apunta a iconos de
    /// páginas que a veces ya borró; Windows muestra entonces el del programa), o el
    /// del archivo o programa.
    pub fn icon_source(&self) -> (String, i32) {
        match (&self.icon, &self.action) {
            (Some((path, index)), _) if std::path::Path::new(path).exists() => {
                (path.clone(), *index)
            }
            (_, Action::Open(path)) => (path.clone(), -1),
            (_, Action::Run { file, .. }) => (file.clone(), -1),
        }
    }
}

/// Expande las variables de entorno de una ruta (`%ProgramFiles%\…`), como las
/// guardan algunos accesos directos en su icono.
fn expand(path: &str) -> String {
    let mut parts = path.split('%');
    let mut out = parts.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = parts.collect();
    let mut i = 0;
    while i < rest.len() {
        // Un nombre entre dos "%" que tenga un valor se sustituye; si no, queda igual.
        match (rest.get(i + 1), std::env::var(rest[i])) {
            (Some(after), Ok(value)) => {
                out.push_str(&value);
                out.push_str(after);
                i += 2;
            }
            _ => {
                out.push('%');
                out.push_str(rest[i]);
                i += 1;
            }
        }
    }
    out
}

/// Lista automática que lleva Windows por cada app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Recent,
    Frequent,
}

/// Una sección del menú: su título y sus elementos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub items: Vec<JumpItem>,
}

/// Cuántos elementos muestra la barra de Windows por defecto en cada lista.
const MAX_ITEMS: u32 = 10;

unsafe fn take(text: PWSTR) -> String {
    let value = text.to_string().unwrap_or_default();
    CoTaskMemFree(Some(text.0.cast()));
    value
}

/// Ejecuta `f` con COM inicializado en este hilo (si no lo estaba ya).
fn with_com<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = f();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// Las secciones que mostraría la barra de Windows para la app `app_id`.
pub fn sections(app_id: &str) -> Vec<Section> {
    with_com(|| arrange(custom_categories(app_id), |kind| read(app_id, kind)))
}

/// Ordena las secciones como la barra de Windows, sin las vacías. Si la app declara
/// su propia lista, van sus categorías en su orden (las suyas, y recientes o
/// frecuentes si las pide, leídas con `read`) y las tareas siempre al final; si no,
/// sus recientes o, si no tiene, sus frecuentes.
fn arrange(custom: Option<Vec<Category>>, read: impl Fn(Kind) -> Vec<JumpItem>) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut tasks = Vec::new();
    match custom {
        Some(categories) => {
            for category in categories {
                match category {
                    Category::Custom(title, items) => sections.push(Section { title, items }),
                    Category::Known(kind) => sections.push(Section {
                        title: kind_title(kind).into(),
                        items: read(kind),
                    }),
                    Category::Tasks(items) => tasks.extend(items),
                }
            }
        }
        None => sections.extend(
            [Kind::Recent, Kind::Frequent]
                .into_iter()
                .map(|kind| Section {
                    title: kind_title(kind).into(),
                    items: read(kind),
                })
                .find(|s| !s.items.is_empty()),
        ),
    }
    sections.push(Section {
        title: "Tareas".into(),
        items: tasks,
    });
    sections.retain(|s| !s.items.is_empty());
    sections
}

fn kind_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Recent => "Recientes",
        Kind::Frequent => "Frecuentes",
    }
}

fn read(app_id: &str, kind: Kind) -> Vec<JumpItem> {
    unsafe {
        let result = (|| -> windows::core::Result<Vec<JumpItem>> {
            let lists: IApplicationDocumentLists =
                CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER)?;
            lists.SetAppID(&HSTRING::from(app_id))?;
            let list = match kind {
                Kind::Recent => ADLT_RECENT,
                Kind::Frequent => ADLT_FREQUENT,
            };
            let array: IObjectArray = lists.GetList(list, MAX_ITEMS)?;
            // Solo archivos y carpetas: algunas apps guardan aquí direcciones internas
            // (p. ej. `whatsapp://chatOpen…`) que la barra de Windows no muestra.
            Ok(items_of(&array)
                .into_iter()
                .filter(|item| {
                    matches!(&item.action, Action::Open(path) if std::path::Path::new(path).exists())
                })
                .collect())
        })();
        result.unwrap_or_default()
    }
}

/// Los elementos de una lista de la jump list: documentos del Shell o accesos
/// directos (las tareas y los enlaces). Se saltan los separadores.
unsafe fn items_of(array: &IObjectArray) -> Vec<JumpItem> {
    let mut items = Vec::new();
    for i in 0..array.GetCount().unwrap_or(0) {
        let item = if let Ok(link) = array.GetAt::<IShellLinkW>(i) {
            link_item(&link)
        } else if let Ok(item) = array.GetAt::<IShellItem>(i) {
            shell_item(&item)
        } else {
            None
        };
        if let Some(item) = item.filter(|i| !i.name.is_empty()) {
            items.push(item);
        }
    }
    items
}

unsafe fn shell_item(item: &IShellItem) -> Option<JumpItem> {
    let name = take(item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?);
    let path = take(item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).ok()?);
    Some(JumpItem {
        name,
        action: Action::Open(path),
        icon: None,
    })
}

/// Una tarea o enlace: su título (en sus propiedades, a veces como recurso
/// "@archivo.dll,-123") y lo que ejecuta.
unsafe fn link_item(link: &IShellLinkW) -> Option<JumpItem> {
    let store: Option<IPropertyStore> = link.cast().ok();
    if let Some(store) = &store {
        let separator = store
            .GetValue(&PKEY_AppUserModel_IsDestListSeparator)
            .ok()
            .and_then(|v| PropVariantToBoolean(&v).ok())
            .is_some_and(|b| b.as_bool());
        if separator {
            return None;
        }
    }
    let text = |read: &dyn Fn(&mut [u16]) -> windows::core::Result<()>| {
        let mut buffer = vec![0u16; 2048];
        read(&mut buffer).ok()?;
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let value = String::from_utf16_lossy(&buffer[..end]);
        (!value.is_empty()).then_some(value)
    };
    let file = text(&|b| link.GetPath(b, std::ptr::null_mut(), 0)).unwrap_or_default();
    let args = text(&|b| link.GetArguments(b)).unwrap_or_default();
    let dir = text(&|b| link.GetWorkingDirectory(b)).unwrap_or_default();
    let title = store
        .and_then(|s| s.GetValue(&PKEY_Title).ok())
        .and_then(|v| PropVariantToStringAlloc(&v).ok())
        .map(|t| take(t))
        .filter(|t| !t.is_empty())
        .or_else(|| text(&|b| link.GetDescription(b)))
        .map(|t| indirect(&t))
        .unwrap_or_else(|| {
            std::path::Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    if file.is_empty() {
        return None;
    }
    let index = std::cell::Cell::new(0);
    let icon = text(&|b| {
        let mut number = 0;
        let result = link.GetIconLocation(b, &mut number);
        index.set(number);
        result
    })
    .map(|path| (expand(&path), index.get()));
    Some(JumpItem {
        name: title,
        action: Action::Run { file, args, dir },
        icon,
    })
}

/// Resuelve los textos guardados como recurso ("@shell32.dll,-123" o
/// "@{Paquete?ms-resource://…}"); los demás quedan igual.
fn indirect(text: &str) -> String {
    if !text.starts_with('@') {
        return text.to_string();
    }
    let source = HSTRING::from(text);
    let mut buffer = vec![0u16; 1024];
    unsafe {
        if SHLoadIndirectString(PCWSTR(source.as_ptr()), &mut buffer, None).is_ok() {
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            return String::from_utf16_lossy(&buffer[..end]);
        }
    }
    text.to_string()
}

// ---------------------------------------------------------------------------
// Lista propia de cada app (interfaz interna de la barra de Windows).
// ---------------------------------------------------------------------------

/// La clase que guarda las listas propias de las apps ("Custom Destination List",
/// en windows.storage.dll).
const CLSID_DESTINATION_LIST: GUID = GUID::from_u128(0x77F10CF0_3DB5_4966_B520_B7C54FD35ED6);

/// La interfaz con la que la barra de Windows 10 y 11 lee esas listas. Solo se
/// declaran los métodos hasta el último que se usa, en su orden (cuenta el orden,
/// no el nombre); los siguientes no hacen falta. Llevan los nombres de Windows.
#[windows::core::interface("5632B1A4-E38A-400A-928A-D4CD63230295")]
unsafe trait IInternalCustomDestinationList: IUnknown {
    fn SetMinItems(&self, min: u32) -> HRESULT;
    fn SetApplicationID(&self, app_id: PCWSTR) -> HRESULT;
    fn GetSlotCount(&self, count: *mut u32) -> HRESULT;
    fn GetCategoryCount(&self, count: *mut u32) -> HRESULT;
    fn GetCategory(&self, index: u32, flags: u32, category: *mut AppDestCategory) -> HRESULT;
    fn DeleteCategory(&self, index: u32, permanently: BOOL) -> HRESULT;
    fn EnumerateCategoryDestinations(
        &self,
        index: u32,
        riid: *const GUID,
        object: *mut *mut std::ffi::c_void,
    ) -> HRESULT;
}

/// Una categoría tal como la da la interfaz: tipo 0 propia (con nombre), 1 lista
/// automática (1 frecuentes, 2 recientes) o 2 tareas.
#[repr(C)]
pub(crate) struct AppDestCategory {
    kind: i32,
    /// El nombre (texto que hay que liberar) o el subtipo, según `kind`.
    value: usize,
    count: i32,
    padding: [i32; 10],
}

/// Categoría de la lista propia de una app, en el orden en que la declaró.
enum Category {
    Custom(String, Vec<JumpItem>),
    Known(Kind),
    Tasks(Vec<JumpItem>),
}

/// Versiones de la interfaz, de la más nueva a la más antigua. Windows ha cambiado
/// su identificador, pero el principio de la tabla de métodos (lo único que se usa)
/// es el mismo en ambas; en algunas versiones de Windows 11 solo responde la antigua.
const LIST_VERSIONS: [GUID; 2] = [
    GUID::from_u128(0x5632B1A4_E38A_400A_928A_D4CD63230295),
    GUID::from_u128(0x507101CD_F6AD_46C8_8E20_EEB9E6BAC47F),
];

/// La lista de destinos con la primera versión de la interfaz que admita.
unsafe fn destination_list() -> Option<IInternalCustomDestinationList> {
    let unknown: IUnknown =
        CoCreateInstance(&CLSID_DESTINATION_LIST, None, CLSCTX_INPROC_SERVER).ok()?;
    LIST_VERSIONS.iter().find_map(|iid| {
        let mut raw = std::ptr::null_mut();
        unknown.query(iid, &mut raw).ok().ok()?;
        (!raw.is_null()).then(|| IInternalCustomDestinationList::from_raw(raw))
    })
}

/// La lista propia de la app, si declaró una. `None` si no la tiene o si la
/// interfaz no está en esta versión de Windows.
fn custom_categories(app_id: &str) -> Option<Vec<Category>> {
    unsafe {
        let list = destination_list()?;
        let id = HSTRING::from(app_id);
        list.SetApplicationID(PCWSTR(id.as_ptr())).ok().ok()?;
        let mut count = 0u32;
        list.GetCategoryCount(&mut count).ok().ok()?;
        if count == 0 {
            return None;
        }
        let mut categories = Vec::new();
        for index in 0..count {
            let mut category = AppDestCategory {
                kind: 0,
                value: 0,
                count: 0,
                padding: [0; 10],
            };
            if list.GetCategory(index, 1, &mut category).is_err() {
                continue;
            }
            let items = || {
                let mut object = std::ptr::null_mut();
                list.EnumerateCategoryDestinations(index, &IObjectArray::IID, &mut object)
                    .ok()
                    .ok()
                    .filter(|_| !object.is_null())
                    .map(|_| items_of(&IObjectArray::from_raw(object)))
                    .unwrap_or_default()
            };
            match category.kind {
                0 => {
                    let name = if category.value != 0 {
                        indirect(&take(PWSTR(category.value as *mut u16)))
                    } else {
                        String::new()
                    };
                    categories.push(Category::Custom(name, items()));
                }
                1 => match category.value as i32 {
                    1 => categories.push(Category::Known(Kind::Frequent)),
                    2 => categories.push(Category::Known(Kind::Recent)),
                    _ => {}
                },
                2 => categories.push(Category::Tasks(items())),
                _ => {}
            }
        }
        Some(categories)
    }
}

// ---------------------------------------------------------------------------
// AppUserModelID de cada app.
// ---------------------------------------------------------------------------

/// AppUserModelID de una app del dock: el de su ruta de AppsFolder, el que declara
/// su ventana, el de su acceso directo, el de su entrada en AppsFolder o el que
/// Windows saca de la ruta del ejecutable.
pub fn app_id(command: &str, windows: &[isize]) -> Option<String> {
    if let Some(id) = command.strip_prefix("shell:AppsFolder\\") {
        return Some(id.to_string());
    }
    if let Some(id) = windows.iter().find_map(|&w| window_app_id(w)) {
        return Some(id);
    }
    let lower = command.to_lowercase();
    if lower.ends_with(".lnk") {
        if let Some(id) = with_com(|| item_app_id(command)) {
            return Some(id);
        }
    }
    let exe = crate::pins::shortcut_target(command).unwrap_or_else(|| command.to_string());
    if exe.is_empty() {
        return None;
    }
    let exe_lower = exe.to_lowercase();
    // El Explorador sin ventanas abiertas no declara ninguno, y el suyo no sale de la ruta.
    if exe_lower.rsplit(['\\', '/']).next() == Some("explorer.exe") {
        return Some("Microsoft.Windows.Explorer".into());
    }
    if let Some(id) = apps_folder_ids().get(&exe_lower) {
        return Some(id.clone());
    }
    Some(default_app_id(&exe, &known_folders()))
}

/// El que declara una ventana (las apps lo fijan para agruparse en la barra).
fn window_app_id(window: isize) -> Option<String> {
    with_com(|| unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(HWND(window as *mut _)).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let id = take(PropVariantToStringAlloc(&value).ok()?);
        (!id.is_empty()).then_some(id)
    })
}

/// El que guarda un acceso directo (.lnk) en sus propiedades.
fn item_app_id(path: &str) -> Option<String> {
    unsafe {
        let item: IShellItem2 =
            SHCreateItemFromParsingName(PCWSTR(HSTRING::from(path).as_ptr()), None).ok()?;
        let id = take(item.GetString(&PKEY_AppUserModel_ID).ok()?);
        (!id.is_empty()).then_some(id)
    }
}

/// Ejecutable (en minúsculas) → AppUserModelID de las apps del menú Inicio. Leerlo
/// tarda: se prepara en segundo plano al arrancar con `warm_up`.
fn apps_folder_ids() -> &'static HashMap<String, String> {
    static IDS: OnceLock<HashMap<String, String>> = OnceLock::new();
    IDS.get_or_init(|| with_com(read_apps_folder))
}

/// Prepara en otro hilo lo que tarda en leerse, para que el primer clic derecho
/// no espere.
pub fn warm_up() {
    std::thread::spawn(|| {
        apps_folder_ids();
    });
}

fn read_apps_folder() -> HashMap<String, String> {
    unsafe {
        let result = (|| -> windows::core::Result<HashMap<String, String>> {
            let folder: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
            let mut ids = HashMap::new();
            loop {
                let mut batch = [None];
                let mut fetched = 0;
                items.Next(&mut batch, Some(&mut fetched))?;
                if fetched == 0 {
                    break;
                }
                let Some(item) = batch[0].take().and_then(|i| i.cast::<IShellItem2>().ok()) else {
                    continue;
                };
                let (Ok(id), Ok(target)) = (
                    item.GetString(&PKEY_AppUserModel_ID),
                    item.GetString(&PKEY_Link_TargetParsingPath),
                ) else {
                    continue;
                };
                let (id, target) = (take(id), take(target));
                if !id.is_empty() && !target.is_empty() {
                    ids.entry(target.to_lowercase()).or_insert(id);
                }
            }
            Ok(ids)
        })();
        result.unwrap_or_default()
    }
}

/// Carpetas que Windows sustituye por su GUID al sacar el AppUserModelID de una
/// ruta (p. ej. `{6D809377-…}\Notepad++\notepad++.exe`), de la más concreta a la
/// más general.
fn known_folders() -> Vec<(String, &'static str)> {
    let folders = [
        (FOLDERID_SystemX86, "{D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27}"),
        (FOLDERID_System, "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}"),
        (
            FOLDERID_ProgramFilesX86,
            "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}",
        ),
        (
            FOLDERID_ProgramFilesX64,
            "{6D809377-6AF0-444B-8957-A3773F02200E}",
        ),
        (FOLDERID_Windows, "{F38BF404-1D43-42F2-9305-67DE0B28FC23}"),
    ];
    folders
        .iter()
        .filter_map(|(id, guid)| unsafe {
            let path = take(SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?);
            (!path.is_empty()).then_some((path, *guid))
        })
        .collect()
}

/// AppUserModelID que Windows da a un programa que no declara ninguno: su ruta, con
/// la carpeta conocida en la que está cambiada por su GUID.
pub fn default_app_id(exe: &str, folders: &[(String, &str)]) -> String {
    let lower = exe.to_lowercase();
    for (folder, guid) in folders {
        let prefix = format!("{}\\", folder.to_lowercase());
        if lower.starts_with(&prefix) {
            return format!("{guid}\\{}", &exe[prefix.len()..]);
        }
    }
    exe.to_string()
}

// ---------------------------------------------------------------------------
// Abrir un elemento.
// ---------------------------------------------------------------------------

/// Programa, argumentos y carpeta con los que se abre un elemento, como la barra de
/// Windows. Un archivo o carpeta se abre con la propia app si se sabe su ejecutable
/// (así un .txt reciente de VS Code se abre en VS Code y no en el Bloc de notas); si
/// no, como en las apps de Store o el Explorador, con lo que Windows tenga
/// asociado. Una tarea ejecuta lo que guarda.
pub fn command_for(item: &JumpItem, app_command: &str) -> (String, String, String) {
    match &item.action {
        Action::Run { file, args, dir } => (file.clone(), args.clone(), dir.clone()),
        Action::Open(path) => {
            let exe = crate::pins::shortcut_target(app_command)
                .unwrap_or_else(|| app_command.to_string());
            let lower = exe.to_lowercase();
            if lower.ends_with(".exe") && !lower.ends_with("\\explorer.exe") {
                (exe, format!("\"{path}\""), String::new())
            } else {
                (path.clone(), String::new(), String::new())
            }
        }
    }
}

/// Abre el elemento (ver `command_for`). Devuelve si Windows lo aceptó.
pub fn open(item: &JumpItem, app_command: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (file, args, dir) = command_for(item, app_command);
    let optional = |text: &HSTRING| {
        if text.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(text.as_ptr())
        }
    };
    let (file, args, dir) = (HSTRING::from(file), HSTRING::from(args), HSTRING::from(dir));
    unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(file.as_ptr()),
            optional(&args),
            optional(&dir),
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ids_replace_the_known_folder_with_its_guid() {
        let folders = vec![
            (
                r"C:\Windows\System32".to_string(),
                "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
            ),
            (
                r"C:\Program Files".to_string(),
                "{6D809377-6AF0-444B-8957-A3773F02200E}",
            ),
            (
                r"C:\Windows".to_string(),
                "{F38BF404-1D43-42F2-9305-67DE0B28FC23}",
            ),
        ];
        assert_eq!(
            default_app_id(r"C:\Program Files\Notepad++\notepad++.exe", &folders),
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\Notepad++\notepad++.exe"
        );
        // La más concreta gana: System32 antes que Windows.
        assert_eq!(
            default_app_id(r"c:\windows\system32\notepad.exe", &folders),
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe"
        );
        // Fuera de las carpetas conocidas queda la ruta tal cual, como en Windows.
        let own = r"C:\Users\gerso\AppData\Local\Programs\Notch Isla\notch-isla.exe";
        assert_eq!(default_app_id(own, &folders), own);
    }

    #[test]
    fn apps_folder_paths_and_explorer_have_a_known_id() {
        assert_eq!(
            app_id(r"shell:AppsFolder\Microsoft.VisualStudioCode", &[]).as_deref(),
            Some("Microsoft.VisualStudioCode")
        );
        assert_eq!(
            app_id(r"C:\Windows\explorer.exe", &[]).as_deref(),
            Some("Microsoft.Windows.Explorer")
        );
    }

    fn named(name: &str) -> JumpItem {
        JumpItem {
            name: name.into(),
            action: Action::Open(name.into()),
            icon: None,
        }
    }

    #[test]
    fn sections_follow_the_windows_taskbar_order() {
        let read = |kind: Kind| match kind {
            Kind::Recent => vec![named("reciente.txt")],
            Kind::Frequent => vec![named("frecuente.txt")],
        };
        let titles = |sections: Vec<Section>| {
            sections
                .into_iter()
                .map(|s| (s.title, s.items.len()))
                .collect::<Vec<_>>()
        };
        // Lista propia como la de VS Code: declara las tareas primero, pero van al
        // final; las categorías vacías no salen.
        let custom = vec![
            Category::Tasks(vec![named("New Window")]),
            Category::Custom("Recent Folders".into(), vec![named("GlassBar")]),
            Category::Custom("Vacía".into(), vec![]),
            Category::Known(Kind::Recent),
        ];
        assert_eq!(
            titles(arrange(Some(custom), read)),
            [
                ("Recent Folders".to_string(), 1),
                ("Recientes".to_string(), 1),
                ("Tareas".to_string(), 1)
            ]
        );
        // Con lista propia sin recientes (como WhatsApp), no se añaden.
        let only_tasks = vec![Category::Tasks(vec![named("Nueva ventana")])];
        assert_eq!(
            titles(arrange(Some(only_tasks), read)),
            [("Tareas".to_string(), 1)]
        );
        // Sin lista propia: recientes y, si no hay, frecuentes.
        assert_eq!(titles(arrange(None, read)), [("Recientes".to_string(), 1)]);
        let no_recent = |kind: Kind| match kind {
            Kind::Recent => vec![],
            Kind::Frequent => vec![named("frecuente.txt")],
        };
        assert_eq!(
            titles(arrange(None, no_recent)),
            [("Frecuentes".to_string(), 1)]
        );
    }

    #[test]
    fn items_open_with_their_app_and_tasks_run_as_saved() {
        let file = JumpItem {
            name: "notas.txt".into(),
            action: Action::Open(r"C:\docs\notas.txt".into()),
            icon: None,
        };
        // Con la propia app si es un programa de escritorio.
        assert_eq!(
            command_for(&file, r"C:\Programs\Code\Code.exe"),
            (
                r"C:\Programs\Code\Code.exe".into(),
                r#""C:\docs\notas.txt""#.into(),
                String::new()
            )
        );
        // Con lo asociado si es una app de Store o el Explorador.
        for app in [
            r"shell:AppsFolder\Store.App!App",
            r"C:\Windows\explorer.exe",
        ] {
            assert_eq!(command_for(&file, app).0, r"C:\docs\notas.txt");
        }
        // Una tarea, tal cual la guardó la app.
        let task = JumpItem {
            name: "Nueva ventana de incógnito".into(),
            action: Action::Run {
                file: r"C:\Chrome\chrome.exe".into(),
                args: "--incognito".into(),
                dir: r"C:\Chrome".into(),
            },
            icon: Some((r"C:\Chrome\chrome.exe".into(), 3)),
        };
        // Su icono propio si existe; si ya no está (Chrome borra los de páginas), el
        // del programa. Sin icono propio, el del archivo.
        let explorer = format!(r"{}\explorer.exe", std::env::var("WINDIR").unwrap());
        let own = JumpItem {
            icon: Some((explorer.clone(), 3)),
            ..task.clone()
        };
        assert_eq!(own.icon_source(), (explorer, 3));
        let gone = JumpItem {
            icon: Some((r"C:\no\existe\pagina.ico".into(), 0)),
            ..task.clone()
        };
        assert_eq!(gone.icon_source(), (r"C:\Chrome\chrome.exe".into(), -1));
        assert_eq!(file.icon_source(), (r"C:\docs\notas.txt".into(), -1));
        std::env::set_var("GLASSBAR_PRUEBA", r"C:\Programas");
        assert_eq!(
            expand(r"%GLASSBAR_PRUEBA%\Chrome\chrome.exe"),
            r"C:\Programas\Chrome\chrome.exe"
        );
        assert_eq!(expand("100% seguro"), "100% seguro");
        assert_eq!(
            command_for(&task, "cualquiera"),
            (
                r"C:\Chrome\chrome.exe".into(),
                "--incognito".into(),
                r"C:\Chrome".into()
            )
        );
    }
}
