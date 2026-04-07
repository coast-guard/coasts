# Назначение и снятие назначения

Назначение и снятие назначения управляют тем, на какое рабочее дерево (worktree) указывает экземпляр Coast. См. [Filesystem](FILESYSTEM.md), чтобы понять, как переключение worktree работает на уровне монтирования.

## Назначение

`coast assign` переключает экземпляр Coast на конкретное worktree. Coast создаёт worktree, если его ещё не существует, обновляет код внутри Coast и перезапускает сервисы согласно настроенной стратегии назначения (assign).

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

После назначения `dev-1` работает на ветке `feature/oauth` со всеми поднятыми сервисами.

## Снятие назначения

`coast unassign` переключает экземпляр Coast обратно на корень проекта (ваша ветка main/master). Привязка к worktree удаляется, и Coast возвращается к запуску от основного репозитория.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## Стратегии назначения

Когда Coast назначается на новое worktree, каждому сервису нужно знать, как обработать изменение кода. Это настраивается для каждого сервиса в вашем [Coastfile](COASTFILE_TYPES.md) в разделе `[assign]`:

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

Доступные стратегии:

- **none** — ничего не делать. Используйте это для сервисов, которые не меняются между ветками, например Postgres или Redis.
- **hot** — заменить только файловую систему. Сервис остаётся запущенным и подхватывает изменения через распространение монтирования (mount propagation) и файловые наблюдатели (например, dev-сервер с hot reload).
- **restart** — перезапустить контейнер сервиса. Используйте это для интерпретируемых сервисов, которым достаточно перезапуска процесса. Это значение по умолчанию.
- **rebuild** — пересобрать образ сервиса и перезапустить. Используйте это, когда смена ветки затрагивает `Dockerfile` или зависимости времени сборки.

Также можно указать триггеры пересборки, чтобы сервис пересобирался только при изменении определённых файлов:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Если ни один из триггерных файлов не изменился между ветками, сервис пропускает пересборку, даже если стратегия установлена в `rebuild`.

## Удалённые worktree

Если назначенное worktree удалено, демон `coastd` автоматически снимает назначение этого экземпляра обратно на корень основного Git-репозитория.

---

> **Совет: снижение задержки назначения в больших кодовых базах**
>
> Внутри, первое назначение на новое worktree подготавливает (bootstrap) выбранные файлы, игнорируемые git, в это worktree, а сервисы с `[assign.rebuild_triggers]` могут запускать `git diff --name-only`, чтобы решить, нужна ли пересборка. В больших кодовых базах этот шаг bootstrap и ненужные пересборки обычно доминируют по времени назначения.
>
> Используйте `exclude_paths` в вашем Coastfile, чтобы уменьшить объём bootstrap для файлов, игнорируемых git, используйте `"hot"` для сервисов с файловыми наблюдателями и держите `[assign.rebuild_triggers]` сфокусированными на настоящих входных данных времени сборки. Если вам нужно вручную обновить bootstrap игнорируемых файлов для существующего worktree, выполните `coast assign --force-sync`. Полное руководство см. в [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md).

---

> **Удалённое назначение**
>
> Для [удалённых coast](REMOTES.md) `coast assign` выполняет rsync содержимого нового worktree на удалённый хост перед пересылкой запроса назначения в `coast-service`. Применяются те же стратегии назначения. После первоначального rsync mutagen продолжает синхронизировать изменения файлов в реальном времени.
