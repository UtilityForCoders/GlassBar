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
  | `revert` | Deshace un commit anterior. |

- **Ámbito** (opcional), el módulo o la parte del dock: `feat(launcher): ...`, `fix(systray): ...`, `perf(bar): ...`.
- **Descripción** en español, en minúscula, en presente y sin punto final: `fix: el nombre de los iconos de la bandeja sale delante del panel`.
- **Cambio incompatible**: `!` tras el tipo (`feat!: ...`) o un pie `BREAKING CHANGE: ...`.
- Un commit por cambio lógico.
- Los títulos de los PR siguen el mismo formato: los PR contra `develop` se fusionan con squash y su título queda como commit.
- Los PR de `release/*` y `hotfix/*` contra `main` se fusionan con merge commit, no con squash: así `main` conserva los commits de cada PR y las notas de la versión los enumeran.

## Ramas: Git Flow

| Rama | Sale de | Se fusiona en | Para qué |
|---|---|---|---|
| `main` | — | — | Solo versiones publicadas. Cada fusión lleva su etiqueta `vX.Y.Z`. |
| `develop` | `main` | — | Integración: aquí llega todo el trabajo terminado. |
| `feature/<nombre>` | `develop` | `develop` | Funciones nuevas (`feature/estilos-por-situacion`). |
| `bugfix/<nombre>` | `develop` | `develop` | Arreglos de algo aún no publicado. |
| `release/<X.Y.Z>` | `develop` | `main` y `develop` | Preparar una versión: subir la versión en `Cargo.toml`, últimos arreglos. |
| `hotfix/<X.Y.Z>` | `main` | `main` y `develop` | Arreglos urgentes sobre una versión publicada. |

- Los nombres de rama van en minúsculas y con guiones.
- Nunca se hace commit directo en `main` ni en `develop`: todo entra por PR.
- Los PR de `feature/*` y `bugfix/*` van contra `develop`. Los de `release/*` y `hotfix/*`, contra `main`; después se fusiona también en `develop`.
- Antes de abrir un PR deben pasar `cargo fmt --check`, `cargo test` y `cargo build --release`, igual que en el CI.
- Las versiones siguen SemVer: `feat` sube la menor, `fix` y `perf` la de parche, y un cambio incompatible la mayor.

## CI/CD

| Workflow | Cuándo | Qué hace |
|---|---|---|
| `build.yml` (Compilar) | Push a `main` o `develop`, y cada PR | Formato, tests y compilación release; sube el `.exe` como artefacto. En los PR contra `main`, además exige una versión nueva en `Cargo.toml`. |
| `pull-request.yml` (Normas del PR) | Cada PR | El título sigue Conventional Commits y la rama va contra la base que manda Git Flow. |
| `release.yml` (Publicar versión) | Tras cada Compilar correcto de un push a `main` | Si `vX.Y.Z` (de `Cargo.toml`) aún no existe, crea la etiqueta y la versión en GitHub con el `.exe`, el instalador de Inno Setup y `SHA256SUMS.txt`. Las notas se generan con los PR incluidos. |

Dependabot (`.github/dependabot.yml`) abre sus PR contra `develop`, con mensajes `build(deps): ...` y `ci(deps): ...`.

## Publicar una versión

1. Desde `develop`, crear `release/X.Y.Z`.
2. Subir `version` en `Cargo.toml` y ejecutar `cargo build` para poner al día `Cargo.lock`. El CI compila con `--locked` y falla si no está al día.
3. Commit `chore(release): X.Y.Z`; abrir el PR contra `main` con ese mismo título.
4. Fusionarlo con merge commit. El CI compila `main` y `release.yml` publica `vX.Y.Z` sola.
5. Fusionar `main` en `develop` para que tenga la versión nueva.

Un `hotfix/X.Y.Z` sigue los mismos pasos, pero sale de `main`.
