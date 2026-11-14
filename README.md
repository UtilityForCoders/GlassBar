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

Como en la barra de Windows, **Mayús+clic** o el **clic central** sobre una app abren otra instancia, y **Ctrl+Mayús+clic** la abre como administrador (Windows pide confirmación).

La franja fina del extremo derecho del dock, después del reloj, es **Mostrar escritorio**: un clic minimiza todas las ventanas y otro las devuelve, igual que Win+D.

Las apps de Microsoft Store se anclan por su identificador de aplicación. El clic derecho en un hueco del dock no abre nada: los ajustes están en la configuración (ver Configuración). La selección se conserva en `%LOCALAPPDATA%\GlassBar\apps.json`. Los accesos directos mantienen su icono, argumentos y directorio de trabajo. No se descargan ni recrean logos. Los archivos que ya no existen muestran el icono genérico de Windows.

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

Windows no deja leer los iconos de la bandeja de Explorer, así que GlassBar los recibe directamente, como RetroBar: crea su propia ventana de bandeja, en un hilo propio, delante de la de Explorer y con su mismo tamaño, guarda cada icono que registran los programas y reenvía el mensaje a Explorer, cuya bandeja sigue igual. Al abrirse pide a los programas que vuelvan a registrar sus iconos (y otra vez a los 2 s, o si la bandeja de Explorer se pone delante, como mucho una vez por minuto), y al cerrarse, que se los den otra vez a Explorer. Si no puede ponerla delante, el panel se abre igual y avisa de que no hay apps en segundo plano; GlassBar ya no abre el panel nativo de Explorer, que solo se podía pulsar con trucos que cada actualización de Windows rompía. Mientras el programa abre su menú, el dock no se oculta. Para depurar, con la variable de entorno `GLASSBAR_TRAY_LOG=1` lo que manda cada programa y los clics que recibe quedan en `%TEMP%\glassbar-tray.log`; sin ella no se escribe nada, porque los nombres de los iconos pueden llevar datos personales.

## Avisos de las apps

Cuando un programa pide atención (un mensaje nuevo en un chat, una descarga terminada, un diálogo esperando respuesta), su icono se marca como en la barra de Windows 11: fondo naranja tenue y, en lugar del punto de app abierta, una línea naranja más ancha. Si el dock está escondido, se asoma 3 s para avisar, salvo con una app a pantalla completa. El aviso dura hasta que se activa esa ventana o se cierra, aunque haya dejado de parpadear, y un clic en el icono lleva justo a la ventana que lo pidió.

GlassBar recibe estos avisos del Shell de Windows (`RegisterShellHookWindow`), los mismos que recibe su barra. Los iconos superpuestos (el contador de mensajes de algunas apps) y la barra de progreso en el icono no se muestran: Windows solo se los manda a la barra de Explorer y no deja que otro programa ocupe su lugar mientras Explorer está abierto.

## Barra superior

En la configuración, **Controles en una barra superior** separa el dock al estilo de macOS: abajo quedan solo las apps y arriba, en una franja fina de 28 píxeles lógicos en el monitor principal, todo lo demás.

- **A la izquierda:** el logo de Inicio (abre el lanzador; con clic derecho, el menú de Windows como Win+X) y el nombre de la app activa: la descripción de su ejecutable ("Google Chrome"), el título de la ventana en las apps de Microsoft Store o "Escritorio".
- **En el centro:** nada; queda libre para una isla como Notch Isla y el nombre de la app se recorta antes de llegar. Las ventanas "siempre encima" de otros programas que tocan la barra (Notch Isla, un vídeo flotante) quedan por encima de ella, así se ven enteras y reciben sus clics, sin que la barra tenga ningún hueco.
- **A la derecha:** la flecha de las apps en segundo plano (hacia abajo, porque su panel se abre debajo), el centro de control: el idioma, la red, el volumen y la batería forman un solo botón que abre el panel rápido (la rueda encima cambia el volumen), y desde su botón del idioma, en el pie, se pasa al panel de idioma, el día y la hora ("vie 13 nov  22:28") y una franja fina para mostrar el escritorio.

