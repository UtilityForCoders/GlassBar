//! Iconos del Shell, rasterizados una vez y conservados en memoria.
use tiny_skia::Pixmap;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{
    SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, LoadIconW, DI_NORMAL, IDI_APPLICATION,
};

pub fn load(path: &str) -> Option<Pixmap> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    // El Shell puede usar extensiones COM con cachés compartidas; serializar la
    // extracción también mantiene deterministas las cargas desde varios hilos.
    static ICONS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ICONS.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = load_inner(path);
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// El icono número `index` de un archivo de recursos (.exe, .dll, .ico), como el que
/// guarda una tarea de la jump list. Con `index` negativo, el icono del archivo.
pub fn load_resource(path: &str, index: i32) -> Option<Pixmap> {
    if index < 0 {
        return load(path);
    }
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let mut icon = windows::Win32::UI::WindowsAndMessaging::HICON::default();
        let result =
            (SHDefExtractIconW(PCWSTR(wide.as_ptr()), index, 0, Some(&mut icon), None, 128)
                .is_ok()
                && !icon.is_invalid())
            .then(|| {
                let image = rasterize(icon);
                let _ = DestroyIcon(icon);
                image
            })
            .flatten();
        if initialized {
            CoUninitialize();
        }
        result.or_else(|| load(path))
    }
}

/// Nombre que el Shell muestra para una ruta o elemento (`shell:AppsFolder\...`).
pub fn display_name(path: &str) -> Option<String> {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
            let text = name.to_string().ok();
            CoTaskMemFree(Some(name.0 as *const _));
            text.filter(|t| !t.is_empty())
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

fn load_inner(path: &str) -> Option<Pixmap> {
    let normalized = path.replace('/', "\\");
    let wide: Vec<u16> = normalized.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let resource = crate::pins::shortcut_icon_source(&normalized).or_else(|| {
            std::path::Path::new(&normalized)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                .then(|| (normalized.clone(), 0))
        });
        if let Some((resource, index)) = resource {
            let wide: Vec<u16> = resource.encode_utf16().chain(Some(0)).collect();
            let mut icon = windows::Win32::UI::WindowsAndMessaging::HICON::default();
            if SHDefExtractIconW(PCWSTR(wide.as_ptr()), index, 0, Some(&mut icon), None, 128)
                .is_ok()
                && !icon.is_invalid()
            {
                let result = rasterize(icon);
                let _ = DestroyIcon(icon);
                if result.is_some() {
                    return result;
                }
            }
        }
        if let Some(image) = shell_image(PCWSTR(wide.as_ptr())) {
            return Some(image);
        }
        let mut info = SHFILEINFOW::default();
        if SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) != 0
            && !info.hIcon.is_invalid()
        {
            let result = rasterize(info.hIcon);
            let _ = DestroyIcon(info.hIcon);
            if result.is_some() {
                return result;
            }
        }
        rasterize(LoadIconW(None, IDI_APPLICATION).ok()?)
    }
}

/// La fábrica del Shell suministra imágenes grandes también para apps de Store.
unsafe fn shell_image(path: PCWSTR) -> Option<Pixmap> {
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
    };
    let factory: IShellItemImageFactory = SHCreateItemFromParsingName(path, None).ok()?;
    let bitmap = factory
        .GetImage(
            windows::Win32::Foundation::SIZE { cx: 128, cy: 128 },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
        .ok()?;
    let result = bitmap_image(bitmap);
    let _ = DeleteObject(bitmap.into());
    result
}
unsafe fn bitmap_image(bitmap: HBITMAP) -> Option<Pixmap> {
    let mut object = BITMAP::default();
    if GetObjectW(
        bitmap.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some((&mut object as *mut BITMAP).cast()),
    ) == 0
    {
        return None;
    }
    let width = object.bmWidth.unsigned_abs();
    let height = object.bmHeight.unsigned_abs();
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return None;
    }
    let mut bytes = vec![0u8; (width * height * 4) as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = CreateCompatibleDC(None);
    if dc.is_invalid() {
        return None;
    }
    let copied = GetDIBits(
        dc,
        bitmap,
        0,
        height,
        Some(bytes.as_mut_ptr().cast()),
        &mut info,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(dc);
    if copied != height as i32 {
        return None;
    }
    let opaque = bytes.chunks_exact(4).all(|p| p[3] == 0);
    let mut image = Pixmap::new(width, height)?;
    for (src, dst) in bytes
        .chunks_exact(4)
        .zip(image.data_mut().chunks_exact_mut(4))
    {
        let alpha = if opaque { 255 } else { src[3] };
        dst.copy_from_slice(&[
            src[2].min(alpha),
            src[1].min(alpha),
            src[0].min(alpha),
            alpha,
        ]);
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_icons_have_visible_and_transparent_pixels() {
        let path = format!("{}\\explorer.exe", std::env::var("WINDIR").unwrap());
        let icon = super::load(&path).expect("icono del Explorador");
        assert!(
            icon.width() >= 128 && icon.height() >= 128,
            "iconos de alta resolución"
        );
        assert!(icon.pixels().iter().any(|p| p.alpha() > 0));
        assert!(icon.pixels().iter().any(|p| p.alpha() == 0));
        assert!(icon
            .pixels()
            .iter()
            .all(|p| p.red() <= p.alpha() && p.green() <= p.alpha() && p.blue() <= p.alpha()));
    }
}

/// Imagen de un icono de Windows a `side` píxeles (p. ej. el de la bandeja de otro
/// programa).
pub fn from_hicon(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    side: u32,
) -> Option<Pixmap> {
    unsafe { rasterize_at(icon, side) }
}

unsafe fn rasterize(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Pixmap> {
    rasterize_at(icon, 128)
}

unsafe fn rasterize_at(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    side: u32,
) -> Option<Pixmap> {
    let side = side.max(1);
    let dc = CreateCompatibleDC(None);
    if dc.is_invalid() {
        return None;
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side as i32,
            biHeight: -(side as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) => b,
        Err(_) => {
            let _ = DeleteDC(dc);
            return None;
        }
    };
    let old = SelectObject(dc, bitmap.into());
    let result = (|| {
        // Dos fondos permiten recuperar también el alfa de iconos antiguos con máscara.
        let bytes = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (side * side * 4) as usize);
        bytes.fill(0);
        DrawIconEx(dc, 0, 0, icon, side as i32, side as i32, 0, None, DI_NORMAL).ok()?;
        let _ = GdiFlush();
        let black = bytes.to_vec();
        bytes.fill(255);
        DrawIconEx(dc, 0, 0, icon, side as i32, side as i32, 0, None, DI_NORMAL).ok()?;
        let _ = GdiFlush();
        let mut out = Pixmap::new(side, side)?;
        for ((dst, b), w) in out
            .data_mut()
            .chunks_exact_mut(4)
            .zip(black.chunks_exact(4))
            .zip(bytes.chunks_exact(4))
        {
            let alpha = 255
                - w[0]
                    .saturating_sub(b[0])
                    .max(w[1].saturating_sub(b[1]))
                    .max(w[2].saturating_sub(b[2]));
            dst.copy_from_slice(&[b[2].min(alpha), b[1].min(alpha), b[0].min(alpha), alpha]);
        }
        Some(out)
    })();
    SelectObject(dc, old);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(dc);
    result
}
