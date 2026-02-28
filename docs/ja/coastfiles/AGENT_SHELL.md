# Agent Shell

> **ほとんどのワークフローでは、コーディングエージェントをコンテナ化する必要はありません。** Coasts はホストマシンと [filesystem](../concepts_and_terminology/FILESYSTEM.md) を共有するため、最もシンプルな方法はホスト上でエージェントを実行し、統合テストのような実行時負荷の高いタスクには [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md) を使うことです。エージェントシェルは、エージェントをコンテナ内で動かしたい場合 — たとえば内部の Docker デーモンへ直接アクセスさせたい場合や、環境を完全に隔離したい場合 — に利用します。

`[agent_shell]` セクションは、Claude Code や Codex のようなエージェント TUI を Coast コンテナ内で実行するように設定します。これが存在する場合、Coast はインスタンス起動時に、設定されたコマンドを実行する永続的な PTY セッションを自動的に起動します。

エージェントシェルの動作（アクティブなエージェントモデル、入力の送信、ライフサイクルと復旧）を含む全体像については、[Agent Shells](../concepts_and_terminology/AGENT_SHELLS.md) を参照してください。

## Configuration

このセクションには必須フィールドが 1 つあります: `command`。

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (required)

エージェント PTY で実行するシェルコマンドです。通常は `[coast.setup]` でインストールしたコーディングエージェントの CLI になります。

コマンドは DinD コンテナ内の `/workspace`（プロジェクトルート）で実行されます。compose サービスではありません — compose スタックや素のサービスと並行して動作し、それらの中で動くわけではありません。

## Lifecycle

- エージェントシェルは `coast run` で自動的に起動します。
- [Coastguard](../concepts_and_terminology/COASTGUARD.md) では、閉じることのできない永続的な「Agent」タブとして表示されます。
- エージェントプロセスが終了した場合、Coast はそれを再起動できます。
- 実行中のエージェントシェルへは `coast agent-shell input` を通して入力を送信できます。

## Examples

### Claude Code

`[coast.setup]` で Claude Code をインストールし、[secrets](SECRETS.md) で認証情報を設定してから、エージェントシェルを設定します:

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

機能が動作することをテストするための最小限のエージェントシェル:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```