Los paneles se abren debajo de la barra, sin que el dock se asome. La barra se registra como barra de aplicación en el borde superior, así que las ventanas maximizadas empiezan debajo de ella; como en Windows las pestañas y los botones de las ventanas están arriba, no se esconde sola. Se aparta con una app a pantalla completa. Sigue la apariencia del dock (con Desenfoque y Acrílico usa el cristal normal, sin desenfoque) y se guarda en `styles.json`. Con la barra activada, Win+B abre directamente el panel de las apps en segundo plano.

Con un panel abierto (apps en segundo plano, panel rápido, idioma o calendario), las flechas izquierda y derecha pasan al panel del botón vecino, como en una barra de menús, dando la vuelta al llegar a un extremo. Arriba y abajo recorren lo de dentro: los iconos de la bandeja, los botones y barras del panel rápido, los teclados o los meses del calendario. Enter o espacio pulsan lo resaltado; en la bandeja, la tecla de menú (o Mayús+F10) abre el menú del icono, y en una barra del panel rápido izquierda y derecha la mueven. Funciona igual con los paneles abiertos desde el dock.

## Varios monitores

El dock aparece en el monitor de cuyo borde inferior se acerca el cursor, como si cada pantalla tuviera el suyo: se esconde del monitor anterior y sube desde el borde del nuevo, con la escala de esa pantalla. No cambia de monitor mientras hay un panel o un menú abierto, o se está arrastrando un icono. La tecla Windows abre el lanzador en el monitor donde está el cursor. Los paneles, la vista previa y el brillo del panel rápido siguen al dock. Si se desconecta el monitor donde estaba, pasa al principal.

## Escritorios virtuales

El dock muestra las apps abiertas en el escritorio actual, como la barra de Windows: al cambiar de escritorio (Ctrl+Win+← o →, o desde la vista de tareas), las del anterior desaparecen y aparecen las del nuevo. El nombre del escritorio al que se pasa lo muestra Windows.

## Idioma del teclado

Con más de un teclado instalado, entre la flecha de iconos ocultos y el estado aparece el idioma de la ventana activa en tres letras ("ESP") y, debajo, la distribución si no es la normal de ese idioma ("INTL" para Estados Unidos-Internacional), como en la barra de Windows. Se actualiza al cambiar de ventana y con los atajos de idioma. El globo muestra el nombre completo. Un clic abre el panel de idioma de GlassBar, con el estilo del panel rápido: un teclado por fila (abreviatura, nombre y una marca en el actual) y **Más configuración de idioma**. El elegido se aplica a la ventana que estaba activa; también se recorre con las flechas y Enter, y Escape o un clic fuera lo cierran. El panel rápido lleva también un botón con el idioma en su pie, junto a **Más opciones**, que abre este mismo panel.

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
- **Luz nocturna**, **Modo avión** y **Ahorro de batería:** una fila de botones con su nombre debajo, como en Windows 11; encendidos llevan el color de énfasis. Windows no tiene una API pública para ninguno, así que cada uno usa lo mismo que su configuración: la luz nocturna, el estado que Windows guarda en el registro (CloudStore), que aplica al momento; el modo avión, la interfaz interna del servicio de radios (`IRadioManager`); y el ahorro de batería, el umbral del plan de energía (al encenderlo pasa al 100 % y al apagarlo vuelve al que había). El ahorro solo se puede encender con batería y sin el cargador; si no, aparece atenuado.
- **Brillo:** barra para la pantalla donde está el dock. Si es la integrada del portátil se usa WMI; si es un monitor externo, DDC/CI, el mismo canal que usa su menú (algunos monitores lo traen desactivado en sus ajustes). Si ninguna vía responde, la barra aparece como no disponible en lugar de cambiar el brillo de otra pantalla.
- **Volumen:** barra de la salida predeterminada; el icono del altavoz silencia o quita el silencio.
- **Salida de audio:** la flecha **›** junto al volumen abre la lista de salidas (altavoces, auriculares…), con la predeterminada marcada; un clic la cambia. Debajo aparecen las aplicaciones que están usando el audio, con su icono y la salida que usan; un clic despliega las salidas para elegir otra o volver a la predeterminada, y Windows lo recuerda para la próxima vez. Windows no ofrece una API pública para esto: GlassBar usa las interfaces internas de su configuración de sonido (`IPolicyConfig` y `AudioPolicyConfig`, Windows 10 1803 o posterior), como EarTrumpet. Si no están, la lista lo indica. **Más configuración de sonido** abre "Volumen y dispositivos de aplicaciones".
- **Pie:** nivel de batería (si la hay) y **Más opciones**, que abre el panel de Windows para lo demás.

