# Назначение и снятие назначения

Назначение и снятие назначения управляют тем, на какое рабочее дерево (worktree) указывает экземпляр Coast. См. [Filesystem](FILESYSTEM.md), чтобы узнать, как переключение worktree работает на уровне монтирования.

## Назначение

`coast assign` переключает экземпляр Coast на конкретное worktree. Coast создаёт worktree, если оно ещё не существует, обновляет код внутри Coast и перезапускает сервисы согласно настроенной стратегии назначения.

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

`coast unassign` переключает экземпляр Coast обратно в корень проекта (ваша ветка main/master). Привязка к worktree удаляется, и Coast возвращается к работе от основного репозитория.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## Стратегии назначения

Когда Coast назначается на новое worktree, каждому сервису нужно понимать, как обрабатывать изменение кода. Это настраивается для каждого сервиса в вашем [Coastfile](COASTFILE_TYPES.md) в разделе `[assign]`:

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

- **none** — ничего не делать. Используйте для сервисов, которые не меняются между ветками, например Postgres или Redis.
- **hot** — менять только файловую систему. Сервис остаётся запущенным и подхватывает изменения через распространение монтирования и файловые наблюдатели (например, dev-сервер с hot reload).
- **restart** — перезапуск контейнера сервиса. Используйте для интерпретируемых сервисов, которым нужен лишь перезапуск процесса. Это значение по умолчанию.
- **rebuild** — пересобрать образ сервиса и перезапустить. Используйте, когда смена ветки затрагивает `Dockerfile` или зависимости времени сборки.

Также можно указать триггеры пересборки, чтобы сервис пересобирался только при изменении определённых файлов:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Если ни один из файлов-триггеров не изменился между ветками, сервис пропускает пересборку, даже если стратегия установлена в `rebuild`.

## Удалённые рабочие деревья

Если назначенное worktree удалено, демон `coastd` автоматически снимает назначение этого экземпляра обратно на корень основного Git-репозитория.

---

> **Совет: снижение задержки назначения в больших кодовых базах**
>
> Внутри Coast запускает `git ls-files` каждый раз, когда worktree монтируется или размонтируется. В больших кодовых базах или репозиториях с большим количеством файлов это может добавлять заметную задержку к операциям назначения и снятия назначения.
>
> Если части вашей кодовой базы не нужно пересобирать между назначениями, вы можете указать Coast пропускать их с помощью `exclude_paths` в вашем Coastfile:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> Пути, перечисленные в `exclude_paths`, игнорируются при диффе файлов, что может существенно ускорить время назначения.
