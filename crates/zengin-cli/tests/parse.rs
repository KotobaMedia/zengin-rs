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
            code_division: "0".to_string(),
            collector_code: "1234567890".to_string(),
            collection_date: "0430".to_string(),
            collector_name: "ﾃｽﾄｼｭｳｷﾝ".to_string(),
            bank_code: "0001".to_string(),
            bank_name: "ﾃｽﾄｷﾞﾝｺｳ".to_string(),
            branch_code: "123".to_string(),
            branch_name: "ﾎﾝﾃﾝ".to_string(),
            account_type: 1,
            account_number: "7654321".to_string(),
        },
        details: vec![Detail {
            bank_code: "0005".to_string(),
            bank_name: "ﾃｽﾄｷﾞﾝｺｳ".to_string(),
            branch_code: "001".to_string(),
            branch_name: "ｼﾃﾝ".to_string(),
            account_type: 1,
            account_number: "1234567".to_string(),
            payer_name: "ﾔﾏﾀﾞﾀﾛｳ".to_string(),
            amount: 1200,
            new_code: "0".to_string(),
            customer_number: "00000000001234567890".to_string(),
        }],
        trailer: Trailer {
            record_count: 1,
            total_amount: 1200,
        },
        end: End,
    }
}

fn pad_text(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn pad_number<T>(value: T, width: usize) -> String
where
    T: std::fmt::Display,
{
    format!("{value:0width$}")
}

fn sample_result_input() -> Vec<u8> {
    let mut lines = Vec::new();
    lines.push(format!(
        "1{:02}{}{}{}{}{}{}{}{}{}{}{}",
        91,
        0,
        "1234567890",
        pad_text("ACME COLLECTOR", 40),
        "0422",
        "0288",
        pad_text("BANK ALPHA", 15),
        "220",
        pad_text("MAIN BRANCH", 15),
        1,
        "5000001",
        " ".repeat(17),
    ));
    lines.push(format!(
        "2{}{}{}{}{}{}{}{}{}{}{}{}{}",
        "0288",
        pad_text("BANK ALPHA", 15),
        "110",
        pad_text("WEST", 15),
        " ".repeat(4),
        1,
        "6000001",
        pad_text("ALPHA INC", 30),
        pad_number(1000, 10),
        0,
        "01234567890123450001",
        0,
        " ".repeat(8),
    ));
    lines.push(format!(
        "2{}{}{}{}{}{}{}{}{}{}{}{}{}",
        "0288",
        pad_text("BANK ALPHA", 15),
        "650",
        pad_text("EAST", 15),
        " ".repeat(4),
        2,
        "6000002",
        pad_text("BETA LLC", 30),
        pad_number(2000, 10),
        1,
        "01234567890123450002",
        1,
        " ".repeat(8),
    ));
    lines.push(format!(
        "8{}{}{}{}{}{}{}",
        pad_number(2, 6),
        pad_number(3000, 12),
        pad_number(1, 6),
        pad_number(1000, 12),
        pad_number(1, 6),
        pad_number(2000, 12),
        " ".repeat(65),
    ));
    lines.push(format!("9{}", " ".repeat(119)));

    for line in &lines {
        assert_eq!(line.len(), 120);
    }

    lines.join("\r\n").into_bytes()
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
    assert_eq!(json["header"]["bank_name"], "ﾃｽﾄｷﾞﾝｺｳ");
    assert_eq!(json["details"][0]["amount"], 1200);
    assert_eq!(
        json["details"][0]["customer_number"],
        "00000000001234567890"
    );
}

#[test]
fn parses_result_file_to_json() {
    let input_path = temp_input_path();
    fs::write(&input_path, sample_result_input()).unwrap();

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
    assert_eq!(json["header"]["collection_date"], "0422");
    assert_eq!(json["header"]["bank_name"], "BANK ALPHA");
    assert_eq!(json["details"][0]["account_holder_name"], "ALPHA INC");
    assert_eq!(json["details"][1]["result_code"], 1);
    assert_eq!(json["trailer"]["failure_count"], 1);
}

#[test]
fn reports_usage_without_an_input_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_zengin")).output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
