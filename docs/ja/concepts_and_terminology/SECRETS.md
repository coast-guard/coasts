# シークレットとエクストラクタ

シークレットは、ホストマシンから抽出され、Coast コンテナに環境変数またはファイルとして注入される値です。Coast はビルド時にシークレットを抽出し、ローカルのキーストアに保存する際に暗号化し、Coast インスタンスが作成されるときに注入します。

## 注入タイプ

各シークレットには `inject` ターゲットがあり、Coast コンテナにどのように届けられるかを制御します:

- `env:VAR_NAME` — 環境変数として注入されます。
- `file:/path/in/container` — コンテナ内のファイルとしてマウントされます。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.credentials]
extractor = "file"
path = "~/.config/my-app/credentials.json"
inject = "file:/run/secrets/credentials.json"
```

## 組み込みエクストラクタ

### env

ホストの環境変数を読み取ります。これは最も一般的で、最もシンプルなエクストラクタです。ホスト上にすでに環境変数としてシークレットがある場合 — `.env` ファイル、`direnv`、シェルプロファイル、またはその他の任意のソースから — それらをそのまま Coast に転送するだけです。

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

ほとんどのプロジェクトは `env` エクストラクタだけで十分です。

### file

ホストのファイルシステムからファイルを読み取ります。ホームディレクトリのパスに対する `~` 展開をサポートします。SSH キー、TLS 証明書、認証情報の JSON ファイルに適しています。

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

シェルコマンドを実行し、stdout をシークレット値としてキャプチャします。コマンドは `sh -c` 経由で実行されるため、パイプ、リダイレクト、変数展開がすべて動作します。これは 1Password CLI、HashiCorp Vault、または任意の動的ソースからシークレットを取得するのに便利です。

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

`command` は、ローカルの設定ファイルから特定フィールドを変換したり抽出したりする目的にも使えます:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

`macos-keychain` のエイリアスです。macOS Keychain から一般パスワード項目を読み取ります。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

キーチェーン・エクストラクタはしばしば不要です。同じ値を環境変数またはファイルで取得できる場合は、それらのよりシンプルな方法を優先してください。キーチェーンからの抽出が有用なのは、シークレットが macOS Keychain にしか存在せず、簡単にはエクスポートできない場合です — たとえば、Keychain に直接書き込むサードパーティ製ツールによって保存されるアプリケーション固有の認証情報などです。

`account` パラメータは任意で、デフォルトは macOS のユーザー名です。

このエクストラクタは macOS でのみ利用できます。他のプラットフォームで参照すると、ビルド時に明確なエラーが発生します。

## カスタムエクストラクタ

組み込みエクストラクタのいずれもワークフローに合わない場合、Coast は PATH 上にある `coast-extractor-{name}` という名前の実行ファイルを探すようにフォールバックします。この実行ファイルは stdin で JSON としてエクストラクタのパラメータを受け取り、stdout にシークレット値を書き出す必要があります。

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

Coast は `coast-extractor-vault` を起動し、stdin に `{"path": "secret/data/token"}` を渡します。終了コード 0 は成功、非 0 は失敗を意味します（stderr はエラーメッセージに含まれます）。

## 非シークレットの注入

`[inject]` セクションは、ホストの環境変数やファイルをシークレットとして扱わずに Coast へ転送します。これらの値は暗号化されず、直接渡されます。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

機密ではない設定には `[inject]` を使用してください。保存時に暗号化されるべきものはすべて `[secrets]` を使用してください。

## シークレットはビルドに保存されません

シークレットはビルド時に抽出されますが、coast のビルドアーティファクトに焼き込まれることはありません。`coast run` で Coast インスタンスが作成されるときに注入されます。つまり、シークレットを露出させることなくビルドアーティファクトを共有できます。

シークレットは再ビルドなしで実行時に再注入できます。[Coastguard](COASTGUARD.md) UI では、Secrets タブの **Re-run Secrets** アクションを使用してください。CLI からは、[`coast build --refresh`](BUILDS.md) を使用してシークレットを再抽出して更新します。

## TTL と再抽出

シークレットには任意で `ttl`（time-to-live）フィールドを設定できます。シークレットが期限切れになると、`coast build --refresh` がソースから再抽出します。

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## 保存時の暗号化

抽出されたすべてのシークレットは、ローカルのキーストア内で AES-256-GCM により暗号化されます。暗号鍵は macOS では macOS Keychain に保存され、Linux では 0600 権限のファイルに保存されます。
