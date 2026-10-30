//! Ritmo de las animaciones: un fotograma por cada refresco de la pantalla (60, 120,
//! 144 Hz…), solo mientras algo se mueve.
//!
//! Un temporizador de Windows de 16 ms no da 60 fps: avanza en pasos de 15,6 ms y
//! suele saltar cada dos (unos 32 fps). Aquí un hilo espera hasta el siguiente
//! refresco de DWM con un temporizador de alta resolución y entonces envía a cada
//! ventana que se anima el mismo `WM_TIMER` de animación que antes. Sin ventanas
//! animándose, el hilo duerme y no gasta nada.
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_TIMER};

struct Window {
    hwnd: isize,
    /// Cuándo se le mandó un fotograma que aún no se sacó de la cola: no se le manda
    /// otro, para que no se acumulen si la interfaz va lenta. Si un bucle modal de
    /// Windows (un menú) lo despachó sin pasar por `delivered`, pasado `STALE` se
    /// le manda igual para que la animación no se quede parada.
    sent: Option<Instant>,
}

const STALE: Duration = Duration::from_millis(250);

struct Ticker {
    windows: Mutex<Vec<Window>>,
    wake: Condvar,
    /// Id del temporizador que reciben las ventanas en `WM_TIMER`.
    timer_id: usize,
}

static TICKER: OnceLock<Ticker> = OnceLock::new();

/// Arranca el hilo; `timer_id` es el id de `WM_TIMER` con el que las ventanas ya
/// dibujaban sus animaciones.
pub fn init(timer_id: usize) {
    if TICKER
        .set(Ticker {
            windows: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            timer_id,
        })
        .is_ok()
    {
        std::thread::spawn(run);
    }
}

fn ticker() -> Option<&'static Ticker> {
    TICKER.get()
}

fn windows() -> Option<std::sync::MutexGuard<'static, Vec<Window>>> {
    ticker().map(|t| t.windows.lock().unwrap_or_else(|e| e.into_inner()))
}

/// La ventana empieza a animarse: recibirá un fotograma por refresco.
pub fn start(hwnd: HWND) {
    let Some(mut list) = windows() else {
        return;
    };
    let hwnd = hwnd.0 as isize;
    if !list.iter().any(|w| w.hwnd == hwnd) {
        list.push(Window { hwnd, sent: None });
        if let Some(t) = ticker() {
            t.wake.notify_one();
        }
    }
}

/// La ventana dejó de animarse.
pub fn stop(hwnd: HWND) {
    if let Some(mut list) = windows() {
        list.retain(|w| w.hwnd != hwnd.0 as isize);
    }
}

/// Se sacó de la cola un fotograma de `hwnd` (desde el bucle de mensajes): puede
/// recibir el siguiente.
pub fn delivered(hwnd: HWND, msg: u32, wparam: usize) {
    let Some(t) = ticker() else {
        return;
    };
    if msg != WM_TIMER || wparam != t.timer_id {
        return;
    }
    if let Some(mut list) = windows() {
        if let Some(window) = list.iter_mut().find(|w| w.hwnd == hwnd.0 as isize) {
            window.sent = None;
        }
    }
}

fn run() {
    let Some(t) = ticker() else {
        return;
    };
    let timer = Timer::new();
    loop {
        let targets: Vec<isize> = {
            let mut list = t.windows.lock().unwrap_or_else(|e| e.into_inner());
            while list.is_empty() {
                list = t.wake.wait(list).unwrap_or_else(|e| e.into_inner());
            }
            let now = Instant::now();
            list.iter_mut()
                .filter(|w| w.sent.is_none_or(|at| now.duration_since(at) > STALE))
                .map(|w| {
                    w.sent = Some(now);
                    w.hwnd
                })
                .collect()
        };
        for hwnd in targets {
            let posted = unsafe {
                PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_TIMER,
                    WPARAM(t.timer_id),
                    LPARAM(0),
                )
            };
            // La ventana ya no existe: deja de contar como animándose.
            if posted.is_err() {
                if let Ok(mut list) = t.windows.lock() {
                    list.retain(|w| w.hwnd != hwnd);
                }
            }
        }
        timer.wait(until_next_refresh());
    }
}

/// Tiempo hasta el siguiente refresco de la pantalla, en unidades de 100 ns. Lo
/// da DWM (cuándo fue el último y cada cuánto hay uno); si no, 60 Hz.
fn until_next_refresh() -> i64 {
    use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    const FALLBACK: i64 = 166_667;
    unsafe {
        let mut info = DWM_TIMING_INFO {
            cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
            ..Default::default()
        };
        let (mut now, mut frequency) = (0i64, 0i64);
        if DwmGetCompositionTimingInfo(HWND::default(), &mut info).is_err()
            || QueryPerformanceCounter(&mut now).is_err()
            || QueryPerformanceFrequency(&mut frequency).is_err()
            || frequency <= 0
        {
            return FALLBACK;
        }
        let ticks = next_refresh_in(
            now,
            info.qpcVBlank as i64,
            info.qpcRefreshPeriod as i64,
            frequency / 1000,
        );
        match ticks {
            Some(ticks) => ticks * 10_000_000 / frequency,
            None => FALLBACK,
        }
    }
}

/// Cuánto falta (en las mismas unidades) para el próximo refresco tras `now`,
/// sabiendo que hubo uno en `vblank` y que se repiten cada `period`. Si queda
/// menos de `margin`, se espera al siguiente para no enviar dos fotogramas juntos.
fn next_refresh_in(now: i64, vblank: i64, period: i64, margin: i64) -> Option<i64> {
    if period <= 0 {
        return None;
    }
    let since = (now - vblank).rem_euclid(period);
    let mut wait = period - since;
    if wait < margin {
        wait += period;
    }
    Some(wait)
}

/// Temporizador de espera de alta resolución (Windows 10 1803 o posterior); si no
/// existe, uno normal.
struct Timer(HANDLE);

impl Timer {
    fn new() -> Self {
        use windows::Win32::System::Threading::{
            CreateWaitableTimerExW, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS,
        };
        unsafe {
            let handle = CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
            .or_else(|_| CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0))
            .unwrap_or_default();
            Self(handle)
        }
    }

    /// Espera `hundred_ns` unidades de 100 ns.
    fn wait(&self, hundred_ns: i64) {
        use windows::Win32::System::Threading::{SetWaitableTimer, WaitForSingleObject, INFINITE};
        unsafe {
            // Negativo: tiempo relativo a ahora.
            let due = -hundred_ns.max(1);
            if self.0.is_invalid() || SetWaitableTimer(self.0, &due, 0, None, None, false).is_err()
            {
                std::thread::sleep(std::time::Duration::from_nanos(hundred_ns as u64 * 100));
                return;
            }
            WaitForSingleObject(self.0, INFINITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next_refresh_in;

    #[test]
    fn waits_for_the_next_refresh_after_now() {
        // Refresco cada 100 unidades; el último fue en 1000.
        assert_eq!(next_refresh_in(1030, 1000, 100, 5), Some(70));
        // El dato de DWM puede ser de un refresco anterior o posterior a `now`.
        assert_eq!(next_refresh_in(1230, 1000, 100, 5), Some(70));
        assert_eq!(next_refresh_in(970, 1000, 100, 5), Some(30));
        // Justo antes de un refresco no se envían dos fotogramas seguidos.
        assert_eq!(next_refresh_in(1098, 1000, 100, 5), Some(102));
        assert_eq!(next_refresh_in(1000, 1000, 0, 5), None);
    }
}
