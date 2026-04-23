# 全銀フォーマット読み込み・書き込みライブラリ

このライブラリは、全銀フォーマットを Rust の構造体に読み書きするための、`serde` ベースの実装です。

現時点では `種別コード 91` の `口座振替` を対象に、ヘッダ・明細・トレーラ・エンドの最小実装を提供します。

## 現在の実装範囲

- `zengin_rs::account_transfer::{File, Header, Detail, Trailer, End}` を提供
- `from_bytes` で固定長レコードを `serde` 経由で Rust 構造体に復元
- `to_bytes` で Rust 構造体から固定長レコードを生成
- 改行あり (`LF` / `CRLF`)・改行なし・EOF (`0x1a`) 付き入力を受理
- 出力形式は `OutputFormat` で明示指定

`Encoding` には `Ascii` / `Jis` / `Ebcdic` を定義していますが、この最初の実装で実際に書き出せるのは `Ascii` のみです。README のサンプルも ASCII 互換のデータを使っています。

## 使い方

### 読み込み

```rust
use zengin_rs::{account_transfer, from_bytes};

# fn pad_right(value: &str, width: usize) -> String {
#     format!("{value:<width$}")
# }
# fn sample_bytes() -> Vec<u8> {
#     let header = format!(
#         "1{:02}{}{}{}{}{}{}{}{}",
#         91,
#         "20260430",
#         "1234567890",
#         pad_right("ACME COLLECT", 40),
#         "0001",
#         "123",
#         1,
#         "76543210",
#         " ".repeat(43),
#     );
#     let detail = format!(
#         "2{}{}{}{}{}{}{:010}{}",
#         "9000000001",
#         pad_right("TARO YAMADA", 40),
#         "0005",
#         "001",
#         1,
#         "12345678",
#         1200,
#         " ".repeat(43),
#     );
#     let trailer = format!("8{:06}{:012}{}", 1, 1200, " ".repeat(101));
#     let end = format!("9{}", " ".repeat(119));
#     format!("{header}\n{detail}\n{trailer}\n{end}\n").into_bytes()
# }
let file: account_transfer::File = from_bytes(&sample_bytes())?;

assert_eq!(file.header.kind_code, 91);
assert_eq!(file.header.collector_name, "ACME COLLECT");
assert_eq!(file.details.len(), 1);
assert_eq!(file.details[0].amount, 1200);

# Ok::<(), zengin_rs::Error>(())
```

### 書き込み

```rust
use zengin_rs::{
    OutputFormat, account_transfer,
    from_bytes, to_bytes,
};

let file = account_transfer::File {
    header: account_transfer::Header {
        kind_code: 91,
        collection_date: "20260430".into(),
        collector_code: "1234567890".into(),
        collector_name: "ACME COLLECT".into(),
        bank_code: "0001".into(),
        branch_code: "123".into(),
        account_type: 1,
        account_number: "76543210".into(),
    },
    details: vec![account_transfer::Detail {
        payer_code: "9000000001".into(),
        payer_name: "TARO YAMADA".into(),
        bank_code: "0005".into(),
        branch_code: "001".into(),
        account_type: 1,
        account_number: "12345678".into(),
        amount: 1200,
    }],
    trailer: account_transfer::Trailer {
        record_count: 1,
        total_amount: 1200,
    },
    end: account_transfer::End,
};

let encoded = to_bytes(&file, OutputFormat::readable())?;
let decoded: account_transfer::File = from_bytes(&encoded)?;

assert_eq!(decoded, file);

# Ok::<(), zengin_rs::Error>(())
```
