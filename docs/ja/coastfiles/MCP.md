# MCP Servers and Clients

> **Note:** MCP 構成が関係するのは、[`[agent_shell]`](AGENT_SHELL.md) を介して Coast コンテナ内でコーディングエージェントを実行している場合のみです。エージェントがホスト上で動作している（より一般的な）構成では、エージェントは既に自身の MCP サーバーへアクセスできるため、Coast がそれらを構成する必要はありません。

`[mcp.*]` セクションは、Coast インスタンスの内部または隣接して動作する MCP（Model Context Protocol）サーバーを構成します。`[mcp_clients.*]` セクションは、それらのサーバーを Claude Code や Cursor のようなコーディングエージェントに接続し、自動的に検出して利用できるようにします。

MCP サーバーがどのようにインストールされ、プロキシされ、実行時に管理されるかについては、[MCP Servers](../concepts_and_terminology/MCP_SERVERS.md) を参照してください。

## MCP Servers — `[mcp.*]`

各 MCP サーバーは、`[mcp]` 配下の名前付き TOML セクションです。モードは 2 つあります: **internal**（Coast コンテナ内で実行）と **host-proxied**（ホスト上で実行し、Coast にプロキシ）です。

### Internal MCP servers

内部サーバーは DinD コンテナ内にインストールされ、そこで実行されます。`proxy` がない場合、`command` フィールドが必須です。

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

フィールド:

- **`command`**（必須）— 実行する実行ファイル
- **`args`** — コマンドに渡す引数
- **`env`** — サーバープロセス用の環境変数
- **`install`** — サーバー起動前に実行するコマンド（文字列または配列を受け付けます）
- **`source`** — ホスト上のディレクトリをコンテナ内の `/mcp/{name}/` にコピーするための指定

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
```

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

### Host-proxied MCP servers

ホストプロキシのサーバーはホストマシン上で実行され、`coast-mcp-proxy` を介して Coast 内で利用可能になります。このモードを有効にするには `proxy = "host"` を設定します。

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

`proxy = "host"` の場合:

- `command`、`args`、`env` は任意です — 省略した場合、サーバーはホスト側の既存 MCP 構成から名前で解決されます。
- `install` と `source` は **使用できません**（サーバーはコンテナ内ではなくホスト上で実行されます）。

追加フィールドのないホストプロキシサーバーは、ホスト構成から名前でサーバーを検索します:

```toml
[mcp.host-lookup]
proxy = "host"
```

`proxy` の有効な値は `"host"` のみです。

### Multiple servers

任意の数の MCP サーバーを定義できます:

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]

[mcp.host-lookup]
proxy = "host"
```

## MCP Clients — `[mcp_clients.*]`

MCP クライアントコネクタは、コーディングエージェントが読み取る設定ファイルへ MCP サーバー構成を書き込む方法を Coast に伝えます。これにより、あなたの `[mcp.*]` サーバーがエージェントへ自動的に接続されます。

### Built-in connectors

2 つのコネクタが組み込みで提供されています: `claude-code` と `cursor`。これらを使用するのに追加フィールドは不要です。

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

組み込みコネクタは自動的に次を把握しています:

- **`claude-code`** — `/root/.claude/mcp_servers.json` に書き込みます
- **`cursor`** — `/workspace/.cursor/mcp.json` に書き込みます

設定パスは上書きできます:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### Custom connectors

組み込みではないエージェントの場合、`run` フィールドで Coast が MCP サーバーを登録するために実行するシェルコマンドを指定します:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

`run` フィールドは `format` または `config_path` と併用できません。

### Custom format connectors

エージェントが Claude Code や Cursor と同じ設定ファイル形式を使うが、パスが異なる場合:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

`format` は `"claude-code"` または `"cursor"` である必要があります。`format` とともに組み込みではない名前を使う場合、`config_path` は必須です。

## Examples

### Internal MCP server wired to Claude Code

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Host-proxied server with internal server

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }

[mcp_clients.claude-code]
```

### Multiple client connectors

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```