Los botones activos, las barras, la línea del campo de contraseña y las selecciones de texto usan el color de énfasis de Windows (Configuración → Personalización → Colores), con los mismos tonos que Windows usa en modo oscuro: el oscuro para los botones activos y el claro para barras y selecciones. Si se cambia el color, GlassBar lo aplica al momento. Las barras se arrastran o se mueven con la rueda de 2 % en 2 %. Leer y cambiar las radios y el brillo puede tardar, así que se hace en un hilo aparte y el panel no se bloquea. Escape o un clic fuera lo cierran.

## Tecla Windows

Mientras GlassBar está abierto, pulsar y soltar la tecla Windows sola muestra u oculta este lanzador en lugar del Inicio nativo. Los atajos que Windows manda a su barra los atiende GlassBar, porque con la barra nativa oculta no harían nada visible:

- **Win+1…9 y Win+0:** la app en esa posición del dock (anclados y luego abiertos; Win+0 es la décima). Si no está abierta, la abre; si lo está, la trae al frente, y si ya está delante con varias ventanas, pasa a la siguiente. Con **Mayús** abre otra instancia, con **Ctrl+Mayús** la abre como administrador y con **Alt** abre su menú.
- **Win+T:** recorre el dock con el teclado desde el primer programa; **Win+B**, desde la flecha de iconos ocultos. Las flechas, Tab, Inicio y Fin mueven el resaltado; Enter o espacio hacen lo mismo que un clic (con Mayús, otra instancia; con Ctrl+Mayús, como administrador); la tecla de menú o Mayús+F10 abren el menú de la app, y Escape devuelve el foco a la ventana de antes.
- **Win+A:** el panel rápido. **Win+N** y **Win+Alt+D:** el calendario.

Las demás combinaciones (Win+E, Win+D, Win+L, etc.) siguen llegando a Windows. Con una ventana de administrador delante, Windows no deja que GlassBar vea las teclas y los atajos van a la barra nativa. Para impedir el Inicio nativo, GlassBar intercala una tecla sin asignar (`0xE8`) antes de soltar Win, de modo que Windows lo trata como una combinación. Al salir, se retira el gancho de teclado y la tecla vuelve a su comportamiento normal.

Volver a ejecutar GlassBar mientras ya está abierto muestra u oculta este lanzador sin crear otra instancia. `--replace` cierra la instancia anterior de forma normal antes de iniciar la nueva.

## Configuración

El clic derecho en el icono de GlassBar, en el panel de apps en segundo plano (o en la bandeja de Windows si la propia no está), abre su menú con **Configuración** y **Salir de GlassBar**. **Configuración** abre la ventana de configuración, centrada en el monitor del dock y con el estilo de los demás paneles:

- **Dock:** ocultar el dock automáticamente, controles en una barra superior e iniciar con Windows, con interruptores.
- **Apariencia según lo que hay en pantalla:** una fila por situación (ver Estilos) con su icono, un interruptor para activarla (el escritorio siempre cuenta) y un selector con flechas que pasa por Normal, Opaca, Transparente, Desenfoque y Acrílico. Debajo, la barra de la **opacidad de los menús**.
- **GlassBar:** buscar actualizaciones o instalar la nueva, y salir de GlassBar.

Los cambios se aplican y se guardan al momento. La ventana se mueve arrastrando su cabecera, sin salirse del área de trabajo (no tapa la barra superior), y al volver a abrirla sale donde se dejó mientras GlassBar siga abierto. Escape, la ✕ o un clic fuera la cierran. El clic derecho en un hueco del dock o de la barra superior ya no abre ningún menú.

## Estilos

Como en TranslucentTB, el aspecto del dock cambia según lo que pasa en pantalla. La configuración tiene una fila por situación, de menor a mayor prioridad:

- **Escritorio**: el aspecto de base, siempre activo.
- **Ventana visible**: hay una ventana sin minimizar en el monitor del dock.
- **Ventana maximizada**: la ventana de delante está maximizada. Una maximizada tapada por otra ventana no cuenta, y entonces vale Ventana visible.
- **Inicio abierto**: el lanzador de GlassBar está abierto.
- **Búsqueda abierta**: se escribe en el buscador del lanzador o está abierta la búsqueda de Windows (Win+S).
- **Vista de tareas abierta**: Win+Tab.

