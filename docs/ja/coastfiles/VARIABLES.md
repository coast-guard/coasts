# 変数

Coastfile は、すべての文字列値で環境変数の補間をサポートします。変数は TOML が処理される前、パース時に解決されるため、どのセクションでも、どの値の位置でも機能します。

## 構文

`${VAR_NAME}` で環境変数を参照します:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

変数名は文字またはアンダースコアで始まり、その後に文字、数字、またはアンダースコアが続く必要があります（パターン `[A-Za-z_][A-Za-z0-9_]*` に一致）。

## デフォルト値

`${VAR:-default}` を使うと、変数が設定されていない場合にフォールバックを指定できます:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

`PROJECT_NAME` が環境内で設定されている場合は、その値が使用されます。設定されていない場合は、`my-app` が代入されます。デフォルト値には `}` を除く任意の文字を含めることができます。

## 未定義の変数

デフォルトなしで変数が参照され、かつ環境内で設定されていない場合、Coast は **リテラルの `${VAR}` テキストを保持し**、警告を出力します:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

参照を保持することで（空文字列に黙って置き換えるのではなく）、`ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` のようなシェルコマンドを動作させ続けられます — Dockerfile のシェルは、Coast が `${ARCH}` を一度も設定していなくても、ビルド時にそれを展開できます。

変数が存在しないときに実際に空の置換を行いたい場合は、明示的な空のデフォルトを使用してください:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # PROJECT_NAME が未設定の場合は ""
```

警告なしでリテラルの `${VAR}` テキストが必要な場合は、`$${VAR}` でエスケープします（下の [Escaping](#escaping) を参照）。

## エスケープ

Coastfile 内でリテラルの `${...}` を生成するには（たとえば、展開された値ではなく `${VAR}` という文字列自体を含めたい値の場合）、先頭のドル記号を 2 つにします:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

これにより、変数参照を試みることなく、リテラル文字列 `echo '${NOT_EXPANDED}'` が生成されます。

## 例

### 環境由来のキーを使ったシークレット

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### 共有サービス設定

```toml
[shared_services.postgres]
image = "postgres:${PG_VERSION:-16}"
env = [
    "POSTGRES_USER=${DB_USER:-coast}",
    "POSTGRES_PASSWORD=${DB_PASSWORD:-dev}",
    "POSTGRES_DB=${DB_NAME:-coast_dev}",
]
ports = [5432]
```

### 環境ごとの compose パス

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## 変数とシークレットの違い

変数補間と [secrets](SECRETS.md) は、異なる目的に使われます:

| | 変数 (`${VAR}`) | シークレット (`[secrets.*]`) |
|---|---|---|
| **解決されるタイミング** | パース時（TOML 処理前） | ビルド時（設定されたソースから抽出） |
| **保存場所** | 解決済み Coastfile に埋め込まれる | 暗号化キーストア（`~/.coast/keystore.db`） |
| **用途** | 環境ごとに変わる設定（ポート、パス、イメージタグ） | 機密性の高い認証情報（API キー、トークン、パスワード） |
| **成果物内で見えるか** | はい（値はビルド内の `coastfile.toml` に現れます） | いいえ（マニフェストにはシークレット名のみが現れます） |

マシン間や CI 環境間で変わる、機密ではない設定には変数を使用してください。ビルド成果物に決して現れてはならない値にはシークレットを使用してください。
