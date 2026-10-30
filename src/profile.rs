//! Perfilador de CPU y memoria, solo con `--features profile`. Sin esa opción todas
//! las funciones están vacías y el compilador las quita: la app normal no paga nada.
//!
//! Mide:
//! - Cada sección marcada con `scope`/`time`: llamadas, tiempo (media, p50, p95,
//!   p99, máximo) y memoria pedida por llamada.
//! - Cada segundo: CPU del proceso (repartida según lo que hacía el dock: oculto,
//!   visible, animando o con un panel abierto), memoria (working set, privada, heap
//!   de Rust), handles y objetos GDI/USER (para ver fugas) y el vigilante.
//! - Por hilo: CPU de la interfaz, del hilo de sistema y del resto.
//! - Mensajes de Windows recibidos por segundo.
//!
//! Escribe `glassbar-profile/informe.txt` y `glassbar-profile/muestras.csv` cada
//! 10 s y al salir; con `--bench`, `glassbar-profile/bench.txt`.

#[cfg(feature = "profile")]
pub use enabled::*;

#[cfg(not(feature = "profile"))]
pub use disabled::*;

#[cfg(not(feature = "profile"))]
#[allow(dead_code)]
mod disabled {
    pub struct Scope;
    impl Drop for Scope {
        #[inline(always)]
        fn drop(&mut self) {}
    }
    #[inline(always)]
    pub fn scope(_name: &'static str) -> Scope {
        Scope
    }
    #[inline(always)]
    pub fn time<R>(_name: &'static str, f: impl FnOnce() -> R) -> R {
        f()
    }
    #[inline(always)]
    pub fn message(_msg: u32, _wparam: usize) {}
    #[inline(always)]
    pub fn name_thread(_name: &'static str) {}
    #[inline(always)]
    pub fn set_guard(_pid: u32) {}
}

#[cfg(feature = "profile")]
mod enabled {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    // ---- Memoria pedida por Rust ----

    /// Asignador que cuenta cuánta memoria pide y libera el programa.
    pub struct Counting;

    static ALLOCS: AtomicU64 = AtomicU64::new(0);
    static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
    static LIVE: AtomicI64 = AtomicI64::new(0);
    static PEAK: AtomicI64 = AtomicI64::new(0);

    thread_local! {
        /// Asignaciones de este hilo (número y bytes), para atribuirlas a secciones.
        static THREAD: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }

