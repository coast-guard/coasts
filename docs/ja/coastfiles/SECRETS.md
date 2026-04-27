# シークレットとインジェクション

`[secrets.*]` セクションは、Coast がビルド時にホストマシンから抽出する認証情報（キーチェーン、環境変数、ファイル、または任意のコマンド）を定義し、それらを Coast インスタンスに環境変数またはファイルとして注入します。別個の `[inject]` セクションは、抽出や暗号化を行わずに、シークレットではないホストの値をインスタンスに転送します。

シークレットがどのように保存、暗号化され、実行時に管理されるかについては、[Secrets](../concepts_and_terminology/SECRETS.md) を参照してください。

シークレットは [variable interpolation](VARIABLES.md) とは異なります。変数（`${VAR}`）は解析時に解決され、その値はビルド成果物に含まれます。シークレットはビルド時に抽出され、キーストアに暗号化して保存されます -- その値がビルド成果物に現れることはありません。

## `[secrets.*]`

各シークレットは、`[secrets]` 配下の名前付き TOML セクションです。常に 2 つのフィールドが必要です: `extractor` と `inject`。追加のフィールドは extractor にパラメータとして渡されます。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (必須)

抽出方法の名前です。組み込み extractor:

- **`env`** — ホストの環境変数を読み取る
- **`file`** — ホストファイルシステム上のファイルを読み取る
- **`command`** — シェルコマンドを実行して stdout を取得する
- **`keychain`** — macOS キーチェーンから読み取る（macOS のみ）

カスタム extractor も使用できます — PATH 上に `coast-extractor-{name}` という名前の実行可能ファイルがあれば、その `{name}` で extractor として利用できます。

### `inject` (必須)

シークレット値を Coast インスタンス内のどこに配置するかを指定します。形式は 2 つあります:

- `"env:VAR_NAME"` — 環境変数として注入される
- `"file:/absolute/path"` — ファイルに書き込まれる（tmpfs 経由でマウント）

```toml
# 環境変数として
inject = "env:DATABASE_URL"

# ファイルとして
inject = "file:/run/secrets/db_password"
```

`env:` または `file:` の後の値は空であってはなりません。

### `ttl`

省略可能な有効期限です。この期間を過ぎるとシークレットは古いものと見なされ、Coast は次回のビルドで extractor を再実行します。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### 追加パラメータ

シークレットセクション内の追加キー（`extractor`、`inject`、`ttl` を除く）は、extractor にパラメータとして渡されます。必要なパラメータは extractor によって異なります。

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

ホストファイルシステム上のファイルの内容を読み取ります。

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

パラメータ: `path` — ホスト上のファイルへのパス。

### `command` — シェルコマンド

ホスト上でシェルコマンドを実行し、stdout をシークレット値として取得します。

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

### `keychain` — macOS キーチェーン

macOS キーチェーンから認証情報を読み取ります。macOS でのみ利用可能です — 他のプラットフォームでこの extractor を参照すると、ビルド時エラーになります。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

パラメータ: `service` — 検索するキーチェーンサービス名。

## `[inject]`

`[inject]` セクションは、シークレット抽出および暗号化システムを経由せずに、ホストの環境変数とファイルを Coast インスタンスに転送します。サービスがホストから必要とする非機密の値に使用してください。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 転送するホスト環境変数名のリスト
- **`files`** — インスタンスにマウントするホストファイルパスのリスト

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

### macOS キーチェーンからの Claude Code 認証

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

### TTL を持つシークレット

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```
