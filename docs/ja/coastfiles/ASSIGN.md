# Assign

`[assign]` セクションは、`coast assign` でブランチを切り替えるときに、Coast インスタンス内のサービスがどう扱われるかを制御します。各サービスは、フルリビルドが必要か、再起動が必要か、ホットリロードが必要か、あるいは何もしないかに応じて、異なる戦略で設定できます。

実行時に `coast assign` と `coast unassign` がどのように動作するかについては、[Assign](../concepts_and_terminology/ASSIGN.md) を参照してください。

## `[assign]`

### `default`

ブランチ切り替え時に、すべてのサービスに適用されるデフォルトのアクションです。`[assign]` セクション全体が省略された場合、デフォルトは `"restart"` になります。

- **`"none"`** — 何もしません。サービスはそのまま実行され続けます。コードに依存しないデータベースやキャッシュに適しています。
- **`"hot"`** — コードはすでに [filesystem](../concepts_and_terminology/FILESYSTEM.md) によってライブマウントされているため、サービスは（ファイルウォッチャーやホットリロードなどにより）変更を自動的に取り込みます。コンテナの再起動は不要です。
- **`"restart"`** — サービスコンテナを再起動します。サービスが起動時にコードを読み込むが、完全なイメージのリビルドまでは不要な場合に使用します。
- **`"rebuild"`** — サービスの Docker イメージをリビルドして再起動します。Dockerfile の `COPY` や `ADD` によってコードがイメージに焼き込まれている場合に必要です。

```toml
[assign]
default = "none"
```

### `[assign.services]`

サービスごとの上書き設定です。各キーは compose のサービス名で、値は上記 4 つのアクションのいずれかです。

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

これにより、データベースやキャッシュは（デフォルトで `"none"` として）手を触れずに、変更されたコードに依存するサービスだけをリビルドまたは再起動できます。

### `[assign.rebuild_triggers]`

デフォルトのアクションがより軽いものになっていても、特定サービスについてリビルドを強制するファイルパターンです。各キーはサービス名で、値はファイルパスまたはパターンのリストです。

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

`coast assign` 中の worktree 同期から除外するパスのリストです。大規模なモノレポで、特定のディレクトリが Coast 上で動作しているサービスと無関係であり、割り当て操作（assign）を遅くしてしまう場合に有用です。

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Examples

### Rebuild app, leave everything else alone

app サービスがコードを Docker イメージに焼き込む一方で、データベースがコード変更から独立している場合:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### Hot-reload frontend and backend

両サービスがファイルウォッチャー（例: Next.js dev server、Go air、nodemon）を使用しており、コードがライブマウントされている場合:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### Per-service rebuild with triggers

API サービスは通常は再起動のみですが、`Dockerfile` または `package.json` が変更された場合はリビルドします:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### Full rebuild for everything

すべてのサービスがコードをイメージに焼き込む場合:

```toml
[assign]
default = "rebuild"
```
