# プライマリポート & DNS

プライマリポートは、あなたのサービスのうち1つ（通常はWebフロントエンド）へのクイックリンクを作成するための、任意の利便機能です。Coastguard ではクリック可能なバッジとして表示され、`coast ports` では星付きのエントリとして表示されます。ポートの動作自体は変わらず、強調表示するものを1つ選ぶだけです。

## プライマリポートの設定

Coastfile の `[coast]` セクションに `primary_port` を追加し、[`[ports]`](PORTS.md) のキーを参照します:

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

プロジェクトのポートが1つだけの場合、Coast がそれをプライマリとして自動検出するため、明示的に設定する必要はありません。

また、Coastguard の Ports タブで任意のサービスの横にあるスターアイコンをクリックしてプライマリを切り替えることも、CLI から `coast ports set-primary` を使って切り替えることもできます。この設定はビルド単位のため、同じビルドから作成されたすべてのインスタンスで同じプライマリが共有されます。

## 有効になること

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

星が付いたサービスがプライマリです。Coastguard では、インスタンス名の隣にクリック可能なバッジとして表示され、ワンクリックでブラウザでアプリを開けます。

これは特に次の用途で便利です:

- **ホスト側エージェント** — AI エージェントに、変更を確認するための単一URLを渡せます。「localhost:62217 を開いて」と伝える代わりに、プライマリポートのURLは `coast ls` やデーモンAPIからプログラム的に取得できます。
- **ブラウザ MCP** — エージェントがブラウザ MCP を使ってUI変更を検証する場合、プライマリポートのURLが指し示すべき正規のターゲットになります。
- **高速な反復** — 最も頻繁に見るサービスへワンクリックでアクセスできます。

プライマリポートは完全に任意です。なくてもすべて動作します — より速いナビゲーションのためのQOL（使い勝手向上）機能です。

## サブドメインルーティング

分離されたデータベースで複数の Coast インスタンスを動かすと、ブラウザ上ではすべて `localhost` を共有します。つまり `localhost:62217`（dev-1）によって設定されたCookieは `localhost:63104`（dev-2）からも見えてしまいます。アプリがセッションCookieを使っている場合、片方のインスタンスにログインすると、もう片方に干渉することがあります。

サブドメインルーティングは、各インスタンスに独自のオリジンを与えることでこれを解決します:

```text
Without subdomain routing:
  dev-1 web  →  http://localhost:62217
  dev-2 web  →  http://localhost:63104
  (cookies shared — both are "localhost")

With subdomain routing:
  dev-1 web  →  http://dev-1.localhost:62217
  dev-2 web  →  http://dev-2.localhost:63104
  (cookies isolated — different subdomains)
```

プロジェクトごとに有効化するには、Coastguard の Ports タブ（ページ下部のトグル）から行うか、デーモン設定APIを使用します。

### トレードオフ: CORS

欠点として、アプリケーション側でCORS調整が必要になる場合があります。`dev-1.localhost:3000` のフロントエンドが `dev-1.localhost:8080` にAPIリクエストを送ると、ポートが異なるためブラウザはこれらをクロスオリジンとして扱います。多くの開発サーバーはすでにこれを処理していますが、サブドメインルーティングを有効化した後にCORSエラーが出る場合は、アプリケーションの許可オリジン設定を確認してください。

## URL テンプレート

各サービスには、リンクの生成方法を制御するURLテンプレートがあります。デフォルトは次のとおりです:

```text
http://localhost:<port>
```

`<port>` プレースホルダーは実際のポート番号に置き換えられます — インスタンスが [チェックアウト](CHECKOUT.md) されている場合はカノニカルポート、そうでなければダイナミックポートです。サブドメインルーティングが有効な場合、`localhost:` は `{instance}.localhost:` に置き換えられます。

Coastguard の Ports タブ（各サービスの横の鉛筆アイコン）から、サービスごとにテンプレートをカスタマイズできます。これは、開発サーバーがHTTPSを使う場合、カスタムホスト名を使う場合、または標準でないURLスキームを使う場合に便利です:

```text
https://my-service.localhost:<port>
```

テンプレートはデーモン設定に保存され、再起動後も保持されます。

## DNS セットアップ

多くのブラウザは、標準で `*.localhost` を `127.0.0.1` に解決するため、サブドメインルーティングはDNS設定なしで動作します。

カスタムドメイン解決（例: `*.localcoast`）が必要な場合、Coast には組み込みDNSサーバーが含まれています。一度だけセットアップします:

```bash
coast dns setup    # writes /etc/resolver/localcoast (requires sudo)
coast dns status   # check if DNS is configured
coast dns remove   # remove the resolver entry
```

これは任意であり、ブラウザで `*.localhost` が動作しない場合、またはカスタムTLDを使いたい場合にのみ必要です。
