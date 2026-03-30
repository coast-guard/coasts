# プライベートパス

複数の Coast インスタンスが同じプロジェクトルートを共有する場合、それらは同じファイル — そして同じ inode — を共有します。通常、これは意図された動作です。ホスト上のファイル変更は、両側が同じファイルシステムを見ているため、即座に Coast 内に反映されます。しかし、一部のツールは排他的アクセスを前提としたプロセスごとの状態をワークスペースに書き込みます。この前提は、2 つのインスタンスが同じマウントを共有すると崩れます。

## 問題

Next.js 16 を考えてみましょう。開発サーバーの起動時に、`.next/dev/lock` に対して `flock(fd, LOCK_EX)` を使って排他的ロックを取得します。`flock` は inode レベルのカーネル機構であり、マウント名前空間、コンテナ境界、バインドマウントパスを考慮しません。2 つの異なる Coast コンテナ内の 2 つのプロセスが、同じ `.next/dev/lock` inode を指している場合（同じホストのバインドマウントを共有しているため）、2 番目のプロセスは最初のプロセスのロックを検出し、起動を拒否します。

```text
⨯ Another next dev server is already running.

- Local: http://localhost:3000
- PID: 1361
- Dir: /workspace/frontend
```

同じ種類の競合は、以下にも当てはまります。

- `flock` / `fcntl` アドバイザリロック（Next.js、Turbopack、Cargo、Gradle）
- PID ファイル（多くのデーモンは PID ファイルを書き込み、起動時にそれを確認します）
- 単一ライターアクセスを前提とするビルドキャッシュ（Webpack、Vite、esbuild）

マウント名前空間の分離（`unshare`）はここでは役に立ちません。マウント名前空間は、プロセスがどのマウントポイントを見られるかを制御しますが、`flock` は inode 自体に対して動作します。異なるマウントパスを通じて同じ inode を見ている 2 つのプロセスは、依然として競合します。

## 解決策

`private_paths` Coastfile フィールドは、インスタンスごとに分離されるべきワークスペース相対ディレクトリを宣言します。各 Coast インスタンスは、これらのパスに対して独自の隔離されたバインドマウントを取得し、その実体はコンテナ自身のファイルシステム上のインスタンスごとのディレクトリです。

```toml
[coast]
name = "my-app"
private_paths = ["frontend/.next"]
```

Coast は共有伝播付きで `/workspace` をマウントした後、各プライベートパスに対して追加のバインドマウントを適用します。

```text
mkdir -p /coast-private/frontend/.next /workspace/frontend/.next
mount --bind /coast-private/frontend/.next /workspace/frontend/.next
```

`/coast-private/` は DinD コンテナの書き込み可能レイヤー上に存在し、共有ホストのバインドマウント上にはないため、各インスタンスは自然に別々の inode を取得します。`dev-1` のロックファイルは `dev-2` のロックファイルとは異なる inode に存在するため、競合は解消されます。

## 仕組み

プライベートパスマウントは、Coast ライフサイクルの中で `/workspace` がマウントまたは再マウントされるすべての時点で適用されます。

1. **`coast run`** — 初期の `mount --bind /host-project /workspace && mount --make-rshared /workspace` の後に、プライベートパスがマウントされます。
2. **`coast start`** — コンテナ再起動時にワークスペースのバインドマウントを再適用した後。
3. **`coast assign`** — `/workspace` をアンマウントして worktree ディレクトリへ再バインドした後。
4. **`coast unassign`** — `/workspace` をプロジェクトルートに戻した後。

プライベートディレクトリは stop/start サイクルをまたいで保持されます（共有マウント上ではなく、コンテナのファイルシステム上に存在するため）。`coast assign` または `coast unassign` の際には、プライベートディレクトリは **クリア** されます。これにより、開発サーバーは前のブランチの古いビルド出力を配信するのではなく、正しいブランチのソースファイルから再コンパイルします。`coast rm` 時には、コンテナとともに削除されます。

## 使用するタイミング

ツールが、同時実行中の Coast インスタンス間で競合するプロセスごとまたはインスタンスごとの状態をワークスペースディレクトリに書き込む場合は、`private_paths` を使用してください。

- **ファイルロック**: `.next/dev/lock`、Cargo の `target/.cargo-lock`、Gradle の `.gradle/lock`
- **ビルドキャッシュ**: `.next`、`.turbo`、`target/`、`.vite`
- **PID ファイル**: ワークスペースに PID ファイルを書き込む任意のデーモン

インスタンス間で共有する必要があるデータや、ホストから見える必要があるデータには `private_paths` を使用しないでください。永続的で Docker 管理の隔離データ（データベースボリュームなど）が必要な場合は、代わりに [volumes with `strategy = "isolated"`](../coastfiles/VOLUMES.md) を使用してください。

## 検証ルール

- パスは相対パスでなければなりません（先頭の `/` は不可）
- パスに `..` コンポーネントを含めてはいけません
- パスは重複してはいけません — `frontend/.next` と `frontend/.next/cache` の両方を列挙するのはエラーです。最初のマウントが 2 番目を隠してしまうためです

## ボリュームとの関係

`private_paths` と `[volumes]` は、異なる隔離の問題を解決します。

| | `private_paths` | `[volumes]` |
|---|---|---|
| **何を** | ワークスペース相対ディレクトリ | Docker 管理の名前付きボリューム |
| **どこに** | `/workspace` 内 | コンテナ内の任意のマウントパス |
| **実体** | コンテナローカルのファイルシステム（`/coast-private/`） | Docker 名前付きボリューム |
| **隔離** | 常にインスタンスごと | `isolated` または `shared` 戦略 |
| **`coast rm` 後も残るか** | いいえ | Isolated: いいえ。Shared: はい。 |
| **ユースケース** | ビルド生成物、ロックファイル、キャッシュ | データベース、永続的なアプリケーションデータ |

## 設定リファレンス

完全な構文と例については、Coastfile リファレンスの [`private_paths`](../coastfiles/PROJECT.md) を参照してください。
