# レシピ

レシピは、一般的なプロジェクト構成向けの、完全で注釈付きの Coastfile サンプルです。各レシピには、コピーして適用できる完全な Coastfile と、各判断がなぜ行われたのかを説明するセクションごとのウォークスルーが含まれます。

Coastfile が初めての場合は、まず [Coastfiles reference](../coastfiles/README.md) を参照してください。レシピは中核となる概念に精通していることを前提としています。

- [Full-Stack Monorepo](FULLSTACK_MONOREPO.md) - ホスト上で共有する Postgres と Redis、bare-service の Vite フロントエンド、そして compose による dockerized バックエンド。ボリューム戦略、healthchecks、assign のチューニング、大規模リポジトリ向けの `exclude_paths` をカバーします。
- [Next.js Application](NEXTJS.md) - Turbopack を使った Next.js、共有の Postgres と Redis、バックグラウンドワーカー、そして認証コールバックのための動的ポート処理。`.next` の分離のための `private_paths`、bare service の最適化、マルチエージェント worktree サポートをカバーします。
