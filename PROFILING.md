# Medir el rendimiento de GlassBar

Rama temporal para medir CPU y memoria. Sin `--features profile` la app compila
igual que siempre y las mediciones desaparecen del ejecutable.

Los resultados se guardan en la carpeta `glassbar-profile/`, dentro de la carpeta
desde la que se ejecuta el comando (la raíz del repo):

| Archivo        | Contenido |
|----------------|-----------|
| `bench.txt`    | Cuánto tarda y cuánta memoria pide cada trabajo, repetido muchas veces |
| `informe.txt`  | Uso real: CPU por estado del dock y por hilo, memoria, posibles fugas, secciones y mensajes |
| `muestras.csv` | Una fila por segundo: CPU, memoria, handles y objetos GDI/USER |

Cierra cualquier GlassBar abierto antes de empezar.

## 1. Banco de pruebas (unos 30 segundos)

```powershell
cargo run --release --features profile -- --bench
```

Se abren y se cierran solos el lanzador, el panel rápido, el panel de idioma y la
vista previa, y la app sale al terminar. Para que mida la vista previa, deja abierta
alguna ventana de un programa (por ejemplo, el Explorador).

## 2. Uso real (5 minutos)

```powershell
cargo run --release --features profile -- --profile-secs 300
```

La app sale sola a los 300 segundos. Mientras tanto, sigue este guion (los tiempos
son aproximados; el informe detecta solo en qué estado estaba el dock):

1. **Minutos 0–1:** no toques nada, con el dock oculto.
2. **Minuto 1–2:** deja el cursor sobre el dock para que quede visible, sin moverlo.
3. **Minuto 2–3:** pasa el cursor por los iconos, abre la vista previa de algún
   programa y cambia de ventana con Alt+Tab.
4. **Minuto 3–4:** abre el lanzador (tecla Windows), busca algo, desplázate y
   ciérralo. Repite 2 o 3 veces.
5. **Minuto 4–5:** abre el panel rápido, entra en la lista de Wi-Fi, en Bluetooth
   y en audio, mueve el volumen y ciérralo. Abre y cierra el panel de idioma.

## 3. Opcional: fugas de memoria (30 minutos o más)

```powershell
cargo run --release --features profile -- --profile-secs 1800
```

Úsala con normalidad. La línea `Tendencia` del informe muestra si la memoria, los
handles o los objetos GDI/USER crecen sin parar.

## Qué enviar

Pega o adjunta `glassbar-profile/bench.txt` y `glassbar-profile/informe.txt`
(y `muestras.csv` si hiciste la prueba larga). Indica también el modelo del
procesador, la escala de pantalla y si estabas con batería o enchufado.
