# Optimizaciones de Rendimiento

Coast está diseñado para que el cambio de rama sea rápido, pero en monorepos grandes el comportamiento predeterminado aún puede introducir latencia. Esta página cubre las palancas disponibles en tu Coastfile y, más importante, qué partes de `coast assign` afectan realmente.

## Por Qué Assign Puede Ser Lento

`coast assign` hace varias cosas al cambiar un Coast a un nuevo worktree:

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

Los mayores costos variables suelen ser el **bootstrap inicial de archivos ignorados por git**, los **reinicios de contenedores** y las **reconstrucciones de imágenes**. El diff opcional de rama usado para los disparadores de reconstrucción es mucho más barato, pero aun así puede acumularse si lo apuntas a conjuntos de disparadores amplios.

### Bootstrap de Archivos Ignorados por Git

Cuando se crea un worktree por primera vez, Coast inicializa (bootstrap) archivos seleccionados ignorados por git desde la raíz del proyecto hacia ese worktree.

La secuencia es:

1. Ejecutar `git ls-files --others --ignored --exclude-standard` en el host para enumerar los archivos ignorados.
2. Filtrar directorios pesados comunes más cualquier `exclude_paths` configurado.
3. Ejecutar `rsync --files-from` con `--link-dest` para que los archivos seleccionados se enlacen mediante hardlinks en el worktree en lugar de copiarse byte por byte.
4. Registrar el bootstrap exitoso en los metadatos internos del worktree para que asignaciones posteriores al mismo worktree puedan omitirlo.

Si `rsync` no está disponible, Coast recurre a un pipeline de `tar`.

Directorios grandes como `node_modules`, `.git`, `dist`, `target`, `.next`, `.nuxt`, `.cache`, `.worktrees` y `.coasts` se excluyen automáticamente. Se espera que los directorios grandes de dependencias se gestionen mediante cachés o volúmenes de servicios, en lugar de este paso genérico de bootstrap.

Debido a que la lista de archivos se genera por adelantado, `rsync` trabaja desde una lista dirigida en lugar de recorrer a ciegas todo el repositorio. Aun así, los repos con conjuntos muy grandes de archivos ignorados pueden seguir pagando un costo notable y único de bootstrap cuando se crea un worktree por primera vez. Si alguna vez necesitas refrescar ese bootstrap manualmente, ejecuta `coast assign --force-sync`.

### Diff de Disparadores de Reconstrucción

Coast solo calcula un diff de rama cuando `[assign.rebuild_triggers]` está configurado. En ese caso ejecuta:

```bash
git diff --name-only <previous>..<worktree>
```

El resultado se usa para degradar un servicio de `rebuild` a `restart` cuando ninguno de sus archivos disparadores cambió.

Esto es mucho más acotado que el modelo antiguo de “hacer diff de cada archivo rastreado en cada assign”. Si no configuras disparadores de reconstrucción, aquí no hay ningún paso de diff de rama.

`exclude_paths` actualmente no cambia este diff. Mantén tus listas de disparadores enfocadas en verdaderas entradas de tiempo de build como Dockerfiles, lockfiles y manifiestos de paquetes.

## `exclude_paths` — La Palanca Principal para Nuevos Worktrees

La opción `exclude_paths` en tu Coastfile le dice a Coast que omita árboles de directorios completos al construir la lista de archivos ignorados por git para el bootstrap de un nuevo worktree.

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

Los archivos bajo rutas excluidas siguen presentes en el worktree si Git los rastrea. Coast simplemente evita gastar tiempo enumerando y creando hardlinks de archivos ignorados bajo esos árboles durante el bootstrap inicial.

Esto es más impactante cuando la raíz de tu repo contiene grandes directorios ignorados de los que tus servicios en ejecución no se preocupan: apps no relacionadas, cachés vendorizadas, fixtures de test, documentación generada y otros árboles pesados.

