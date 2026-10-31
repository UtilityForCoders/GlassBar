# GlassBar

Dock minimalista para Windows 10/11: iconos reales con vista previa de sus ventanas, idioma del teclado, estado de red, volumen y batería, y hora y fecha en Segoe UI. Fondo plano con 51% de opacidad, borde tenue y sin sombras ni brillo superior. No incluye widgets.

## Agregar programas

Arrastra un `.exe`, `.lnk` o `.url` desde el Explorador o el escritorio y suéltalo sobre la barra. Puedes soltar varios archivos a la vez. Si está oculta, mantén el arrastre junto al borde inferior para desplegarla y después suelta sobre ella. Agregar un programa lo ancla sin ejecutarlo. Los documentos y carpetas se ignoran.

Clic derecho sobre una app del dock, anclada o abierta, abre su menú encima de su icono. Es un menú propio dibujado como el de la barra de Windows 11: panel oscuro redondeado, títulos de sección en gris, un icono por opción (el propio de cada tarea, el de cada tipo de archivo o glifos de Segoe Fluent Icons) y resaltado al pasar el cursor. Entra con la misma animación que los demás paneles, se recorre con las flechas y Enter, y se cierra con Escape o un clic fuera. Contiene:

- **Su jump list:** lo mismo que muestra la barra de Windows, por el AppUserModelID de la app.
  - **Categorías propias y tareas:** por ejemplo "Más visitado", "Cerrado recientemente", "Nueva ventana" o "Nueva ventana de incógnito" en Chrome, y "Recent Folders" o "New Window" en VS Code. Se leen con la interfaz interna de la barra (`IInternalCustomDestinationList`, sin documentar), probando su versión de Windows 10/11 y la anterior, y cada tarea ejecuta el programa y los argumentos que guardó la app.
  - **Recientes o Frecuentes:** cuando la app los pide en su lista o no tiene lista propia. Se leen con la API pública (`IApplicationDocumentLists`), como las carpetas recientes del Explorador. Los archivos se abren con la propia app; las direcciones internas que guardan algunas apps (p. ej. `whatsapp://…`) no se muestran, igual que en Windows.
  - **Orden:** las tareas van siempre al final, como en Windows, y los títulos de más de 60 caracteres se recortan. Si la interfaz interna no está disponible, el menú se queda con Recientes o Frecuentes.
- **Su nombre:** la abre, o abre otra ventana si ya está abierta.
- **Anclar a la barra** o **Desanclar de la barra.**
- **Finalizar tarea,** si está activada en Windows (Configuración > Sistema > Para programadores), como en su barra. Termina a la fuerza el proceso de la app, como el Administrador de tareas, aunque no responda, y se pierde lo no guardado. En las apps de Store termina la app, no el marco que comparten todas. Nunca se ofrece para el Explorador, que es el escritorio.
- **Cerrar ventana** o **Cerrar todas las ventanas,** si tiene alguna abierta. Se les pide cerrarse como desde Windows, así que preguntan si hay cambios sin guardar.

Las apps de Microsoft Store se anclan por su identificador de aplicación. El clic derecho en el resto del dock abre el menú de GlassBar (iniciar con Windows y salir). La selección se conserva en `%LOCALAPPDATA%\GlassBar\apps.json`. Los accesos directos mantienen su icono, argumentos y directorio de trabajo. No se descargan ni recrean logos. Los archivos que ya no existen muestran el icono genérico de Windows.

## Lanzador propio

La cuadrícula, o la tecla Windows, abre el panel de GlassBar al estilo del Inicio de Windows 11, centrado sobre el dock. Tiene una barra de búsqueda y, debajo, una vista con desplazamiento:

- **Anclado:** los programas del dock.
- **Recientes:** los últimos programas abiertos, según el registro de uso de Windows (UserAssist) y el historial de GlassBar en `%LOCALAPPDATA%\GlassBar\recent.json`.
- **Más usadas:** los que más veces se han abierto según ese mismo registro, sin repetir anclados ni recientes.

Solo aparecen programas del catálogo (accesos directos del menú Inicio y aplicaciones instaladas). Los que Windows abre sin que lleguen a primer plano, como la barra de juegos, no cuentan. El resto del catálogo, también los programas de Microsoft Store, aparece al buscar. GlassBar solo lee el registro de uso, nunca lo modifica. El catálogo y los iconos de la vista inicial se preparan en segundo plano al iniciar GlassBar y se reutilizan en cada apertura.

