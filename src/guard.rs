//! Vigilante: un segundo proceso de GlassBar que espera a que el principal termine
//! y vuelve a mostrar la barra nativa. Así no se queda oculta si GlassBar se cierra
//! a la fuerza (Administrador de tareas, fallo), cuando su propia limpieza no llega a ejecutarse.
use std::process::Command;
use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};

const FLAG: &str = "--guard";

/// Lanza el vigilante de este proceso. Si no se puede, GlassBar sigue igual que antes.
pub fn spawn() {
    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .args([FLAG, &std::process::id().to_string()])
            .spawn();
        if let Ok(child) = child {
            crate::profile::set_guard(child.id());
        }
    }
}

/// Si se pidió el modo vigilante, espera al proceso indicado, restaura la barra nativa
/// y devuelve `true`; el llamador debe terminar sin iniciar la barra.
pub fn run(args: &[String]) -> bool {
    let Some(pid) = watched_pid(args) else {
        return false;
    };
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            if WaitForSingleObject(process, INFINITE) == WAIT_FAILED {
                let _ = CloseHandle(process);
                return true;
            }
            let _ = CloseHandle(process);
        }
    }
    // Tras un cierre normal ya está visible y esto no cambia nada.
    crate::shell::restore_native();
    true
}

fn watched_pid(args: &[String]) -> Option<u32> {
    let at = args.iter().position(|arg| arg == FLAG)?;
    args.get(at + 1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_mode_needs_the_flag_and_a_process_id() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            watched_pid(&args(&["glassbar.exe", "--guard", "4242"])),
            Some(4242)
        );
        assert_eq!(watched_pid(&args(&["glassbar.exe"])), None);
        assert_eq!(watched_pid(&args(&["glassbar.exe", "--replace"])), None);
        assert_eq!(watched_pid(&args(&["glassbar.exe", "--guard"])), None);
        assert_eq!(watched_pid(&args(&["glassbar.exe", "--guard", "x"])), None);
    }
}