Si asignas repetidamente al mismo worktree ya sincronizado, `exclude_paths` importa menos porque el bootstrap se omite. En ese caso, las decisiones de reinicio/reconstrucción de servicios se vuelven el factor dominante.

### Elegir Qué Excluir

Empieza perfilando tus archivos ignorados:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Si además quieres una vista del layout rastreado para ajustar los disparadores de reconstrucción, usa:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**Mantén** directorios que:
- Contengan código fuente montado en servicios en ejecución
- Contengan librerías compartidas importadas por esos servicios
- Contengan archivos generados o cachés que tu runtime realmente necesite en el primer arranque
- Estén referenciados en `[assign.rebuild_triggers]`

**Excluye** directorios que:
- Pertenezcan a apps o servicios que no se ejecutan en tu Coast
- Contengan documentación, scripts, configuraciones de CI o tooling no relacionado con el runtime
- Almacenen grandes cachés ignoradas que ya se preservan en otro lugar, como cachés dedicadas por servicio o volúmenes compartidos

### Ejemplo: Monorepo con Múltiples Apps

Un monorepo con muchos directorios de nivel superior, pero solo un subconjunto importa para los servicios que se ejecutan en este Coast:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

Esto mantiene el bootstrap inicial del worktree enfocado en los directorios que los servicios en ejecución realmente necesitan, en lugar de gastar tiempo en árboles ignorados no relacionados.

## Recorta Servicios Inactivos de `[assign.services]`

Si tu `COMPOSE_PROFILES` solo inicia un subconjunto de servicios, elimina los servicios inactivos de `[assign.services]`. Coast evalúa la estrategia de assign para cada servicio listado, y reiniciar o reconstruir un servicio que no está ejecutándose es trabajo desperdiciado.

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

Lo mismo aplica a `[assign.rebuild_triggers]` — elimina entradas de servicios que no estén activos.

## Usa `"hot"` Donde Sea Posible

La estrategia `"hot"` omite por completo el reinicio del contenedor. El [remontaje del sistema de archivos](FILESYSTEM.md) intercambia el código bajo `/workspace` y el watcher de archivos del servicio (Vite, webpack, nodemon, air, etc.) detecta los cambios automáticamente.

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"` es más rápido que `"restart"` porque evita el ciclo de detener/arrancar el contenedor. Úsalo para cualquier servicio que ejecute un servidor de desarrollo con vigilancia de archivos. Reserva `"restart"` para servicios que cargan el código al arrancar y no observan cambios (la mayoría de apps Rails, Go y Java).

## Usa `"rebuild"` con Disparadores

Si la estrategia predeterminada de un servicio es `"rebuild"`, cada cambio de rama reconstruye la imagen de Docker — incluso si no cambió nada que afecte a la imagen. Añade `[assign.rebuild_triggers]` para condicionar la reconstrucción a archivos específicos:

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

Si ninguno de los archivos disparadores cambió entre ramas, Coast omite la reconstrucción y vuelve a un reinicio en su lugar. Esto evita builds de imágenes costosos en cambios rutinarios de código.

## Resumen

| Optimización | Impacto | Afecta | Cuándo usar |
|---|---|---|---|
| `exclude_paths` | Alto | bootstrap inicial de archivos ignorados por git | Repos con árboles ignorados grandes que tu Coast no necesita |
| Eliminar servicios inactivos | Medio | reinicio/recreación de servicios | Cuando `COMPOSE_PROFILES` limita qué servicios se ejecutan |
| Estrategia `"hot"` | Alto | reinicio de contenedores | Servicios con watchers de archivos (Vite, webpack, nodemon, air) |
| `rebuild_triggers` | Alto | reconstrucciones de imágenes + diff de rama opcional | Servicios que usan `"rebuild"` y solo lo necesitan para cambios de infra |

Si los nuevos worktrees tardan en asignarse por primera vez, empieza con `exclude_paths`. Si los assigns repetidos son lentos, concéntrate en `hot` vs `restart`, recorta servicios inactivos y mantén `rebuild_triggers` bien acotado.