    fn record_alloc(size: usize) {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(size as u64, Relaxed);
        let live = LIVE.fetch_add(size as i64, Relaxed) + size as i64;
        PEAK.fetch_max(live, Relaxed);
        let _ = THREAD.try_with(|t| {
            let (n, b) = t.get();
            t.set((n + 1, b + size as u64));
        });
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = System.alloc(layout);
            if !p.is_null() {
                record_alloc(layout.size());
            }
            p
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = System.alloc_zeroed(layout);
            if !p.is_null() {
                record_alloc(layout.size());
            }
            p
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout);
            LIVE.fetch_sub(layout.size() as i64, Relaxed);
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let p = System.realloc(ptr, layout, new_size);
            if !p.is_null() {
                LIVE.fetch_sub(layout.size() as i64, Relaxed);
                record_alloc(new_size);
            }
            p
        }
    }

    fn thread_allocs() -> (u64, u64) {
        THREAD.try_with(|t| t.get()).unwrap_or((0, 0))
    }

    // ---- Estado del perfilador ----

    #[derive(Default)]
    struct Stat {
        count: u64,
        total: Duration,
        max: Duration,
        /// Duraciones en ns, para percentiles (hasta un límite).
        samples: Vec<u64>,
        allocs: u64,
        alloc_bytes: u64,
    }

    #[derive(Clone, Copy, Default)]
    struct Memory {
        working_set: u64,
        private: u64,
        peak_working_set: u64,
        page_faults: u64,
        handles: u64,
        gdi: u64,
        user: u64,
        heap_live: i64,
        guard_working_set: u64,
    }

    struct Sample {
        at: f64,
        cpu_percent: f64,
        memory: Memory,
        allocs_per_s: f64,
        mode: &'static str,
    }

    #[derive(Default)]
    struct ModeStat {
        seconds: f64,
        cpu: f64,
        peak_percent: f64,
    }

    struct State {
        start: Instant,
        stats: HashMap<&'static str, Stat>,
        messages: HashMap<(u32, usize), u64>,
        gauges: Vec<(&'static str, String)>,
        threads: HashMap<u32, &'static str>,
        samples: Vec<Sample>,
        modes: HashMap<&'static str, ModeStat>,
        /// Tiempo pasado en cada modo desde la última muestra.
        pending_modes: HashMap<&'static str, Duration>,
        last_observe: Option<(Instant, &'static str)>,
        last_sample: Option<(Instant, u64, u64)>,
        first_memory: Option<Memory>,
        guard: Option<u32>,
        deadline: Option<Instant>,
        bench: bool,
        dir: PathBuf,
        csv_written: usize,
    }

    static STATE: Mutex<Option<State>> = Mutex::new(None);

    fn state() -> MutexGuard<'static, Option<State>> {
        let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let args: Vec<String> = std::env::args().collect();
            let seconds = args
                .iter()
                .position(|a| a == "--profile-secs")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<u64>().ok());
            let start = Instant::now();
            *guard = Some(State {
                start,
                stats: HashMap::new(),
                messages: HashMap::new(),
                gauges: Vec::new(),
                threads: HashMap::new(),
                samples: Vec::new(),
                modes: HashMap::new(),
                pending_modes: HashMap::new(),
                last_observe: None,
                last_sample: None,
                first_memory: None,
                guard: None,
                deadline: seconds.map(|s| start + Duration::from_secs(s)),
                bench: args.iter().any(|a| a == "--bench"),
                dir: std::env::current_dir()
                    .unwrap_or_default()
                    .join("glassbar-profile"),
                csv_written: 0,
            });
        }
        guard
    }

    pub fn is_bench() -> bool {
        state().as_ref().is_some_and(|s| s.bench)
    }

    // ---- Secciones ----

    pub struct Scope {
        name: &'static str,
        start: Instant,
        allocs: (u64, u64),
    }

    /// Mide desde aquí hasta el final del bloque.
    pub fn scope(name: &'static str) -> Scope {
        Scope {
            name,
            allocs: thread_allocs(),
            start: Instant::now(),
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            let elapsed = self.start.elapsed();
            let (n, b) = thread_allocs();
            record(
                self.name,
                elapsed,
                n - self.allocs.0,
                b.saturating_sub(self.allocs.1),
            );
        }
    }

    /// Mide una expresión.
    pub fn time<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
        let _scope = scope(name);
        f()
    }

    fn record(name: &'static str, elapsed: Duration, allocs: u64, bytes: u64) {
        let mut guard = state();
        let Some(state) = guard.as_mut() else {
            return;
        };
        let stat = state.stats.entry(name).or_default();
        stat.count += 1;
        stat.total += elapsed;
        stat.max = stat.max.max(elapsed);
        stat.allocs += allocs;
        stat.alloc_bytes += bytes;
        if stat.samples.len() < 500_000 {
            stat.samples.push(elapsed.as_nanos() as u64);
        }
    }

    /// Mensaje de Windows sacado de la cola (los temporizadores por su id).
    pub fn message(msg: u32, wparam: usize) {
        let key = if msg == 0x0113 {
            (msg, wparam)
        } else {
            (msg, 0)
        };
        if let Some(state) = state().as_mut() {
            *state.messages.entry(key).or_default() += 1;
        }
    }

    pub fn name_thread(name: &'static str) {
        let id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
        if let Some(state) = state().as_mut() {
            state.threads.insert(id, name);
        }
    }

    pub fn set_guard(pid: u32) {
        if let Some(state) = state().as_mut() {
            state.guard = Some(pid);
        }
    }

    /// Valor informativo (tamaños de cachés, superficies…), el último que se dio.
    pub fn gauge(name: &'static str, value: String) {
        if let Some(state) = state().as_mut() {
            match state.gauges.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => entry.1 = value,
                None => state.gauges.push((name, value)),
            }
        }
    }

    /// Qué hace el dock ahora (se llama muchas veces por segundo): la CPU de cada
    /// intervalo se reparte entre los modos según el tiempo pasado en cada uno.
    pub fn observe(mode: &'static str) {
        let now = Instant::now();
        if let Some(state) = state().as_mut() {
            if let Some((since, previous)) = state.last_observe {
                *state.pending_modes.entry(previous).or_default() += now - since;
            }
            state.last_observe = Some((now, mode));
        }
    }

    // ---- Lecturas del sistema ----

    fn filetime(t: windows::Win32::Foundation::FILETIME) -> u64 {
        ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
    }

    /// CPU del proceso (núcleo + usuario) en unidades de 100 ns.
    fn process_cpu() -> u64 {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let (mut a, mut b, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            let _ = GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user);
        }
        filetime(kernel) + filetime(user)
    }

    fn working_set_of(process: windows::Win32::Foundation::HANDLE) -> Option<(u64, u64, u64, u64)> {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        };
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        unsafe {
            GetProcessMemoryInfo(
                process,
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                counters.cb,
            )
            .ok()?;
        }
        Some((
            counters.WorkingSetSize as u64,
            counters.PrivateUsage as u64,
            counters.PeakWorkingSetSize as u64,
            counters.PageFaultCount as u64,
        ))
    }

    fn memory(guard: Option<u32>) -> Memory {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GetProcessHandleCount, OpenProcess, GR_GDIOBJECTS,
            GR_USEROBJECTS, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let process = GetCurrentProcess();
            let (working_set, private, peak_working_set, page_faults) =
                working_set_of(process).unwrap_or_default();
            let mut handles = 0;
            let _ = GetProcessHandleCount(process, &mut handles);
            let guard_working_set = guard
                .and_then(|pid| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok())
                .map(|h| {
                    let ws = working_set_of(h).map_or(0, |m| m.0);
                    let _ = CloseHandle(h);
                    ws
                })
                .unwrap_or(0);
            Memory {
                working_set,
                private,
                peak_working_set,
                page_faults,
                handles: handles as u64,
                gdi: GetGuiResources(process, GR_GDIOBJECTS) as u64,
                user: GetGuiResources(process, GR_USEROBJECTS) as u64,
                heap_live: LIVE.load(Relaxed),
                guard_working_set,
            }
        }
    }

    /// Memoria del proceso ahora: (working set, privada, heap de Rust), en bytes.
    pub fn memory_now() -> (u64, u64, i64) {
        let m = memory(None);
        (m.working_set, m.private, m.heap_live)
    }

    /// CPU de cada hilo vivo del proceso, en segundos.
    fn thread_cpu() -> Vec<(u32, f64)> {
        use windows::Win32::Foundation::{CloseHandle, FILETIME};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        use windows::Win32::System::Threading::{
            GetCurrentProcessId, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
        };
        let mut out = Vec::new();
        unsafe {
            let pid = GetCurrentProcessId();
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
                return out;
            };
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut more = Thread32First(snapshot, &mut entry).is_ok();
            while more {
                if entry.th32OwnerProcessID == pid {
                    if let Ok(thread) =
                        OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, entry.th32ThreadID)
                    {
                        let (mut a, mut b, mut kernel, mut user) = (
                            FILETIME::default(),
                            FILETIME::default(),
                            FILETIME::default(),
                            FILETIME::default(),
                        );
                        if GetThreadTimes(thread, &mut a, &mut b, &mut kernel, &mut user).is_ok() {
                            let seconds = (filetime(kernel) + filetime(user)) as f64 / 1e7;
                            out.push((entry.th32ThreadID, seconds));
                        }
                        let _ = CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        out
    }

    fn cores() -> f64 {
        std::thread::available_parallelism().map_or(1, |n| n.get()) as f64
    }

    /// Toma una muestra (una vez por segundo, desde el temporizador del reloj).
    /// Devuelve `true` cuando se cumplió el tiempo de `--profile-secs`.
    pub fn sample() -> bool {
        let _scope = scope("profile.sample (coste del propio perfilador)");
        let now = Instant::now();
        let cpu = process_cpu();
        let allocs = ALLOCS.load(Relaxed);
        let mut guard = state();
        let Some(state) = guard.as_mut() else {
            return false;
        };
        let memory = memory(state.guard);
        state.first_memory.get_or_insert(memory);
        if let Some((since, observed)) = state.last_observe {
            *state.pending_modes.entry(observed).or_default() += now - since;
            state.last_observe = Some((now, observed));
        }
        if let Some((before, before_cpu, before_allocs)) = state.last_sample {
            let wall = (now - before).as_secs_f64().max(1e-6);
            let used = (cpu - before_cpu) as f64 / 1e7;
            let percent = used / wall * 100.0;
            let total: f64 = state
                .pending_modes
                .values()
                .map(Duration::as_secs_f64)
                .sum::<f64>()
                .max(1e-6);
            let mut main_mode = "dock oculto";
            let mut longest = 0.0;
            for (&mode, spent) in &state.pending_modes {
                let share = spent.as_secs_f64() / total;
                let entry = state.modes.entry(mode).or_default();
                entry.seconds += wall * share;
                entry.cpu += used * share;
                if share > longest {
                    longest = share;
                    main_mode = mode;
                }
            }
            if let Some(entry) = state.modes.get_mut(main_mode) {
                entry.peak_percent = entry.peak_percent.max(percent);
            }
            state.samples.push(Sample {
                at: (now - state.start).as_secs_f64(),
                cpu_percent: percent,
                memory,
                allocs_per_s: (allocs - before_allocs) as f64 / wall,
                mode: main_mode,
            });
        }
        state.pending_modes.clear();
        state.last_sample = Some((now, cpu, allocs));
        if state.samples.len() % 10 == 0 {
            write_files(state);
        }
        state.deadline.is_some_and(|d| now >= d)
    }

    /// Escribe el informe final (al salir del bucle de mensajes).
    pub fn finish() {
        if let Some(state) = state().as_mut() {
            write_files(state);
        }
    }

    // ---- Banco de pruebas ----

    /// Repite `f` `times` veces y la mide con el nombre `name`.
    pub fn repeat(name: &'static str, times: usize, mut f: impl FnMut()) {
        for _ in 0..times {
            let _scope = scope(name);
            f();
        }
    }

    /// Escribe `bench.txt` con lo medido hasta ahora.
    pub fn write_bench() {
        if let Some(state) = state().as_mut() {
            let _ = std::fs::create_dir_all(&state.dir);
            let mut text = header(state, "banco de pruebas (--bench)");
            sections(&mut text, state, true);
            gauges(&mut text, state);
            let _ = std::fs::write(state.dir.join("bench.txt"), text);
        }
    }

    // ---- Informe ----

    fn mb(bytes: u64) -> f64 {
        bytes as f64 / (1024.0 * 1024.0)
    }

    fn percentile(sorted: &[u64], p: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() - 1) as f64 * p).round() as usize;
        sorted[index]
    }

    fn us(ns: u64) -> String {
        format!("{:.1}", ns as f64 / 1000.0)
    }

    fn header(state: &State, kind: &str) -> String {
        let mut text = String::new();
        let elapsed = state.start.elapsed().as_secs_f64();
        let _ = writeln!(text, "GlassBar {} — {kind}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(
            text,
            "Duración: {elapsed:.0} s · núcleos lógicos: {} · compilación: {}",
            cores(),
            if cfg!(debug_assertions) {
                "DEBUG (los tiempos no son representativos)"
            } else {
                "release"
            }
        );
        text.push('\n');
        text
    }

    fn sections(text: &mut String, state: &State, bench: bool) {
        let elapsed = state.start.elapsed().as_secs_f64().max(1e-6);
        let _ = writeln!(text, "== SECCIONES (ordenadas por tiempo total) ==");
        let _ = writeln!(
            text,
            "Los tiempos de una sección incluyen los de las secciones que llama. µs = microsegundos."
        );
        let _ = writeln!(
            text,
            "{:<44} {:>9} {:>8} {:>10} {:>7} {:>9} {:>9} {:>9} {:>9} {:>10} {:>9} {:>10}",
            "sección",
            "llamadas",
            "/s",
            "total ms",
            "%pared",
            "media µs",
            "p50 µs",
            "p95 µs",
            "p99 µs",
            "máx µs",
            "asig/ll",
            "KB/ll"
        );
        let mut rows: Vec<_> = state.stats.iter().collect();
        rows.sort_by(|a, b| b.1.total.cmp(&a.1.total));
        for (name, stat) in rows {
            if bench && !name.starts_with("bench.") && stat.count == 0 {
                continue;
            }
            let mut sorted = stat.samples.clone();
            sorted.sort_unstable();
            let count = stat.count.max(1);
            let _ = writeln!(
                text,
                "{:<44} {:>9} {:>8.2} {:>10.2} {:>6.3}% {:>9} {:>9} {:>9} {:>9} {:>10} {:>9.1} {:>10.2}",
                name,
                stat.count,
                stat.count as f64 / elapsed,
                stat.total.as_secs_f64() * 1000.0,
                stat.total.as_secs_f64() / elapsed * 100.0,
                us(stat.total.as_nanos() as u64 / count),
                us(percentile(&sorted, 0.5)),
                us(percentile(&sorted, 0.95)),
                us(percentile(&sorted, 0.99)),
                us(stat.max.as_nanos() as u64),
                stat.allocs as f64 / count as f64,
                stat.alloc_bytes as f64 / count as f64 / 1024.0,
            );
        }
        text.push('\n');
    }

    fn gauges(text: &mut String, state: &State) {
        if state.gauges.is_empty() {
            return;
        }
        let _ = writeln!(text, "== MEDIDORES (último valor) ==");
        for (name, value) in &state.gauges {
            let _ = writeln!(text, "{name:<44} {value}");
        }
        text.push('\n');
    }

    fn message_name(msg: u32) -> &'static str {
        match msg {
            0x0012 => "WM_QUIT",
            0x000F => "WM_PAINT",
            0x0113 => "WM_TIMER",
            0x0200 => "WM_MOUSEMOVE",
            0x0201 => "WM_LBUTTONDOWN",
            0x0202 => "WM_LBUTTONUP",
            0x0205 => "WM_RBUTTONUP",
            0x020A => "WM_MOUSEWHEEL",
            0x02A3 => "WM_MOUSELEAVE",
            0x0100 => "WM_KEYDOWN",
            0x0101 => "WM_KEYUP",
            0x0102 => "WM_CHAR",
            0x0118 => "WM_SYSTIMER",
            0x0060 => "WM_SYNCPAINT",
            0x001A => "WM_SETTINGCHANGE",
            0x0320 => "WM_DWMCOLORIZATIONCOLORCHANGED",
            0x8002 => "WM_SHOW_LAUNCHER (propio)",
            0x8003 => "WM_FOREGROUND (propio)",
            0x8004 => "WM_QUICK_UPDATE (propio)",
            m if m >= 0xC000 => "mensaje registrado (bandeja, etc.)",
            m if m >= 0x8000 => "WM_APP+n",
            _ => "otro",
        }
    }

    fn timer_name(id: usize) -> &'static str {
        match id {
            1 => "TIMER_ANIM (16 ms, animaciones)",
            2 => "TIMER_TICK (1 s, reloj/apps/estado)",
            3 => "TIMER_EDGE (40 ms, borde/autoocultar)",
            4 => "TIMER_TIP (globo)",
            _ => "otro temporizador",
        }
    }

    fn write_files(state: &mut State) {
        let _ = std::fs::create_dir_all(&state.dir);
        let elapsed = state.start.elapsed().as_secs_f64().max(1e-6);
        let mut text = header(state, "informe de rendimiento");

        // CPU.
        let cpu_total = process_cpu() as f64 / 1e7;
        let _ = writeln!(text, "== CPU ==");
        let _ = writeln!(
            text,
            "Total: {cpu_total:.2} s de CPU en {elapsed:.0} s → media {:.3}% de un núcleo ({:.3}% del equipo, como el Administrador de tareas)",
            cpu_total / elapsed * 100.0,
            cpu_total / elapsed * 100.0 / cores()
        );
        let mut percents: Vec<f64> = state.samples.iter().map(|s| s.cpu_percent).collect();
        percents.sort_by(|a, b| a.total_cmp(b));
        if !percents.is_empty() {
            let at = |p: f64| percents[((percents.len() - 1) as f64 * p).round() as usize];
            let _ = writeln!(
                text,
                "Por segundo (% de un núcleo): p50 {:.2} · p95 {:.2} · máx {:.2}",
                at(0.5),
                at(0.95),
                at(1.0)
            );
        }
        let _ = writeln!(text, "\nPor estado del dock:");
        let _ = writeln!(
            text,
            "{:<20} {:>10} {:>12} {:>16} {:>12}",
            "estado", "segundos", "CPU s", "media % núcleo", "pico %"
        );
        let mut modes: Vec<_> = state.modes.iter().collect();
        modes.sort_by(|a, b| b.1.seconds.total_cmp(&a.1.seconds));
        for (mode, stat) in modes {
            let _ = writeln!(
                text,
                "{:<20} {:>10.1} {:>12.3} {:>16.3} {:>12.2}",
                mode,
                stat.seconds,
                stat.cpu,
                stat.cpu / stat.seconds.max(1e-6) * 100.0,
                stat.peak_percent
            );
        }
        let _ = writeln!(text, "\nPor hilo (vivos ahora):");
        let mut named = 0.0;
        let mut threads = thread_cpu();
        threads.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (id, seconds) in &threads {
            named += seconds;
            let name = state
                .threads
                .get(id)
                .copied()
                .unwrap_or("Windows/COM/otros");
            let _ = writeln!(
                text,
                "  {:<28} id {:>6}  {:>8.3} s  ({:.1}%)",
                name,
                id,
                seconds,
                seconds / cpu_total.max(1e-6) * 100.0
            );
        }
        let _ = writeln!(
            text,
            "  {:<28}            {:>8.3} s  (hilos que ya terminaron, p. ej. carga del lanzador)",
            "terminados",
            (cpu_total - named).max(0.0)
        );
        text.push('\n');

        // Memoria.
        let now = memory(state.guard);
        let first = state.first_memory.unwrap_or(now);
        let series = |f: &dyn Fn(&Memory) -> f64| {
            let values: Vec<f64> = state.samples.iter().map(|s| f(&s.memory)).collect();
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(0.0, f64::max);
            let avg = values.iter().sum::<f64>() / values.len().max(1) as f64;
            (if min.is_finite() { min } else { 0.0 }, avg, max)
        };
        let _ = writeln!(text, "== MEMORIA ==");
        let _ = writeln!(
            text,
            "{:<34} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "", "inicio", "mín", "media", "máx", "ahora"
        );
        let mut row = |label: &str, f: &dyn Fn(&Memory) -> f64, unit: &str| {
            let (min, avg, max) = series(f);
            let _ = writeln!(
                text,
                "{:<34} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2}  {unit}",
                label,
                f(&first),
                min,
                avg,
                max,
                f(&now)
            );
        };
        row("Working set (RAM en uso)", &|m| mb(m.working_set), "MB");
        row("Memoria privada (commit)", &|m| mb(m.private), "MB");
        row(
            "Heap de Rust vivo",
            &|m| m.heap_live as f64 / 1048576.0,
            "MB",
        );
        row("Handles", &|m| m.handles as f64, "");
        row("Objetos GDI", &|m| m.gdi as f64, "");
        row("Objetos USER", &|m| m.user as f64, "");
        row(
            "Vigilante (proceso aparte) WS",
            &|m| mb(m.guard_working_set),
            "MB",
        );
        let _ = writeln!(
            text,
            "Pico de working set: {:.2} MB · pico del heap de Rust: {:.2} MB · fallos de página: {}",
            mb(now.peak_working_set),
            PEAK.load(Relaxed) as f64 / 1048576.0,
            now.page_faults
        );
        let allocs = ALLOCS.load(Relaxed);
        let _ = writeln!(
            text,
            "Asignaciones: {allocs} ({:.1}/s) · {:.2} MB pedidos en total ({:.1} KB/s)",
            allocs as f64 / elapsed,
            mb(ALLOC_BYTES.load(Relaxed)),
            ALLOC_BYTES.load(Relaxed) as f64 / elapsed / 1024.0
        );
        if state.samples.len() >= 20 {
            // Tendencia de la segunda mitad: si crece sin parar, hay una fuga.
            let half = &state.samples[state.samples.len() / 2..];
            let (a, b) = (&half[0], &half[half.len() - 1]);
            let minutes = ((b.at - a.at) / 60.0).max(1e-6);
            let _ = writeln!(
                text,
                "Tendencia (2.ª mitad): working set {:+.3} MB/min · privada {:+.3} MB/min · heap {:+.3} MB/min · handles {:+.1}/min · GDI {:+.1}/min · USER {:+.1}/min",
                (mb(b.memory.working_set) - mb(a.memory.working_set)) / minutes,
                (mb(b.memory.private) - mb(a.memory.private)) / minutes,
                (b.memory.heap_live - a.memory.heap_live) as f64 / 1048576.0 / minutes,
                (b.memory.handles as f64 - a.memory.handles as f64) / minutes,
                (b.memory.gdi as f64 - a.memory.gdi as f64) / minutes,
                (b.memory.user as f64 - a.memory.user as f64) / minutes,
            );
        }
        text.push('\n');

        gauges(&mut text, state);
        sections(&mut text, state, false);

        // Mensajes.
        let _ = writeln!(text, "== MENSAJES DE WINDOWS (sacados de la cola) ==");
        let mut messages: Vec<_> = state.messages.iter().collect();
        messages.sort_by(|a, b| b.1.cmp(a.1));
        for ((msg, id), count) in messages {
            let name = if *msg == 0x0113 {
                format!("WM_TIMER {}", timer_name(*id))
            } else {
                format!("{} (0x{msg:04X})", message_name(*msg))
            };
            let _ = writeln!(
                text,
                "{:<52} {:>9} {:>9.2}/s",
                name,
                count,
                *count as f64 / elapsed
            );
        }

        let _ = std::fs::write(state.dir.join("informe.txt"), text);

        // Serie temporal, añadiendo solo las muestras nuevas.
        let path = state.dir.join("muestras.csv");
        let mut csv = String::new();
        if state.csv_written == 0 {
            csv.push_str("segundo,estado,cpu_pct_nucleo,ws_mb,privada_mb,heap_mb,asig_por_s,handles,gdi,user,vigilante_mb\n");
        }
        for s in &state.samples[state.csv_written..] {
            let _ = writeln!(
                csv,
                "{:.1},{},{:.3},{:.3},{:.3},{:.3},{:.0},{},{},{},{:.3}",
                s.at,
                s.mode,
                s.cpu_percent,
                mb(s.memory.working_set),
                mb(s.memory.private),
                s.memory.heap_live as f64 / 1048576.0,
                s.allocs_per_s,
                s.memory.handles,
                s.memory.gdi,
                s.memory.user,
                mb(s.memory.guard_working_set)
            );
        }
        // La primera vez se empieza el archivo de cero (puede quedar uno de antes).
        let file = if state.csv_written == 0 {
            std::fs::File::create(&path)
        } else {
            std::fs::OpenOptions::new().append(true).open(&path)
        };
        state.csv_written = state.samples.len();
        use std::io::Write;
        if let Ok(mut file) = file {
            let _ = file.write_all(csv.as_bytes());
        }
    }
}
