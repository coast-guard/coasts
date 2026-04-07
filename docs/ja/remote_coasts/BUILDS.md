# リモートビルド

リモートビルドは、coast-service を介してリモートマシン上で実行されます。これにより、ローカルのアーキテクチャ（例: ARM Mac）に関係なく、ビルドはリモートのネイティブアーキテクチャ（例: EC2 インスタンス上の x86_64）を使用します。クロスコンパイルやアーキテクチャエミュレーションは不要です。

## 仕組み

`coast build --type remote` を実行すると、以下の処理が行われます:

1. デーモンは、SSH 経由でプロジェクトのソースファイル（Coastfile、compose.yml、Dockerfiles、inject/）をリモートワークスペースに rsync します。
2. デーモンは、SSH トンネル経由で coast-service に対して `POST /build` を呼び出します。
3. coast-service は、`/data/images/` 配下で `docker build`、イメージのプル、イメージキャッシュ、シークレット抽出を含む完全なビルドを、リモート上でネイティブに実行します。
4. coast-service は、artifact パスとビルドメタデータを含む `BuildResponse` を返します。
5. デーモンは、完全な artifact ディレクトリ（coastfile.toml、compose.yml、manifest.json、secrets/、inject/、イメージ tarball）をローカルマシン上の `~/.coast/images/{project}/{build_id}/` に rsync で戻します。
6. デーモンは、新しいビルドを指す `latest-remote` シンボリックリンクを作成します。

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## コマンド

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

`coast run --type remote` も、まだ互換性のあるビルドが存在しない場合はビルドをトリガーします。

## アーキテクチャの一致

各ビルドの `manifest.json` には、それがどのアーキテクチャ向けにビルドされたか（例: `aarch64`、`x86_64`）が記録されます。`coast run --type remote` を実行すると、デーモンは既存のビルドが対象リモートのアーキテクチャに一致するかを確認します:

- **アーキテクチャが一致する**: そのビルドが再利用されます。再ビルドは不要です。
- **アーキテクチャが一致しない**: デーモンは正しいアーキテクチャを持つ最新のビルドを検索します。存在しない場合は、再ビルドを促すガイダンス付きのエラーを返します。

これは、x86_64 リモートで一度ビルドすれば、再ビルドなしで任意の数の x86_64 リモートにデプロイできることを意味します。しかし、ARM ビルドを x86_64 リモートで使用することも、その逆もできません。

## シンボリックリンク

リモートビルドは、ローカルビルドとは別のシンボリックリンクを使用します:

| Symlink | Points to |
|---------|-----------|
| `latest` | 最新のローカルビルド |
| `latest-remote` | 最新のリモートビルド |
| `latest-{type}` | 特定の Coastfile タイプの最新のローカルビルド |

この分離により、リモートビルドがローカルの `latest` シンボリックリンクを上書きしたり、その逆が起きたりすることを防ぎます。

## 自動プルーニング

Coast は、`(coastfile_type, architecture)` の組ごとに最大 5 件のリモートビルドを保持します。リモートビルドが成功するたびに、上限を超えた古いビルドは自動的に削除されます。

実行中のインスタンスで使用されているビルドは、上限に関係なく決してプルーニングされません。たとえば x86_64 のリモートビルドが 7 件あり、そのうち 3 件がアクティブなインスタンスを支えている場合、その 3 件はすべて保護されます。

プルーニングはアーキテクチャを考慮します。`aarch64` と `x86_64` の両方のリモートビルドがある場合、それぞれのアーキテクチャは独立して自身の 5 ビルドのプールを維持します。

## artifact ストレージ

リモートビルドの artifact は 2 か所に保存されます:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | リモートマシン上の信頼できる元データ |
| Local | `~/.coast/images/{project}/{build_id}/` | リモート間で再利用するためのローカルキャッシュ |

リモート上の `/data/image-cache/` にあるイメージキャッシュは、ローカルの `~/.coast/image-cache/` と同様に、すべてのプロジェクト間で共有されます。

## ローカルビルドとの関係

リモートビルドとローカルビルドは独立しています。`coast build`（`--type remote` なし）は常にローカルマシン上でビルドし、`latest` シンボリックリンクを更新します。`coast build --type remote` は常にリモートマシン上でビルドし、`latest-remote` シンボリックリンクを更新します。

同じプロジェクトについて、ローカルビルドとリモートビルドの両方を共存させることができます。ローカル coast はローカルビルドを使用し、リモート coast はリモートビルドを使用します。

ビルドの一般的な仕組み（manifest 構造、イメージキャッシュ、型付きビルド）については、[Builds](../concepts_and_terminology/BUILDS.md) を参照してください。
