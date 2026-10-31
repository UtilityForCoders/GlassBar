# Política de Privacidad de GlassBar

**GlassBar** funciona 100% de forma local en tu computadora:

1. **Sin telemetría ni rastreo:** GlassBar no recopila, almacena ni envía ninguna información personal, estadística de uso o datos a servidores externos.
2. **Sin conexión de red obligatoria:** La aplicación funciona completamente fuera de línea. La única conexión que hace es buscar actualizaciones (ver punto 6), y se puede desactivar.
3. **Acceso a procesos:** La detección de ventanas y aplicaciones abiertas se realiza en memoria local únicamente para mostrar los estados activos en la barra de tareas.
4. **Código abierto:** Todo el código fuente está disponible para auditoría y contribución bajo la licencia MIT.
5. **Configuración e historial locales:** Los programas anclados y las aplicaciones abiertas desde GlassBar se conservan únicamente en `%LOCALAPPDATA%\GlassBar\apps.json` y `recent.json`. Los estilos y los ajustes de actualizaciones, en `styles.json` y `updates.json`, en la misma carpeta. Estos archivos no se envían a ningún servidor.
6. **Actualizaciones:**
   - **Cuándo:** si está activado (lo está por defecto), GlassBar pregunta a GitHub por la última versión publicada poco después de arrancar y luego cada 6 horas.
   - **Qué se envía:** nada del equipo ni del uso. Solo una petición a `api.github.com` con la versión de GlassBar como identificación. Como en cualquier conexión, GitHub ve la dirección IP.
   - **Al actualizar:** el instalador se descarga de GitHub y se comprueba con su suma SHA-256 antes de ejecutarlo.
   - **Desactivarlo:** "Buscar actualizaciones", en el menú de GlassBar.