El pie muestra el nombre de la cuenta y el botón de apagado. El panel tiene 96% de opacidad. Los iconos se obtienen del recurso original o de la fábrica de imágenes del Shell a alta resolución, y se reducen con un filtro de área sin ampliar los iconos pequeños.

Escribe para buscar; la búsqueda ignora mayúsculas y tildes, admite varias palabras y prioriza las coincidencias exactas. Enter abre el elemento seleccionado o el primer resultado. El campo admite cursor, Inicio/Fin, Retroceso/Supr, Ctrl+A, Ctrl+V y un botón para limpiar. Las flechas mueven la selección por la cuadrícula y entre secciones (izquierda/derecha editan el texto mientras no haya selección). La rueda y Re Pág/Av Pág desplazan la vista. Escape o un clic fuera cierran el panel.

El botón de encendido del pie abre un menú como el del Inicio de Windows 11 con **Bloquear**, **Suspender**, **Apagar** y **Reiniciar**, con los iconos del sistema (Segoe Fluent Icons). Aparece en 200 ms, desvaneciéndose y subiendo unos píxeles; si los efectos de animación de Windows están desactivados, aparece directamente. También se recorre con las flechas y Enter; Escape o un clic fuera lo cierran sin hacer nada. Apagar y reiniciar piden a Windows el cierre normal, sin forzar el de las aplicaciones abiertas.

## Vista previa de ventanas

Al dejar el cursor sobre un programa con ventanas abiertas aparece, encima de su icono, una tarjeta por ventana con su título y su miniatura en vivo (la dibuja Windows con DWM, como en su barra), en fila; si no caben en la pantalla se encogen. La tarjeta bajo el cursor se resalta y muestra su ✕. Al pasar a otro programa la vista previa cambia al momento; un clic trae la ventana al frente y la **✕** le pide que se cierre. El panel se mantiene mientras el cursor va del icono hacia él y el dock no se oculta mientras está abierto. Los programas sin ventana siguen mostrando su nombre.

## Apps en segundo plano

La flecha **^** del dock abre el panel de GlassBar con los iconos de la bandeja de los demás programas (Spotify, Discord, Steam, la seguridad de Windows…), en filas de cinco como en Windows 11. Al pasar el cursor se ve el nombre de cada uno; un clic, un doble clic o un clic derecho le llegan al programa igual que desde la bandeja de Windows, que abre su ventana o su menú. Escape o un clic fuera cierran el panel.

Windows no deja leer los iconos de la bandeja de Explorer, así que GlassBar los recibe directamente, como RetroBar: crea su propia ventana de bandeja, en un hilo propio, delante de la de Explorer y con su mismo tamaño, guarda cada icono que registran los programas y reenvía el mensaje a Explorer, cuya bandeja sigue igual. Al abrirse pide a los programas que vuelvan a registrar sus iconos (y otra vez a los 2 s, o si la bandeja de Explorer se pone delante, como mucho una vez por minuto), y al cerrarse, que se los den otra vez a Explorer. Si no puede, la flecha abre el panel nativo como antes. Mientras el programa abre su menú, el dock no se oculta. Para depurar, con la variable de entorno `GLASSBAR_TRAY_LOG=1` lo que manda cada programa y los clics que recibe quedan en `%TEMP%\glassbar-tray.log`; sin ella no se escribe nada, porque los nombres de los iconos pueden llevar datos personales.

## Idioma del teclado

Con más de un teclado instalado, entre la flecha de iconos ocultos y el estado aparece el idioma de la ventana activa en tres letras ("ESP") y, debajo, la distribución si no es la normal de ese idioma ("INTL" para Estados Unidos-Internacional), como en la barra de Windows. Se actualiza al cambiar de ventana y con los atajos de idioma. El globo muestra el nombre completo. Un clic abre el panel de idioma de GlassBar, con el estilo del panel rápido: un teclado por fila (abreviatura, nombre y una marca en el actual) y **Más configuración de idioma**. El elegido se aplica a la ventana que estaba activa; también se recorre con las flechas y Enter, y Escape o un clic fuera lo cierran.

## Hora y fecha

El reloj muestra la hora y, debajo, la fecha corta en el formato regional del usuario; al pasar el cursor, el globo muestra la fecha larga. Un clic abre la configuración de fecha y hora.

## Red, volumen y batería

Entre la flecha de iconos ocultos y la hora, el dock muestra el estado del sistema con los iconos de Windows 11 (Segoe Fluent Icons):

