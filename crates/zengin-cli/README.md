# zengin-cli

`zengin` は全銀ファイルを読み込み、JSON または CSV として標準出力へ出す CLI です。

既定は JSON 出力と自動判別です。複数フォーマットとして解釈できる入力はエラーになります。その場合は `--type` で明示してください。入力サイズ上限は 10 MiB です。

## インストール

crates.io からインストールする場合:

```bash
cargo install zengin-cli
```

このリポジトリから直接インストールする場合:

```bash
cargo install --path crates/zengin-cli
```

## 使い方

JSON として出力:

```bash
zengin --type request ./zengin.txt
```

開発中にワークスペースから実行:

```bash
cargo run -p zengin-cli -- --type request ./zengin.txt
```

指定できる主な `--type` は `general-transfer`, `payroll-transfer`, `request` (`account-transfer`), `result` (`account-transfer-result`), `transfer-account-inquiry`, `payment-notice` です。

CSV として出力:

```bash
zengin --format csv --type request ./zengin.txt
```

CSV は `file_type`, `record_type`, `detail_index` と、各レコードのフィールドを持つレコード単位の表です。

ヘッダーとトレーラーだけを確認:

```bash
zengin --metadata-only --type result ./zengin.txt
```

`--metadata-only` は明細とエンドレコードを省いた出力にします。
