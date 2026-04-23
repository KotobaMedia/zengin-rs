# 全銀フォーマット読み込み・書き込みライブラリ

このライブラリは、全銀フォーマットを Rust の構造体に読み書きするための、`serde` ベースの実装です。

現時点では `種別コード 91` の `口座振替` を対象に、依頼ファイルと口座振替結果照会ファイルの読み込みを提供します。

## ワークスペース構成

- `crates/zengin-rs`: パーサーとシリアライズを提供するライブラリ
- `crates/zengin-cli`: 全銀ファイルを読み込んで JSON を標準出力へ出す最小 CLI (`zengin`)

## 現在の実装範囲

- `zengin_rs::account_transfer::{File, Header, Detail, Trailer, End}` を提供
- `zengin_rs::account_transfer_result::{File, Header, Detail, Trailer, End}` を提供
- `zengin_rs::ParsedFile` と `zengin_rs::parse` / `from_bytes` で対応ファイルを自動判別
- `from_bytes` で固定長レコードを `serde` 経由で Rust 構造体に復元
- `to_bytes` で Rust 構造体から固定長レコードを生成
- `Jis` は `JIS8 (JIS X 0201)` として扱い、メモリ上では Unicode の `String` に変換
- 改行あり (`LF` / `CRLF`)・改行なし・EOF (`0x1a`) 付き入力を受理
- 出力形式は `OutputFormat` で明示指定

現時点で書き出しに対応しているのは `Ascii` と `Jis` です。`Ebcdic` は未対応です。

`Jis` のテキスト項目は Unicode では半角カナとして保持します。たとえば wire 上の JIS8 バイト列は、Rust 側では `"ﾔﾏﾀﾞﾀﾛｳ"` のような `String` として扱えます。JIS8 の対象外である全角カナ・ひらがな・漢字は、この実装ではまだ書き出しできません。

## CLI

`zengin` は入力ファイルを 1 つ受け取り、対応している全銀ファイルを自動判別して JSON を標準出力へ出します。

```bash
cargo run -p zengin-cli -- ./sample.zengin
```

インストールして使う場合:

```bash
cargo install --path crates/zengin-cli
zengin ./sample.zengin
```

## 使い方

### 読み込み

```rust
use zengin_rs::{OutputFormat, account_transfer, from_bytes, to_bytes};

# let sample = account_transfer::File {
#     header: account_transfer::Header {
#         kind_code: 91,
#         code_division: "0".into(),
#         collector_code: "1234567890".into(),
#         collector_name: "ﾃｽﾄｼｭｳｷﾝ".into(),
#         collection_date: "0430".into(),
#         bank_code: "0001".into(),
#         bank_name: "ﾃｽﾄｷﾞﾝｺｳ".into(),
#         branch_code: "123".into(),
#         branch_name: "ﾎﾝﾃﾝ".into(),
#         account_type: 1,
#         account_number: "7654321".into(),
#     },
#     details: vec![account_transfer::Detail {
#         bank_code: "0005".into(),
#         bank_name: "ﾃｽﾄｷﾞﾝｺｳ".into(),
#         branch_code: "001".into(),
#         branch_name: "ｼﾃﾝ".into(),
#         account_type: 1,
#         account_number: "1234567".into(),
#         payer_name: "ﾔﾏﾀﾞﾀﾛｳ".into(),
#         amount: 1200,
#         new_code: "0".into(),
#         customer_number: "00000000001234567890".into(),
#     }],
#     trailer: account_transfer::Trailer {
#         record_count: 1,
#         total_amount: 1200,
#     },
#     end: account_transfer::End,
# };
# let input = to_bytes(&sample, OutputFormat::readable())?;
let file: account_transfer::File = from_bytes(&input)?;

assert_eq!(file.header.kind_code, 91);
assert_eq!(file.header.collector_name, "ﾃｽﾄｼｭｳｷﾝ");
assert_eq!(file.details.len(), 1);
assert_eq!(file.details[0].payer_name, "ﾔﾏﾀﾞﾀﾛｳ");
assert_eq!(file.details[0].amount, 1200);

# Ok::<(), zengin_rs::Error>(())
```

### 書き込み

銀行へアップロードする送信用ファイルは `OutputFormat::canonical()` を使ってください。`OutputFormat::readable()` は確認用に改行を入れる形式です。

```rust
use zengin_rs::{
    OutputFormat, account_transfer,
    from_bytes, to_bytes,
};

let file = account_transfer::File {
    header: account_transfer::Header {
        kind_code: 91,
        code_division: "0".into(),
        collector_code: "1234567890".into(),
        collector_name: "ﾃｽﾄｼｭｳｷﾝ".into(),
        collection_date: "0430".into(),
        bank_code: "0001".into(),
        bank_name: "ﾃｽﾄｷﾞﾝｺｳ".into(),
        branch_code: "123".into(),
        branch_name: "ﾎﾝﾃﾝ".into(),
        account_type: 1,
        account_number: "7654321".into(),
    },
    details: vec![account_transfer::Detail {
        bank_code: "0005".into(),
        bank_name: "ﾃｽﾄｷﾞﾝｺｳ".into(),
        branch_code: "001".into(),
        branch_name: "ｼﾃﾝ".into(),
        account_type: 1,
        account_number: "1234567".into(),
        payer_name: "ﾔﾏﾀﾞﾀﾛｳ".into(),
        amount: 1200,
        new_code: "0".into(),
        customer_number: "00000000001234567890".into(),
    }],
    trailer: account_transfer::Trailer {
        record_count: 1,
        total_amount: 1200,
    },
    end: account_transfer::End,
};

let encoded = to_bytes(&file, OutputFormat::readable())?;
assert!(encoded.iter().any(|byte| *byte >= 0xA1));

let decoded: account_transfer::File = from_bytes(&encoded)?;

assert_eq!(decoded, file);

# Ok::<(), zengin_rs::Error>(())
```