- **Red:** Wi-Fi o cable según el adaptador en uso (si hay los dos, el cable, como Windows); atenuado si Windows indica que no hay acceso a Internet, y un globo tachado sin conexión. La intensidad de la señal Wi-Fi no se muestra.
- **Volumen:** de cero a tres ondas según el nivel de la salida predeterminada, y una cruz si está silenciada. La rueda del ratón sobre el icono lo sube o lo baja de 2 % en 2 %.
- **Batería:** solo en equipos con batería; el nivel en once pasos, con rayo mientras está enchufada.

El estado se lee una vez por segundo. Al pasar el cursor se muestra el detalle (p. ej. "Volumen: 45 %").

Un clic en el grupo abre el panel rápido de GlassBar justo encima, con el estilo del lanzador:

- **Wi-Fi** y **Bluetooth:** botones para encenderlos y apagarlos (API de radios de Windows); el de Wi-Fi muestra el nombre de la red conectada.
- **Redes Wi-Fi:** la flecha **›** del botón de Wi-Fi abre la lista de redes cercanas (API WLAN), con la intensidad de la señal y un candado en las protegidas, la conectada primero y el resto por señal. Al abrirla se buscan redes y la lista se actualiza sola unos segundos después. Un clic conecta con una red guardada o desconecta la actual. En una red nueva, la fila se despliega para escribir la contraseña (WPA, WPA2 o WPA3 personal; las abiertas se conectan directamente): GlassBar crea su perfil y, si Windows no confirma la conexión en 20 s, lo borra y vuelve a pedir la contraseña con un aviso. Las redes de empresa (con usuario) y las WEP abren la lista de Windows. **Más configuración de Wi-Fi** abre la configuración de Windows.
- **Dispositivos Bluetooth:** la flecha del botón de Bluetooth abre la lista de dispositivos emparejados (clásicos y de bajo consumo), con su tipo, los conectados primero. Los de audio (auriculares, altavoces) se conectan y desconectan desde la lista, pidiéndoselo al controlador de audio Bluetooth; Windows no ofrece a las apps una forma de conectar los demás, que se conectan solos al encenderlos, así que su botón abre la configuración de Bluetooth.
- **Brillo:** barra para la pantalla donde está el dock (el monitor principal). Si es la integrada del portátil se usa WMI; si es un monitor externo, DDC/CI, el mismo canal que usa su menú (algunos monitores lo traen desactivado en sus ajustes). Si ninguna vía responde, la barra aparece como no disponible en lugar de cambiar el brillo de otra pantalla.
- **Volumen:** barra de la salida predeterminada; el icono del altavoz silencia o quita el silencio.
- **Salida de audio:** la flecha **›** junto al volumen abre la lista de salidas (altavoces, auriculares…), con la predeterminada marcada; un clic la cambia. Debajo aparecen las aplicaciones que están usando el audio, con su icono y la salida que usan; un clic despliega las salidas para elegir otra o volver a la predeterminada, y Windows lo recuerda para la próxima vez. Windows no ofrece una API pública para esto: GlassBar usa las interfaces internas de su configuración de sonido (`IPolicyConfig` y `AudioPolicyConfig`, Windows 10 1803 o posterior), como EarTrumpet. Si no están, la lista lo indica. **Más configuración de sonido** abre "Volumen y dispositivos de aplicaciones".
- **Pie:** nivel de batería (si la hay) y **Más opciones**, que abre el panel de Windows para lo demás.

Los botones activos, las barras, la línea del campo de contraseña y las selecciones de texto usan el color de énfasis de Windows (Configuración → Personalización → Colores), con los mismos tonos que Windows usa en modo oscuro: el oscuro para los botones activos y el claro para barras y selecciones. Si se cambia el color, GlassBar lo aplica al momento. Las barras se arrastran o se mueven con la rueda de 2 % en 2 %. Leer y cambiar las radios y el brillo puede tardar, así que se hace en un hilo aparte y el panel no se bloquea. Escape o un clic fuera lo cierran.

## Tecla Windows

Mientras GlassBar está abierto, pulsar y soltar la tecla Windows sola muestra u oculta este lanzador en lugar del Inicio nativo. Las combinaciones (Win+E, Win+D, Win+L, etc.) siguen funcionando. Para impedir el Inicio nativo, GlassBar intercala una tecla sin asignar (`0xE8`) antes de soltar Win, de modo que Windows lo trata como una combinación. Al salir, se retira el gancho de teclado y la tecla vuelve a su comportamiento normal.

Volver a ejecutar GlassBar mientras ya está abierto muestra u oculta este lanzador sin crear otra instancia. `--replace` cierra la instancia anterior de forma normal antes de iniciar la nueva.

