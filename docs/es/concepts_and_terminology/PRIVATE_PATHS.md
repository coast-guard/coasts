# Rutas Privadas

Cuando varias instancias de Coast comparten la misma raíz de proyecto, comparten los mismos archivos — y los mismos inodos. Normalmente ese es el objetivo: los cambios de archivos en el host aparecen dentro de Coast al instante porque ambos lados ven el mismo sistema de archivos. Pero algunas herramientas escriben estado por proceso en el espacio de trabajo que asume acceso exclusivo, y esa suposición deja de cumplirse cuando dos instancias comparten el mismo montaje.

## El Problema

Considera Next.js 16, que adquiere un bloqueo exclusivo en `.next/dev/lock` mediante `flock(fd, LOCK_EX)` cuando se inicia el servidor de desarrollo. `flock` es un mecanismo del kernel a nivel de inodo — no le importan los espacios de nombres de montaje, los límites de contenedores ni las rutas de bind mount. Si dos procesos en dos contenedores Coast diferentes apuntan ambos al mismo inodo de `.next/dev/lock` (porque comparten el mismo bind mount del host), el segundo proceso ve el bloqueo del primero y se niega a iniciarse:

```text
⨯ Another next dev server is already running.

- Local: http://localhost:3000
- PID: 1361
- Dir: /workspace/frontend
```

La misma categoría de conflicto se aplica a:

- Bloqueos consultivos `flock` / `fcntl` (Next.js, Turbopack, Cargo, Gradle)
- Archivos PID (muchos demonios escriben un archivo PID y lo comprueban al iniciarse)
- Cachés de compilación que asumen acceso de un solo escritor (Webpack, Vite, esbuild)

El aislamiento del espacio de nombres de montaje (`unshare`) no ayuda aquí. Los espacios de nombres de montaje controlan qué puntos de montaje puede ver un proceso, pero `flock` opera sobre el propio inodo. Dos procesos que ven el mismo inodo a través de diferentes rutas de montaje seguirán entrando en conflicto.

## La Solución

El campo `private_paths` del Coastfile declara directorios relativos al espacio de trabajo que deben ser por instancia. Cada instancia de Coast obtiene su propio bind mount aislado para estas rutas, respaldado por un directorio por instancia en el propio sistema de archivos del contenedor.

```toml
[coast]
name = "my-app"
private_paths = ["frontend/.next"]
```

Después de que Coast monte `/workspace` con propagación compartida, aplica un bind mount adicional para cada ruta privada:

```text
mkdir -p /coast-private/frontend/.next /workspace/frontend/.next
mount --bind /coast-private/frontend/.next /workspace/frontend/.next
```

`/coast-private/` vive en la capa escribible del contenedor DinD — no en el bind mount compartido del host — así que cada instancia obtiene naturalmente inodos separados. El archivo de bloqueo en `dev-1` vive en un inodo diferente al archivo de bloqueo en `dev-2`, y el conflicto desaparece.

## Cómo Funciona

Los montajes de rutas privadas se aplican en cada punto del ciclo de vida de Coast donde `/workspace` se monta o se vuelve a montar:

1. **`coast run`** — después del `mount --bind /host-project /workspace && mount --make-rshared /workspace` inicial, se montan las rutas privadas.
2. **`coast start`** — después de volver a aplicar el bind mount del espacio de trabajo al reiniciar el contenedor.
3. **`coast assign`** — después de desmontar y volver a enlazar `/workspace` a un directorio worktree.
4. **`coast unassign`** — después de revertir `/workspace` de vuelta a la raíz del proyecto.

Los directorios privados persisten entre ciclos de stop/start (viven en el sistema de archivos del contenedor, no en el montaje compartido). En `coast assign` o `coast unassign`, los directorios privados se **vacían** para que los servidores de desarrollo recompilen a partir de los archivos fuente de la rama correcta en lugar de servir salidas de compilación obsoletas de una rama anterior. En `coast rm`, se destruyen junto con el contenedor.

## Cuándo Usarlo

Usa `private_paths` cuando una herramienta escriba estado por proceso o por instancia en un directorio del espacio de trabajo que entre en conflicto entre instancias concurrentes de Coast:

- **Bloqueos de archivos**: `.next/dev/lock`, `target/.cargo-lock` de Cargo, `.gradle/lock` de Gradle
- **Cachés de compilación**: `.next`, `.turbo`, `target/`, `.vite`
- **Archivos PID**: cualquier demonio que escriba un archivo PID en el espacio de trabajo

No uses `private_paths` para datos que necesiten compartirse entre instancias o ser visibles en el host. Si necesitas datos aislados persistentes administrados por Docker (como volúmenes de base de datos), usa [volumes con `strategy = "isolated"`](../coastfiles/VOLUMES.md) en su lugar.

## Reglas de Validación

- Las rutas deben ser relativas (sin `/` inicial)
- Las rutas no deben contener componentes `..`
- Las rutas no deben solaparse — listar tanto `frontend/.next` como `frontend/.next/cache` es un error porque el primer montaje ocultaría al segundo

## Relación con los Volumes

`private_paths` y `[volumes]` resuelven distintos problemas de aislamiento:

| | `private_paths` | `[volumes]` |
|---|---|---|
| **Qué** | Directorios relativos al espacio de trabajo | Volúmenes con nombre administrados por Docker |
| **Dónde** | Dentro de `/workspace` | Rutas de montaje arbitrarias del contenedor |
| **Respaldado por** | Sistema de archivos local del contenedor (`/coast-private/`) | Volúmenes con nombre de Docker |
| **Aislamiento** | Siempre por instancia | Estrategia `isolated` o `shared` |
| **Sobrevive a `coast rm`** | No | Isolated: no. Shared: yes. |
| **Caso de uso** | Artefactos de compilación, archivos de bloqueo, cachés | Bases de datos, datos persistentes de la aplicación |

## Referencia de Configuración

Consulta [`private_paths`](../coastfiles/PROJECT.md) en la referencia de Coastfile para ver la sintaxis completa y ejemplos.
