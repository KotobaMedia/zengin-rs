use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use zengin_rs::{
    OutputFormat,
    account_transfer::{Detail, End, File, Header, Trailer},
    to_bytes,
};

fn sample_file() -> File {
    File {
        header: Header {
            kind_code: 91,
            collection_date: "20260430".to_string(),
            collector_code: "1234567890".to_string(),
            collector_name: "ﾃｽﾄｼｭｳｷﾝ".to_string(),
            bank_code: "0001".to_string(),
            branch_code: "123".to_string(),
            account_type: 1,
            account_number: "76543210".to_string(),
        },
        details: vec![Detail {
            payer_code: "9000000001".to_string(),
            payer_name: "ﾔﾏﾀﾞﾀﾛｳ".to_string(),
            bank_code: "0005".to_string(),
            branch_code: "001".to_string(),
            account_type: 1,
            account_number: "12345678".to_string(),
            amount: 1200,
        }],
        trailer: Trailer {
            record_count: 1,
            total_amount: 1200,
        },
        end: End,
    }
}

fn temp_input_path() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("zengin-cli-{stamp}.txt"))
}

#[test]
fn parses_input_file_to_json() {
    let input_path = temp_input_path();
    let input = to_bytes(&sample_file(), OutputFormat::readable()).unwrap();
    fs::write(&input_path, input).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_zengin"))
        .arg(&input_path)
        .output()
        .unwrap();

    let _ = fs::remove_file(&input_path);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["header"]["kind_code"], 91);
    assert_eq!(json["header"]["collector_name"], "ﾃｽﾄｼｭｳｷﾝ");
    assert_eq!(json["details"][0]["amount"], 1200);
}

#[test]
fn reports_usage_without_an_input_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_zengin")).output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