En cada una se elige la apariencia: **Normal** (el cristal de siempre), **Opaca**, **Transparente**, **Desenfoque** o **Acrílico**. Las dos últimas usan el desenfoque de Windows recortado a la forma del dock. Si se dan varias situaciones, gana la activada de mayor prioridad. Elegir una apariencia activa la situación, y su interruptor la quita sin perder la apariencia elegida. Por defecto solo cuenta el Escritorio con el aspecto Normal. Los ajustes se guardan en `%LOCALAPPDATA%\GlassBar\styles.json`.

## Comportamiento

Por defecto el dock se esconde solo. En la configuración, desactivar **Ocultar el dock automáticamente** lo deja siempre a la vista: entonces se registra como barra de aplicación en el borde inferior de su monitor, Windows le reserva su sitio y las ventanas maximizadas terminan encima de él. En este modo el dock no cambia de monitor (con él se movería el sitio reservado) y se aparta con una app a pantalla completa. Se guarda en `styles.json`.

Con el ocultar automático, acerca el cursor a los últimos 6 píxeles lógicos del borde inferior de cualquier monitor para desplegar la barra en él. Al alejarlo, espera 300 ms y se oculta con una animación suave sin rebote. El margen entre la barra y el borde también permite mantenerla abierta. Se oculta por completo, incluso si activaste el borde sin entrar en ella.

La barra inicia con el Explorador si no hay configuración guardada. El ancho se adapta a los programas anclados; si supera la pantalla, se reduce la escala. La hora usa formato de 24 horas; la fecha, el formato regional. Los indicadores de ejecución comparan nombres de ejecutables; los accesos web y algunas aplicaciones empaquetadas pueden no mostrar indicador.

La transparencia usa composición alfa real; el desenfoque de fondo solo aparece con las apariencias Desenfoque y Acrílico (ver Estilos). Mientras GlassBar está abierto, oculta las barras nativas del monitor principal y los secundarios. Un evento de Windows vuelve a ocultarlas cuando Explorer intenta mostrarlas; la comprobación periódica también cubre reinicios de Explorer. Al salir normalmente, se quita el detector y se restaura la barra original. Oculta, la barra nativa seguiría reservando su franja inferior y las ventanas maximizadas dejarían un hueco; por eso, si su ocultar automático está desactivado, GlassBar lo activa mientras está abierto y lo desactiva al salir. Lo apunta en `%LOCALAPPDATA%\GlassBar\native-autohide.txt` para devolverlo aunque se cierre a la fuerza; si ya estaba activado, no lo toca. Un segundo proceso `glassbar.exe` (el vigilante) espera a que GlassBar termine y vuelve a mostrar la barra nativa, incluso si se cerró a la fuerza o falló.

El dock se pone por encima de las demás ventanas "siempre encima" al mostrarse y, mientras está a la vista, cada vez que cambia la ventana activa. Si la que se activa también es "siempre encima" (un vídeo flotante, "Siempre visible" de PowerToys), no se le quita el sitio. Con una app a pantalla completa en su monitor no aparece al llevar el ratón al borde; la tecla Windows sigue abriendo el lanzador. Cuenta como pantalla completa la ventana activa que cubre el monitor sin barra de título, un juego a pantalla completa exclusiva o el modo presentación de Windows (aunque no tengan el foco) y, en el modo siempre visible, lo que Windows avisa a su barra de aplicación.

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

GlassBar busca versiones nuevas poco después de arrancar y luego cada 6 horas. Si hay una, avisa con una notificación de Windows; un clic en ella, o en **Actualizar a vX.Y.Z** de la configuración, la instala:

1. Descarga el instalador.
2. Comprueba su suma SHA-256.
3. Lo ejecuta en silencio y GlassBar se vuelve a abrir solo.

**Buscar actualizaciones**, en ese mismo menú, mira ya la última versión publicada y avisa también si no hay ninguna nueva. Ver [PRIVACY.md](PRIVACY.md).

Las versiones se publican solas en [Releases](https://github.com/UtilityForCoders/GlassBar/releases). Cada una trae `glassbar-X.Y.Z-x64.exe` (el programa, sin instalar), `glassbar-setup-X.Y.Z-x64.exe` (el instalador, para el usuario actual) y `SHA256SUMS.txt` para comprobarlos.

Licencia MIT.
