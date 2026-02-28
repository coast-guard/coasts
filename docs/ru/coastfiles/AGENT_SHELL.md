# Agent Shell

> **В большинстве рабочих процессов вам не нужно контейнеризировать вашего кодингового агента.** Поскольку Coasts разделяют [файловую систему](../concepts_and_terminology/FILESYSTEM.md) с вашей хост-машиной, самый простой подход — запускать агента на хосте и использовать [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md) для ресурсоёмких задач во время выполнения, таких как интеграционные тесты. Agent shells предназначены для случаев, когда вы специально хотите, чтобы агент работал внутри контейнера — например, чтобы дать ему прямой доступ к внутреннему Docker-демону или полностью изолировать его окружение.

Раздел `[agent_shell]` настраивает агентский TUI — например, Claude Code или Codex — для запуска внутри контейнера Coast. При наличии этого раздела Coast автоматически поднимает постоянную PTY-сессию, выполняющую настроенную команду при запуске инстанса.

Для полного понимания того, как работают agent shells — активная модель агента, отправка ввода, жизненный цикл и восстановление — см. [Agent Shells](../concepts_and_terminology/AGENT_SHELLS.md).

## Configuration

В разделе есть одно обязательное поле: `command`.

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (required)

Команда оболочки, которую нужно запустить в PTY агента. Обычно это CLI кодингового агента, который вы установили через `[coast.setup]`.

Команда выполняется внутри контейнера DinD в `/workspace` (корень проекта). Это не compose-сервис — она работает рядом с вашим compose-стеком или отдельными сервисами, а не внутри них.

## Lifecycle

- Agent shell автоматически запускается при `coast run`.
- В [Coastguard](../concepts_and_terminology/COASTGUARD.md) он отображается как постоянная вкладка "Agent", которую нельзя закрыть.
- Если процесс агента завершится, Coast может перезапустить его.
- Вы можете отправлять ввод в работающий agent shell через `coast agent-shell input`.

## Examples

### Claude Code

Установите Claude Code в `[coast.setup]`, настройте учётные данные через [secrets](SECRETS.md), затем настройте agent shell:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### Simple agent shell

Минимальный agent shell для проверки, что функция работает:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```
