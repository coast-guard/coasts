# パフォーマンス最適化

Coast はブランチ切り替えを高速にするよう設計されていますが、大規模なモノレポではデフォルトの挙動でも遅延が発生する場合があります。このページでは、Coastfile で利用できる調整レバーと、より重要な点として、それらが実際に `coast assign` のどの部分に影響するのかを説明します。

## Assign が遅くなる理由

`coast assign` は、Coast を新しい worktree に切り替える際にいくつかの処理を行います。

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

変動コストが最も大きいのは、通常 **初回の gitignored ブートストラップ**、**コンテナの再起動**、**イメージの再ビルド** です。rebuild トリガー用の任意のブランチ diff ははるかに軽量ですが、広いトリガーセットを指していると積み重なって効いてくることがあります。

### Gitignored ファイルのブートストラップ

worktree が初めて作成されると、Coast はプロジェクトルートから選択した gitignored ファイルをその worktree にブートストラップします。

手順は次のとおりです。

1. ホスト上で `git ls-files --others --ignored --exclude-standard` を実行して、無視されているファイルを列挙します。
2. よくある重いディレクトリと、設定された `exclude_paths` を除外します。
3. `--link-dest` 付きで `rsync --files-from` を実行し、選択されたファイルがバイト単位でコピーされるのではなく、worktree にハードリンクされるようにします。
4. 成功したブートストラップを内部の worktree メタデータに記録し、同じ worktree への以後の assign ではこれをスキップできるようにします。

`rsync` が利用できない場合、Coast は `tar` パイプラインにフォールバックします。

`node_modules`、`.git`、`dist`、`target`、`.next`、`.nuxt`、`.cache`、`.worktrees`、`.coasts` のような大きなディレクトリは自動的に除外されます。大きな依存ディレクトリは、この汎用ブートストラップ手順ではなく、サービスのキャッシュやボリュームで扱われることが想定されています。

ファイルリストは事前に生成されるため、`rsync` はリポジトリ全体を盲目的にクロールするのではなく、対象を絞ったリストに基づいて動作します。それでも、無視ファイルの集合が非常に大きいリポジトリでは、worktree を初めて作成する際に目立つ一回限りのブートストラップコストが発生し得ます。もしそのブートストラップを手動で更新する必要がある場合は、`coast assign --force-sync` を実行してください。

### Rebuild-Trigger Diff

Coast がブランチ diff を計算するのは `[assign.rebuild_triggers]` が設定されている場合に限られます。その場合、次を実行します。

```bash
git diff --name-only <previous>..<worktree>
```

結果は、トリガーファイルのどれも変更されていないときに、サービスを `rebuild` から `restart` にダウングレードするために使用されます。

これは、旧来の「assign のたびに追跡対象ファイルをすべて diff する」モデルよりはるかに限定的です。rebuild トリガーを設定しない場合、ここにブランチ diff のステップは一切ありません。

`exclude_paths` は現在この diff を変更しません。トリガーリストは、Dockerfile、ロックファイル、パッケージマニフェストのような真のビルド時入力に絞ってください。

## `exclude_paths` — 新規 Worktree に対する主要レバー

Coastfile の `exclude_paths` オプションは、新しい worktree の gitignored ブートストラップ用ファイルリストを構築する際に、ディレクトリツリー全体をスキップするよう Coast に指示します。

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

除外パス配下のファイルは、Git が追跡している場合は worktree に存在し続けます。Coast は、初回ブートストラップ中にそれらのツリー配下の無視ファイルを列挙してハードリンクする時間を使わないだけです。

これは、リポジトリルートに大きな無視ディレクトリがあり、実行中のサービスがそれらを必要としない場合に最も効果的です。無関係なアプリ、ベンダーされたキャッシュ、テストフィクスチャ、生成ドキュメント、その他の重いツリーなどが該当します。

