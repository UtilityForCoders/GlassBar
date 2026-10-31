# GlassBar

Dock en Rust que sustituye a la barra de tareas de Windows 10/11.

## Commits: Conventional Commits

Todos los commits siguen [Conventional Commits 1.0](https://www.conventionalcommits.org/es/v1.0.0/):

```
<tipo>(<ámbito opcional>): <descripción>

<cuerpo opcional>

<pie opcional>
```

- **Tipos**:

  | Tipo | Para qué |
  |---|---|
  | `feat` | Función nueva. |
  | `fix` | Corrección de un error. |
  | `perf` | Mejora de rendimiento. |
  | `refactor` | Cambio de código sin cambio de comportamiento. |
  | `style` | Formato, sin cambio de lógica. |
  | `test` | Tests. |
  | `docs` | Documentación. |
  | `build` | Cargo, dependencias o compilación. |
  | `ci` | GitHub Actions. |
  | `chore` | Mantenimiento que no encaja en los demás. |

- **Ámbito** (opcional), el módulo o la parte del dock: `feat(launcher): ...`, `fix(systray): ...`, `perf(bar): ...`.
- **Descripción** en español, en minúscula, en presente y sin punto final: `fix: el nombre de los iconos de la bandeja sale delante del panel`.
- **Cambio incompatible**: `!` tras el tipo (`feat!: ...`) o un pie `BREAKING CHANGE: ...`.
- Un commit por cambio lógico.
- Los títulos de los PR siguen el mismo formato, porque se fusionan con squash y el título del PR queda como commit en la rama de destino.

## Ramas: Git Flow

| Rama | Sale de | Se fusiona en | Para qué |
|---|---|---|---|
| `main` | — | — | Solo versiones publicadas. Cada fusión lleva su etiqueta `vX.Y.Z`. |
| `develop` | `main` | — | Integración: aquí llega todo el trabajo terminado. |
| `feature/<nombre>` | `develop` | `develop` | Funciones nuevas (`feature/estilos-por-situacion`). |
| `release/<X.Y.Z>` | `develop` | `main` y `develop` | Preparar una versión: subir la versión en `Cargo.toml`, últimos arreglos. |
| `hotfix/<X.Y.Z>` | `main` | `main` y `develop` | Arreglos urgentes sobre una versión publicada. |

- Los nombres de rama van en minúsculas y con guiones.
- Nunca se hace commit directo en `main` ni en `develop`: todo entra por PR.
- Los PR de `feature/*` van contra `develop`. Los de `release/*` y `hotfix/*`, contra `main`; después se fusiona también en `develop`.
- Antes de abrir un PR deben pasar `cargo fmt --check`, `cargo test` y `cargo build --release`, igual que en el CI.
- Las versiones siguen SemVer: `feat` sube la menor, `fix` y `perf` la de parche, y un cambio incompatible la mayor.
