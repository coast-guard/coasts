# MCP Servers

MCP（Model Context Protocol）サーバーは、AIエージェントにツールへのアクセス（ファイル検索、データベースクエリ、ドキュメント参照、ブラウザ自動化など）を提供します。Coast は Coast コンテナ内に MCP サーバーをインストールおよび設定できるため、コンテナ化されたエージェントが必要なツールにアクセスできます。

**これは、Coast コンテナ内でエージェントを実行している場合にのみ関係します。** エージェントをホスト上で実行する（推奨のアプローチ）場合、MCP サーバーもホスト上で実行され、この設定は一切不要です。このページは [Agent Shells](AGENT_SHELLS.md) を土台にし、その上にさらに複雑さを追加します。先に進む前に、そちらの警告を読んでください。

## Internal vs Host-Proxied Servers

Coast は MCP サーバーに対して 2 つのモードをサポートしており、Coastfile の `[mcp]` セクションにある `proxy` フィールドで制御します。

### Internal Servers

Internal サーバーは `/mcp/<name>/` に DinD コンテナ内へインストールされ、そこで実行されます。コンテナ化されたファイルシステムと稼働中のサービスへ直接アクセスできます。

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

また、プロジェクトから MCP ディレクトリへソースファイルをコピーすることもできます。

```toml
[mcp.my-custom-tool]
source = "tools/my-mcp-server"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
```

`source` フィールドは、セットアップ中に `/workspace/<path>/` から `/mcp/<name>/` へファイルをコピーします。`install` コマンドはそのディレクトリ内で実行されます。これは、リポジトリ内にある MCP サーバーに便利です。

### Host-Proxied Servers

Host-proxied サーバーはコンテナ内ではなく、ホストマシン上で実行されます。Coast は `coast-mcp-proxy` を使い、コンテナからホストへネットワーク越しに MCP リクエストを転送するクライアント設定を生成します。

```toml
[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]
```

Host-proxied サーバーには `install` や `source` フィールドを設定できません。ホスト上ですでに利用可能であることが前提です。ブラウザ自動化やホストのファイルシステムツールなど、ホストレベルのアクセスが必要な MCP サーバーにはこのモードを使用してください。

### When to Use Which

| Mode | Runs in | Good for | Limitations |
|---|---|---|---|
| Internal | DinD container | コンテナのファイルシステムへのアクセスが必要なツール、プロジェクト固有のツール | Alpine Linux 上でインストール可能である必要がある、`coast run` の時間が増える |
| Host-proxied | Host machine | ブラウザ自動化、ホストレベルのツール、巨大で事前インストール済みのサーバー | コンテナのファイルシステムへ直接アクセスできない |

## Client Connectors

`[mcp_clients]` セクションは、コンテナ内のエージェントがサーバーを検出できるように、生成された MCP サーバー設定を Coast がどこへ書き込むかを指定します。

### Built-in Formats

Claude Code と Cursor の場合、正しい名前の空セクションを用意するだけで十分です。Coast が形式とデフォルトの設定パスを自動検出します。

```toml
[mcp_clients.claude-code]
# Writes to /root/.claude/mcp_servers.json (auto-detected)

[mcp_clients.cursor]
# Writes to /workspace/.cursor/mcp.json (auto-detected)
```

### Custom Config Path

その他の AI ツールでは、形式とパスを明示的に指定してください。

```toml
[mcp_clients.my-tool]
format = "claude-code"
config_path = "/home/coast/.config/my-tool/mcp.json"
```

### Command-Based Connectors

ファイルに書き込む代わりに、生成された設定 JSON をコマンドへパイプすることもできます。

```toml
[mcp_clients.custom-setup]
run = "my-config-tool import-mcp --stdin"
```

`run` フィールドは `format` および `config_path` と同時に使用できません（排他的です）。

## Coastguard MCP Tab

[Coastguard](COASTGUARD.md) Web UI は、MCP タブから MCP 設定の可視性を提供します。

![MCP tab in Coastguard](../../assets/coastguard-mcp.png)
*設定済みサーバー、それらのツール、クライアント設定の場所を表示している Coastguard の MCP タブ。*

このタブには 3 つのセクションがあります。

- **MCP Servers** — 宣言された各サーバーを、名前、種別（Internal または Host）、コマンド、状態（Installed、Proxied、または Not Installed）とともに一覧表示します。
- **Tools** — サーバーを選択して、MCP プロトコル経由で公開されているツールを確認します。各ツールは名前と説明を表示し、クリックすると完全な入力スキーマが表示されます。
- **Client Locations** — 生成された設定ファイルが書き込まれた場所を表示します（例:`claude-code` 形式が `/root/.claude/mcp_servers.json`）。

