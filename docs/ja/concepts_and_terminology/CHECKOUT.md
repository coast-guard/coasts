# Checkout

Checkout は、あなたのプロジェクトの[canonical ports](PORTS.md)をどの Coast インスタンスが所有するかを制御します。Coast をチェックアウトすると、`localhost:3000`、`localhost:5432`、およびその他すべての canonical port がそのインスタンスに直接マッピングされます。

```bash
coast checkout dev-1
```

```text
Before checkout:
  localhost:3000  ──→  (nothing)
  localhost:5432  ──→  (nothing)

After checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

Checkout の切り替えは瞬時に行われます — Coast は軽量な `socat` フォワーダーを停止して再生成します。コンテナは再起動されません。

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Linux Note

動的ポートは、Linux では特別な権限なしで常に動作します。

`1024` 未満の canonical port は異なります。Coastfile で `80` や `443` のようなポートを宣言している場合、ホストを設定するまで Linux が `coast checkout` によるバインドをブロックすることがあります。一般的な対処法は次のとおりです。

- `net.ipv4.ip_unprivileged_port_start` を引き上げる
- フォワーディング用バイナリまたはプロセスに bind capability を付与する

ホストがバインドを拒否した場合、Coast はこれを明示的に報告します。

WSL では、Coast は Docker によって公開された checkout bridge を使用するため、Windows のブラウザやツールは Sail のような Docker Desktop ワークフローと同様に、`127.0.0.1` を通じてチェックアウトされた canonical port に到達できます。

Caddy を使うローカル HTTPS プロジェクトでは、Coast は Coast のインストールごとに 1 つの Caddy ローカル CA を再利用します。そのルートを一度信頼すれば、同じインストール配下で再作成されたワークスペースでも引き続きそれが使われます。

ルート証明書の場所は次のとおりです。

- 通常のインストールでは `~/.coast/caddy/pki/authorities/local/root.crt`
- `coast-dev` では `~/.coast-dev/caddy/pki/authorities/local/root.crt`

これらは意図的に分離されているため、`coast-dev` を信頼しても通常の `coast` インストールまで信頼されることはなく、その逆も同様です。

アクティブなインストールのルート証明書を確認またはエクスポートするには:

```bash
coast cert info
coast cert path
coast cert fingerprint
coast cert export --to ~/Downloads/coast-root.crt
```

Coast は信頼ストアへのインストール自体はユーザーに委ねています。証明書をエクスポートし、必要に応じて OS またはブラウザの信頼ストアにインポートしてください。

## Do You Need to Check Out?

必ずしもそうではありません。実行中の各 Coast は常にそれぞれ独自の動的ポートを持っており、何もチェックアウトしなくても、いつでもそれらのポートを通じて任意の Coast にアクセスできます。

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

チェックアウトしなくても、ブラウザで `localhost:62217` を開いて dev-1 の web サーバーにアクセスできます。これは多くのワークフローでまったく問題なく、`coast checkout` を一度も使わずに、好きなだけ多くの Coast を実行できます。

## When Checkout Is Useful

動的ポートだけでは不十分で、canonical port が必要になる状況があります。

- **canonical port にハードコードされたクライアントアプリケーション。** Coast の外で動作するクライアント、たとえばホスト上のフロントエンド開発サーバー、スマートフォン上のモバイルアプリ、またはデスクトップアプリが `localhost:3000` や `localhost:8080` を前提としている場合、あらゆる場所でポート番号を変更するのは現実的ではありません。Coast をチェックアウトすると、設定を一切変更せずに本来のポートを利用できます。

- **Webhook とコールバック URL。** Stripe、GitHub、OAuth プロバイダのようなサービスは、登録済みの URL — 通常は `localhost:3000` に転送する `https://your-ngrok-tunnel.io` のようなもの — にコールバックを送信します。動的ポートに切り替えると、コールバックは届かなくなります。チェックアウトにより、テスト中の Coast に対して canonical port が有効であることが保証されます。

- **データベースツール、デバッガー、IDE 連携。** 多くの GUI クライアント（pgAdmin、DataGrip、TablePlus）、デバッガー、IDE の実行構成は、特定のポートを持つ接続プロファイルを保存します。Checkout を使えば、保存済みプロファイルはそのままに、その背後にある Coast だけを切り替えられます — コンテキストを切り替えるたびに、デバッガーのアタッチ先やデータベース接続を再設定する必要はありません。

## Releasing Checkout

別の Coast をチェックアウトせずに canonical port を解放したい場合:

```bash
coast checkout --none
```

この後、canonical port を所有する Coast はなくなります。すべての Coast は引き続きそれぞれの動的ポートを通じてアクセス可能です。

## Only One at a Time

一度にチェックアウトできる Coast は正確に 1 つだけです。`dev-1` がチェックアウトされている状態で `coast checkout dev-2` を実行すると、canonical port は即座に `dev-2` に切り替わります。空白時間はありません — 古いフォワーダーが停止され、新しいものが同じ操作の中で生成されます。

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

動的ポートは checkout の影響を受けません。変わるのは、canonical port の向き先だけです。
