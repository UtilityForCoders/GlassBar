# GlassBar

Dock minimalista para Windows 10/11: iconos reales con vista previa de sus ventanas, idioma del teclado, estado de red, volumen y batería, y hora y fecha en Segoe UI. Fondo plano con 51% de opacidad, borde tenue y sin sombras ni brillo superior. No incluye widgets.

## Agregar programas

Arrastra un `.exe`, `.lnk` o `.url` desde el Explorador o el escritorio y suéltalo sobre la barra. Puedes soltar varios archivos a la vez. Si está oculta, mantén el arrastre junto al borde inferior para desplegarla y después suelta sobre ella. Agregar un programa lo ancla sin ejecutarlo. Los documentos y carpetas se ignoran.

Clic derecho sobre un icono → **Quitar este programa**. La selección se conserva en `%LOCALAPPDATA%\GlassBar\apps.json`. Los accesos directos mantienen su icono, argumentos y directorio de trabajo. No se descargan ni recrean logos. Los archivos que ya no existen muestran el icono genérico de Windows.

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

Al dejar el cursor sobre un programa con una ventana abierta aparece, encima de su icono, un panel con el título de la ventana y su miniatura en vivo (la dibuja Windows con DWM, como en su barra). Al pasar a otro programa la vista previa cambia al momento; un clic trae la ventana al frente y la **✕** le pide que se cierre. El panel se mantiene mientras el cursor va del icono hacia él y el dock no se oculta mientras está abierto. Los programas sin ventana siguen mostrando su nombre.

## Idioma del teclado

Con más de un teclado instalado, entre la flecha de iconos ocultos y el estado aparece el idioma de la ventana activa en tres letras ("ESP") y, debajo, la distribución si no es la normal de ese idioma ("INTL" para Estados Unidos-Internacional), como en la barra de Windows. Se actualiza al cambiar de ventana y con los atajos de idioma. El globo muestra el nombre completo y un clic abre un menú con los teclados instalados para cambiar el de la ventana activa.

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

Las barras se arrastran o se mueven con la rueda de 2 % en 2 %. Leer y cambiar las radios y el brillo puede tardar, así que se hace en un hilo aparte y el panel no se bloquea. Escape o un clic fuera lo cierran.

## Tecla Windows

Mientras GlassBar está abierto, pulsar y soltar la tecla Windows sola muestra u oculta este lanzador en lugar del Inicio nativo. Las combinaciones (Win+E, Win+D, Win+L, etc.) siguen funcionando. Para impedir el Inicio nativo, GlassBar intercala una tecla sin asignar (`0xE8`) antes de soltar Win, de modo que Windows lo trata como una combinación. Al salir, se retira el gancho de teclado y la tecla vuelve a su comportamiento normal.

Volver a ejecutar GlassBar mientras ya está abierto muestra u oculta este lanzador sin crear otra instancia. `--replace` cierra la instancia anterior de forma normal antes de iniciar la nueva.

## Comportamiento

Acerca el cursor a los últimos 6 píxeles lógicos del borde inferior del monitor principal para desplegar la barra. Al alejarlo, espera 300 ms y se oculta con una animación suave sin rebote. El margen entre la barra y el borde también permite mantenerla abierta. Se oculta por completo, incluso si activaste el borde sin entrar en ella.

La barra inicia con el Explorador si no hay configuración guardada. El ancho se adapta a los programas anclados; si supera la pantalla, se reduce la escala. La hora usa formato de 24 horas; la fecha, el formato regional. Los indicadores de ejecución comparan nombres de ejecutables; los accesos web y algunas aplicaciones empaquetadas pueden no mostrar indicador.

La transparencia usa composición alfa real, sin desenfoque de fondo. Mientras GlassBar está abierto, oculta las barras nativas del monitor principal y los secundarios. Un evento de Windows vuelve a ocultarlas cuando Explorer intenta mostrarlas; la comprobación periódica también cubre reinicios de Explorer. Al salir normalmente, se quita el detector y se restaura la barra original. No se modifican las preferencias de la barra de Windows. Un segundo proceso `glassbar.exe` (el vigilante) espera a que GlassBar termine y vuelve a mostrar la barra nativa, incluso si se cerró a la fuerza o falló.

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

Licencia MIT.