## CLI Commands

```bash
coast mcp dev-1 ls                          # list servers with type and status
coast mcp dev-1 tools context7              # list tools exposed by a server
coast mcp dev-1 tools context7 info resolve # show input schema for a specific tool
coast mcp dev-1 locations                   # show where client configs were written
```

`tools` コマンドは、コンテナ内の MCP サーバープロセスへ JSON-RPC の `initialize` と `tools/list` リクエストを送ることで動作します。これは internal サーバーでのみ動作します。host-proxied サーバーはホスト側から確認する必要があります。

## How Installation Works

`coast run` 中、内側の Docker デーモンの準備ができてサービスが起動し始めた後、Coast は MCP のセットアップを行います。

1. 各 **internal** MCP サーバーについて:
   - DinD コンテナ内に `/mcp/<name>/` を作成
   - `source` が設定されている場合、`/workspace/<source>/` から `/mcp/<name>/` へファイルをコピー
   - `/mcp/<name>/` 内で各 `install` コマンドを実行（例:`npm install -g @upstash/context7-mcp`）

2. 各 **client connector** について:
   - 適切な形式（Claude Code または Cursor）で JSON 設定を生成
   - internal サーバーは実際の `command` と `args` を受け取り、`cwd` を `/mcp/<name>/` に設定
   - host-proxied サーバーはコマンドとして `coast-mcp-proxy` を受け取り、引数としてサーバー名を設定
   - 設定を対象パスへ書き込む（または `run` コマンドへパイプする）

Host-proxied サーバーは、コンテナ内の `coast-mcp-proxy` によって MCP プロトコルのリクエストをホストマシンへ転送し、そこで実際の MCP サーバープロセスが実行されます。

## Full Example

internal のドキュメントツールと host-proxied のブラウザツールをセットアップし、Claude Code に接続する Coastfile の例:

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]

[mcp_clients.claude-code]
```

`coast run` の後、コンテナ内の Claude Code は MCP 設定内で両方のサーバーを認識します。`context7` は `/mcp/context7/` でローカル実行され、`browser` はホストへプロキシされます。

## Agents Running on the Host

コーディングエージェントをホストマシン上で実行する（推奨のアプローチ）場合、MCP サーバーもホスト上で実行され、Coast の `[mcp]` 設定は関与しません。ただし 1 つ考慮点があります。**Coast 内のデータベースやサービスへ接続する MCP サーバーは、正しいポートを把握している必要があります。**

サービスが Coast 内で動作している場合、それらは実行するたびに変わる動的ポートでアクセス可能になります。ホスト上の database MCP が `localhost:5432` に接続すると、[checked-out](CHECKOUT.md) されている Coast のデータベースにしか到達しないか、Coast がチェックアウトされていなければ何にも到達しません。チェックアウトされていないインスタンスに対しては、MCP を [dynamic port](PORTS.md)（例:`localhost:55681`）を使うように再設定する必要があります。

回避策は 2 つあります。

**共有サービスを使う。** データベースが [shared service](SHARED_SERVICES.md) として動作している場合、ホストの Docker デーモン上で標準のポート（`localhost:5432`）に存在します。各 Coast インスタンスはブリッジネットワーク越しにそれへ接続し、ホスト側の MCP も常に同じポートで同じデータベースに接続します。再設定不要で、動的ポートの発見も不要です。これが最も簡単なアプローチです。

**`coast exec` または `coast docker` を使う。** データベースが Coast 内（隔離ボリューム）で動作している場合でも、ホスト側のエージェントは Coast を介してコマンドを実行することでクエリできます（[Exec & Docker](EXEC_AND_DOCKER.md) を参照）:

```bash
coast exec dev-1 -- psql -h localhost -U myuser -d mydb -c "SELECT count(*) FROM users"
coast docker dev-1 exec -i my-postgres psql -U myuser -d mydb -c "\\dt"
```

これにより動的ポートを知る必要がまったくなくなります。コマンドは Coast 内で実行され、そこではデータベースが標準のポートで利用可能です。

ほとんどのワークフローでは、共有サービスが最も抵抗の少ない選択肢です。ホスト側の MCP 設定は、Coasts を使い始める前とまったく同じまま維持できます。