## Estilos

Como en TranslucentTB, el aspecto del dock cambia según lo que pasa en pantalla. El menú de GlassBar (clic derecho en un hueco del dock o en su icono de la bandeja) tiene un submenú por situación, de menor a mayor prioridad:

- **Escritorio**: el aspecto de base, siempre activo.
- **Ventana visible**: hay una ventana sin minimizar en el monitor del dock.
- **Ventana maximizada**: la ventana de delante está maximizada. Una maximizada tapada por otra ventana no cuenta, y entonces vale Ventana visible.
- **Inicio abierto**: el lanzador de GlassBar está abierto.
- **Búsqueda abierta**: se escribe en el buscador del lanzador o está abierta la búsqueda de Windows (Win+S).
- **Vista de tareas abierta**: Win+Tab.

En cada una se elige la apariencia: **Normal** (el cristal de siempre), **Opaca**, **Transparente**, **Desenfoque** o **Acrílico**. Las dos últimas usan el desenfoque de Windows recortado a la forma del dock. Si se dan varias situaciones, gana la activada de mayor prioridad. Elegir una apariencia activa la situación, y **Activado** la quita sin perder la apariencia elegida. Por defecto solo cuenta el Escritorio con el aspecto Normal. Los ajustes se guardan en `%LOCALAPPDATA%\GlassBar\styles.json`.

## Comportamiento

Acerca el cursor a los últimos 6 píxeles lógicos del borde inferior del monitor principal para desplegar la barra. Al alejarlo, espera 300 ms y se oculta con una animación suave sin rebote. El margen entre la barra y el borde también permite mantenerla abierta. Se oculta por completo, incluso si activaste el borde sin entrar en ella.

La barra inicia con el Explorador si no hay configuración guardada. El ancho se adapta a los programas anclados; si supera la pantalla, se reduce la escala. La hora usa formato de 24 horas; la fecha, el formato regional. Los indicadores de ejecución comparan nombres de ejecutables; los accesos web y algunas aplicaciones empaquetadas pueden no mostrar indicador.

La transparencia usa composición alfa real; el desenfoque de fondo solo aparece con las apariencias Desenfoque y Acrílico (ver Estilos). Mientras GlassBar está abierto, oculta las barras nativas del monitor principal y los secundarios. Un evento de Windows vuelve a ocultarlas cuando Explorer intenta mostrarlas; la comprobación periódica también cubre reinicios de Explorer. Al salir normalmente, se quita el detector y se restaura la barra original. No se modifican las preferencias de la barra de Windows. Un segundo proceso `glassbar.exe` (el vigilante) espera a que GlassBar termine y vuelve a mostrar la barra nativa, incluso si se cerró a la fuerza o falló.

El dock vuelve a ponerse por encima de las demás ventanas cada vez que cambia la ventana activa. Con una app a pantalla completa (juego, vídeo, presentación) en su monitor, no aparece al llevar el ratón al borde; la tecla Windows sigue abriendo el lanzador.

## Desarrollo

Requiere Windows, Rust y las herramientas de compilación de Visual Studio.

```powershell
cargo run
cargo check --all-targets
cargo test --bin glassbar
cargo build --release
```

`cargo run --example preview` genera `dock_preview.bmp` sin iniciar la barra ni ocultar la barra nativa. Acepta rutas de programas como argumentos para probar distintas distribuciones sin modificar los programas guardados.

`cargo run --example preview -- --launcher` genera una vista previa del lanzador. `--launcher-power` muestra únicamente su confirmación de apagado; estas vistas previas no ejecutan programas ni apagan el equipo.

## Versiones

GlassBar busca versiones nuevas poco después de arrancar y luego cada 6 horas. Si hay una, avisa con una notificación de Windows; un clic en ella, o en **Actualizar a vX.Y.Z** del menú de GlassBar, la instala:

1. Descarga el instalador.
2. Comprueba su suma SHA-256.
3. Lo ejecuta en silencio y GlassBar se vuelve a abrir solo.

Se desactiva con **Buscar actualizaciones** en ese mismo menú. Ver [PRIVACY.md](PRIVACY.md).

Las versiones se publican solas en [Releases](https://github.com/UtilityForCoders/GlassBar/releases). Cada una trae `glassbar-X.Y.Z-x64.exe` (el programa, sin instalar), `glassbar-setup-X.Y.Z-x64.exe` (el instalador, para el usuario actual) y `SHA256SUMS.txt` para comprobarlos.

Licencia MIT.
