# 実行

`coast run` は新しい Coast インスタンスを作成します。最新の [build](BUILDS.md) を解決し、[DinD コンテナ](RUNTIMES_AND_SERVICES.md) をプロビジョニングし、キャッシュされたイメージを読み込み、compose サービスを起動し、[動的ポート](PORTS.md) を割り当て、インスタンスを状態データベースに記録します。

```bash
coast run dev-1
```

`-w` を渡すと、Coast はプロビジョニング完了後にワークツリーも [割り当て](ASSIGN.md) ます:

```bash
coast run dev-1 -w feature/oauth
```

これは、ハーネスやエージェントがワークツリーを作成し、それに対応する Coast も 1 ステップで必要とする場合の最も一般的なパターンです。

## 何が起こるか

`coast run` は 4 つのフェーズを実行します:

1. **検証と挿入** — 名前が一意であることを確認し、build ID（`latest` シンボリックリンクまたは明示的な `--build-id` から）を解決し、`Provisioning` インスタンスレコードを挿入します。
2. **Docker プロビジョニング** — ホストデーモン上に DinD コンテナを作成し、インスタンスごとのイメージをビルドし、キャッシュされたイメージ tarball を内部デーモンに読み込み、compose ファイルを書き換え、シークレットを注入し、`docker compose up -d` を実行します。
3. **最終化** — ポート割り当てを保存し、ポートがちょうど 1 つであればプライマリポートを設定し、インスタンスを `Running` に遷移させます。
4. **オプションのワークツリー割り当て** — `-w <worktree>` が指定されていた場合、新しいインスタンスに対して `coast assign` を実行します。割り当てに失敗しても、Coast 自体は引き続き実行中です — 失敗は警告としてログに記録されます。

DinD コンテナ内の永続的な `/var/lib/docker` ボリュームにより、後続の実行ではイメージの読み込みがスキップされます。コールドキャッシュの新規 `coast run` には 20 秒以上かかることがありますが、`coast rm` 後の再実行は通常 10 秒未満で完了します。

## CLI の使い方

```text
coast run <name> [options]
```

| Flag | Description |
|------|-------------|
| `-w`, `--worktree <name>` | プロビジョニング完了後にこのワークツリーを割り当てる |
| `--n <count>` | バッチ作成。名前には `{n}` を含める必要があります（例: `coast run dev-{n} --n=5` は dev-1 から dev-5 を作成します） |
| `-t`, `--type <type>` | 型付き build を使用する（例: `--type snap` は `latest` の代わりに `latest-snap` を解決します） |
| `--force-remove-dangling` | 作成前に同名の残留 Docker コンテナを削除する |
| `-s`, `--silent` | 進行状況の出力を抑制し、最終サマリーまたはエラーのみを表示する |
| `-v`, `--verbose` | Docker build ログを含む詳細情報を表示する |

git ブランチは常に現在の HEAD から自動検出されます。

## バッチ作成

名前に `{n}` を使い、`--n` で複数のインスタンスを一度に作成します:

```bash
coast run dev-{n} --n=5
```

これにより `dev-1`、`dev-2`、`dev-3`、`dev-4`、`dev-5` が順番に作成されます。各インスタンスはそれぞれ独自の DinD コンテナ、ポート割り当て、ボリューム状態を持ちます。10 個を超えるバッチでは確認が求められます。

## 型付き build

プロジェクトで複数の Coastfile タイプを使用している場合（[Coastfile Types](COASTFILE_TYPES.md) を参照）、使用する build を選択するために `--type` を渡します:

```bash
coast run dev-1                    # "latest" を解決
coast run test-1 --type test       # "latest-test" を解決
coast run snapshot-1 --type snap   # "latest-snap" を解決
```

## run と assign と remove

- `coast run` は **新しい** インスタンスを作成します。別の Coast が必要なときに使います。
- `coast assign` は **既存の** インスタンスを別のワークツリーに向け直します。すでに Coast があり、
  実行するコードを切り替えたいときに使います。
- `coast rm` はインスタンスを完全に削除します。Coast を停止したいときや、
  まっさらな状態から再作成したいときに使います。

日常的な切り替えの多くでは `coast rm` は不要です。通常は `coast assign` と
`coast checkout` で十分です。クリーンに再作成したいとき、特に Coastfile や build を
再ビルドした後には `coast rm` を使ってください。

これらは組み合わせることもできます: `coast run dev-3 -w feature/billing` はインスタンスを作成し、
ワークツリーの割り当ても 1 ステップで行います。

## 残留コンテナ

以前の `coast run` が中断された、または `coast rm` が完全にクリーンアップできなかった場合、「残留 Docker container」エラーが表示されることがあります。残ったコンテナを削除して続行するには `--force-remove-dangling` を渡してください:

```bash
coast run dev-1 --force-remove-dangling
```

## 関連項目

- [Remove](REMOVE.md) — インスタンスを完全に削除する
- [Builds](BUILDS.md) — `coast run` が利用するもの
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — 各インスタンス内の DinD アーキテクチャ
- [Assign and Unassign](ASSIGN.md) — 既存のインスタンスを別のワークツリーに切り替える
- [Ports](PORTS.md) — 動的ポートと正規ポートがどのように割り当てられるか
- [Coasts](COASTS.md) — Coast インスタンスの高レベルな概念
