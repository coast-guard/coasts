# シークレットとインジェクション

`[secrets.*]` セクションは、Coast がビルド時にホストマシン（キーチェーン、環境変数、ファイル、または任意のコマンド）から抽出し、Coast インスタンスへ環境変数またはファイルとして注入する認証情報を定義します。別の `[inject]` セクションは、抽出や暗号化を行わずに、シークレットではないホストの値をインスタンスへ転送します。

シークレットが実行時にどのように保存・暗号化・管理されるかについては、[Secrets](../concepts_and_terminology/SECRETS.md) を参照してください。

## `[secrets.*]`

各シークレットは `[secrets]` 配下の名前付き TOML セクションです。常に必要なフィールドは 2 つで、`extractor` と `inject` です。追加フィールドは extractor へのパラメータとして渡されます。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (required)

抽出方法の名前。組み込み extractor:

- **`env`** — ホストの環境変数を読み取ります
- **`file`** — ホストのファイルシステムからファイルを読み取ります
- **`command`** — シェルコマンドを実行し stdout をキャプチャします
- **`keychain`** — macOS Keychain から読み取ります（macOS のみ）

カスタム extractor も使用できます。PATH 上にある `coast-extractor-{name}` という名前の任意の実行ファイルは、その `{name}` で利用可能な extractor になります。

### `inject` (required)

シークレット値を Coast インスタンス内のどこに配置するか。形式は 2 つ:

- `"env:VAR_NAME"` — 環境変数として注入します
- `"file:/absolute/path"` — ファイルに書き込みます（tmpfs 経由でマウント）

```toml
# 環境変数として
inject = "env:DATABASE_URL"

# ファイルとして
inject = "file:/run/secrets/db_password"
```

`env:` または `file:` の後の値は空であってはいけません。

### `ttl`

任意の有効期限（duration）。この期間を過ぎるとシークレットは古い（stale）とみなされ、次回のビルドで Coast が extractor を再実行します。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### 追加パラメータ

シークレットセクション内の追加キー（`extractor`、`inject`、`ttl` 以外）はすべて、extractor へのパラメータとして渡されます。必要なパラメータは extractor によって異なります。

## 組み込み extractor

### `env` — ホスト環境変数

名前でホスト環境変数を読み取ります。

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

パラメータ: `var` — 読み取る環境変数名。

### `file` — ホストファイル

ホストファイルシステム上のファイル内容を読み取ります。

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

パラメータ: `path` — ホスト上のファイルへのパス。

### `command` — シェルコマンド

ホスト上でシェルコマンドを実行し、stdout をシークレット値としてキャプチャします。

```toml
[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"
```

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); print(json.dumps({k:d[k] for k in [\"oauthAccount\"] if k in d}))"'
inject = "file:/root/.claude.json"
```

パラメータ: `run` — 実行するシェルコマンド。

### `keychain` — macOS Keychain

macOS Keychain から認証情報を読み取ります。macOS でのみ利用可能で、他プラットフォームでこの extractor を参照するとビルド時エラーになります。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

パラメータ: `service` — 参照する Keychain サービス名。

## `[inject]`

`[inject]` セクションは、シークレットの抽出・暗号化システムを経由せずに、ホスト環境変数やファイルを Coast インスタンスへ転送します。サービスがホストから必要とする非機密値に対して使用してください。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 転送するホスト環境変数名のリスト
- **`files`** — インスタンスへマウントするホストファイルパスのリスト

## 例

### 複数の extractor

```toml
[secrets.file_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "env:FILE_SECRET"

[secrets.env_secret]
extractor = "env"
var = "COAST_TEST_ENV_SECRET"
inject = "env:ENV_SECRET"

[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"

[secrets.file_inject_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "file:/run/secrets/test_secret"
```

### macOS Keychain からの Claude Code 認証

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); out={\"hasCompletedOnboarding\":True,\"numStartups\":1}; print(json.dumps(out))"'
inject = "file:/root/.claude.json"
```

### TTL 付きシークレット

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```