すでに同期済みの同じ worktree に対して繰り返し assign している場合、ブートストラップはスキップされるため `exclude_paths` の重要性は下がります。その場合は、サービスの restart/rebuild の選択が支配的な要因になります。

### 除外するものの選び方

まず、無視ファイルをプロファイルします。

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

rebuild トリガー調整のために追跡対象のレイアウトも確認したい場合は、次を使用します。

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**残す** ディレクトリ:
- 実行中サービスにマウントされるソースコードを含む
- それらのサービスが import する共有ライブラリを含む
- 実行時が初回起動で実際に必要とする生成ファイルやキャッシュを含む
- `[assign.rebuild_triggers]` で参照されている

**除外する** ディレクトリ:
- Coast 内で稼働していないアプリやサービスに属する
- 実行時と無関係なドキュメント、スクリプト、CI 設定、またはツールを含む
- 専用サービスキャッシュや共有ボリュームなど、別の場所で既に保持されている大きな無視キャッシュを保持している

### 例: 複数アプリを含むモノレポ

トップレベルディレクトリが多数あるモノレポでも、この Coast で稼働するサービスに関係するのは一部だけ、というケース:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

これにより、初回の worktree ブートストラップは、無関係な無視ツリーに時間を費やすのではなく、稼働サービスが実際に必要とするディレクトリに集中できます。

## `[assign.services]` から非アクティブなサービスを削る

`COMPOSE_PROFILES` が一部のサービスしか起動しない場合、`[assign.services]` から非アクティブなサービスを削除してください。Coast はリストされたすべてのサービスについて assign 戦略を評価します。稼働していないサービスを再起動または再ビルドするのは無駄な作業です。

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

同じことが `[assign.rebuild_triggers]` にも当てはまります。アクティブでないサービスのエントリは削除してください。

## 可能な限り `"hot"` を使う

`"hot"` 戦略は、コンテナ再起動自体を完全にスキップします。[filesystem remount](FILESYSTEM.md) により `/workspace` 配下のコードが差し替えられ、サービスのファイルウォッチャー（Vite、webpack、nodemon、air など）が変更を自動的に取り込みます。

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"` は、コンテナの stop/start サイクルを回避するため `"restart"` より高速です。ファイル監視付きの開発サーバーを動かすサービスにはこれを使用してください。起動時にコードを読み込み、変更を監視しないサービス（多くの Rails、Go、Java アプリ）には `"restart"` を使ってください。

## トリガー付きで `"rebuild"` を使う

サービスのデフォルト戦略が `"rebuild"` の場合、ブランチ切り替えのたびに Docker イメージが再ビルドされます。イメージに影響する変更が何もなくてもです。`[assign.rebuild_triggers]` を追加して、特定ファイルに基づいて rebuild をゲートしてください。

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

ブランチ間でトリガーファイルが何も変更されていなければ、Coast は rebuild をスキップし、代わりに restart にフォールバックします。これにより、日常的なコード変更で高価なイメージビルドを回避できます。

## まとめ

| 最適化 | 効果 | 影響範囲 | 使いどころ |
|---|---|---|---|
| `exclude_paths` | 高 | 初回の gitignored ブートストラップ | Coast が必要としない大きな無視ツリーがあるリポジトリ |
| 非アクティブサービスの削除 | 中 | サービスの再起動/再作成 | `COMPOSE_PROFILES` が稼働サービスを制限している場合 |
| `"hot"` 戦略 | 高 | コンテナ再起動 | ファイルウォッチャーがあるサービス（Vite、webpack、nodemon、air） |
| `rebuild_triggers` | 高 | イメージ再ビルド + 任意のブランチ diff | `"rebuild"` を使うが、インフラ変更のときだけ必要なサービス |

新しい worktree の初回 assign が遅い場合は、まず `exclude_paths` から始めてください。繰り返し assign が遅い場合は、`hot` と `restart` の使い分け、非アクティブサービスの削減、そして `rebuild_triggers` を絞り込むことに注力してください。
