use serde::{Deserialize, Serialize};

use crate::{CodeDivision, Encoding, Error, OutputFormat, fixed};

const RECORD_LEN: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub header: Header,
    pub details: Vec<Detail>,
    pub trailer: Trailer,
    pub end: End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub kind_code: u8,
    #[serde(default)]
    pub code_division: CodeDivision,
    pub collector_code: String,
    pub collector_name: String,
    pub collection_date: String,
    #[serde(default)]
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    #[serde(default)]
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
    pub payer_name: String,
    pub amount: u64,
    #[serde(default)]
    pub new_code: String,
    /// Bank-specific 20-byte customer-code payload carried in bytes 92..111 of the request record.
    ///
    /// This crate intentionally does not normalize this field across banks. Callers must provide
    /// and interpret the wire representation themselves.
    ///
    /// Current wire behavior:
    /// - If the value is shorter than 20 bytes, it is left-aligned and the remaining bytes are
    ///   padded with ASCII spaces (`0x20`).
    /// - On parse, trailing ASCII spaces are trimmed.
    ///
    /// This matters in particular for Yucho (`ゆうちょBizダイレクト`), whose spec defines this
    /// 20-byte area as `支払人コード1` (10 bytes) + `支払人コード2` (10 bytes). This type does not
    /// split or join those subfields for you. If you need Yucho's 10+10 layout, encode and decode
    /// it on the caller side before passing it to this field.
    ///
    /// It also matters for banks whose specs require zero-filled numeric customer codes rather than
    /// space padding. In those cases, callers should pass the exact 20-byte wire value expected by
    /// the bank instead of relying on this crate to apply bank-specific padding rules.
    #[serde(default)]
    pub customer_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub record_count: u32,
    pub total_amount: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct End;

pub(crate) fn parse(input: &[u8]) -> Result<File, Error> {
    let records = fixed::split_records(input, RECORD_LEN)?;
    if records.len() < 4 {
        return Err(Error::InvalidInput(
            "expected header, detail, trailer, and end records".to_string(),
        ));
    }

    let header = parse_header(records[0])?;
    let trailer = parse_trailer(records[records.len() - 2])?;
    let end = parse_end(records[records.len() - 1])?;
    let details = records[1..records.len() - 2]
        .iter()
        .map(|record| parse_detail(record))
        .collect::<Result<Vec<_>, _>>()?;

    let file = File {
        header,
        details,
        trailer,
        end,
    };
    file.validate()?;
    Ok(file)
}

pub(crate) fn write(file: &File, format: OutputFormat) -> Result<Vec<u8>, Error> {
    file.validate()?;
    fixed::ensure_supported_output_encoding(format.encoding)?;

    let mut records = Vec::with_capacity(file.details.len() + 3);
    records.push(render_header(&file.header, format.encoding)?);
    records.extend(render_details(&file.details, format.encoding)?);
    records.push(render_trailer(&file.trailer)?);
    records.push(render_end(&file.end));

    Ok(fixed::write_records(records, format))
}

impl File {
    fn validate(&self) -> Result<(), Error> {
        validate_header(&self.header)?;

        if self.details.is_empty() {
            return Err(Error::Validation(
                "at least one detail record is required".to_string(),
            ));
        }

        for detail in &self.details {
            validate_detail(detail)?;
        }

        let expected_count = self.details.len() as u32;
        if self.trailer.record_count != expected_count {
            return Err(Error::Validation(format!(
                "trailer record_count must be {expected_count}, got {}",
                self.trailer.record_count
            )));
        }

        let expected_total = self
            .details
            .iter()
            .try_fold(0_u64, |sum, detail| sum.checked_add(detail.amount))
            .ok_or_else(|| Error::Validation("detail amount sum overflowed u64".to_string()))?;

        if self.trailer.total_amount != expected_total {
            return Err(Error::Validation(format!(
                "trailer total_amount must be {expected_total}, got {}",
                self.trailer.total_amount
            )));
        }

        if self.trailer.record_count > 999_999 {
            return Err(Error::Validation(
                "trailer record_count exceeds 6 digits".to_string(),
            ));
        }

        if self.trailer.total_amount > 999_999_999_999 {
            return Err(Error::Validation(
                "trailer total_amount exceeds 12 digits".to_string(),
            ));
        }

        Ok(())
    }
}

fn parse_header(record: &[u8]) -> Result<Header, Error> {
    fixed::ensure_record_type(record, "header", b'1')?;

    let code_division = fixed::parse_code_division(record, 3..4, "header", "code_division")?;
    fixed::ensure_supported_code_division(code_division, "header", "code_division")?;
    fixed::ensure_spaces(record, 103..120, "header", "padding")?;

    Ok(Header {
        kind_code: fixed::parse_number(record, 1..3, "header", "kind_code")? as u8,
        code_division,
        collector_code: fixed::parse_digit_string(record, 4..14, "header", "collector_code")?,
        collector_name: fixed::parse_required_text(record, 14..54, "header", "collector_name")?,
        collection_date: fixed::parse_digit_string(record, 54..58, "header", "collection_date")?,
        bank_code: fixed::parse_optional_text(record, 58..62, "header", "bank_code")?,
        bank_name: fixed::parse_optional_text(record, 62..77, "header", "bank_name")?,
        branch_code: fixed::parse_digit_string(record, 77..80, "header", "branch_code")?,
        branch_name: fixed::parse_optional_text(record, 80..95, "header", "branch_name")?,
        account_type: fixed::parse_number(record, 95..96, "header", "account_type")? as u8,
        account_number: fixed::parse_digit_string(record, 96..103, "header", "account_number")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<Detail, Error> {
    fixed::ensure_record_type(record, "detail", b'2')?;
    fixed::ensure_spaces(record, 38..42, "detail", "bank_padding")?;
    fixed::ensure_spaces(record, 112..120, "detail", "padding")?;

    let result_code = fixed::parse_number(record, 111..112, "detail", "result_code")? as u8;
    if result_code != 0 {
        return Err(Error::InvalidField {
            record: "detail",
            field: "result_code",
            message: format!("must be 0 for request files, got {result_code}"),
        });
    }

    Ok(Detail {
        bank_code: fixed::parse_optional_text(record, 1..5, "detail", "bank_code")?,
        bank_name: fixed::parse_optional_text(record, 5..20, "detail", "bank_name")?,
        branch_code: fixed::parse_digit_string(record, 20..23, "detail", "branch_code")?,
        branch_name: fixed::parse_optional_text(record, 23..38, "detail", "branch_name")?,
        account_type: fixed::parse_number(record, 42..43, "detail", "account_type")? as u8,
        account_number: fixed::parse_digit_string(record, 43..50, "detail", "account_number")?,
        payer_name: fixed::parse_required_text(record, 50..80, "detail", "payer_name")?,
        amount: fixed::parse_number(record, 80..90, "detail", "amount")?,
        new_code: fixed::parse_optional_text(record, 90..91, "detail", "new_code")?,
        customer_number: fixed::parse_optional_text(record, 91..111, "detail", "customer_number")?,
    })
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    fixed::ensure_record_type(record, "trailer", b'8')?;
    fixed::ensure_spaces(record, 55..120, "trailer", "padding")?;

    let success_count = fixed::parse_number(record, 19..25, "trailer", "success_count")?;
    let success_amount = fixed::parse_number(record, 25..37, "trailer", "success_amount")?;
    let failure_count = fixed::parse_number(record, 37..43, "trailer", "failure_count")?;
    let failure_amount = fixed::parse_number(record, 43..55, "trailer", "failure_amount")?;

    for (field, value) in [
        ("success_count", success_count),
        ("success_amount", success_amount),
        ("failure_count", failure_count),
        ("failure_amount", failure_amount),
    ] {
        if value != 0 {
            return Err(Error::InvalidField {
                record: "trailer",
                field,
                message: format!("must be 0 for request files, got {value}"),
            });
        }
    }

    Ok(Trailer {
        record_count: fixed::parse_number(record, 1..7, "trailer", "record_count")? as u32,
        total_amount: fixed::parse_number(record, 7..19, "trailer", "total_amount")?,
    })
}

fn parse_end(record: &[u8]) -> Result<End, Error> {
    fixed::ensure_record_type(record, "end", b'9')?;
    fixed::ensure_spaces(record, 1..120, "end", "padding")?;
    Ok(End)
}

fn render_header(header: &Header, encoding: Encoding) -> Result<[u8; RECORD_LEN], Error> {
    validate_header(header)?;

    let mut record = fixed::blank_record(b'1');
    fixed::put_number(
        &mut record,
        1..3,
        header.kind_code.into(),
        "header",
        "kind_code",
    )?;
    fixed::put_code_division(
        &mut record,
        3..4,
        header.code_division,
        "header",
        "code_division",
    )?;
    fixed::put_digits(
        &mut record,
        4..14,
        &header.collector_code,
        "header",
        "collector_code",
    )?;
    fixed::put_required_text(
        &mut record,
        14..54,
        &header.collector_name,
        "header",
        "collector_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        54..58,
        &header.collection_date,
        "header",
        "collection_date",
    )?;
    fixed::put_optional_text(
        &mut record,
        58..62,
        &header.bank_code,
        "header",
        "bank_code",
        encoding,
    )?;
    fixed::put_optional_text(
        &mut record,
        62..77,
        &header.bank_name,
        "header",
        "bank_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        77..80,
        &header.branch_code,
        "header",
        "branch_code",
    )?;
    fixed::put_optional_text(
        &mut record,
        80..95,
        &header.branch_name,
        "header",
        "branch_name",
        encoding,
    )?;
    fixed::put_number(
        &mut record,
        95..96,
        header.account_type.into(),
        "header",
        "account_type",
    )?;
    fixed::put_digits(
        &mut record,
        96..103,
        &header.account_number,
        "header",
        "account_number",
    )?;
    Ok(record)
}

fn render_details(details: &[Detail], encoding: Encoding) -> Result<Vec<[u8; RECORD_LEN]>, Error> {
    details
        .iter()
        .map(|detail| render_detail(detail, encoding))
        .collect()
}

fn render_detail(detail: &Detail, encoding: Encoding) -> Result<[u8; RECORD_LEN], Error> {
    validate_detail(detail)?;

    let mut record = fixed::blank_record(b'2');
    fixed::put_optional_text(
        &mut record,
        1..5,
        &detail.bank_code,
        "detail",
        "bank_code",
        encoding,
    )?;
    fixed::put_optional_text(
        &mut record,
        5..20,
        &detail.bank_name,
        "detail",
        "bank_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        20..23,
        &detail.branch_code,
        "detail",
        "branch_code",
    )?;
    fixed::put_optional_text(
        &mut record,
        23..38,
        &detail.branch_name,
        "detail",
        "branch_name",
        encoding,
    )?;
    fixed::put_number(
        &mut record,
        42..43,
        detail.account_type.into(),
        "detail",
        "account_type",
    )?;
    fixed::put_digits(
        &mut record,
        43..50,
        &detail.account_number,
        "detail",
        "account_number",
    )?;
    fixed::put_required_text(
        &mut record,
        50..80,
        &detail.payer_name,
        "detail",
        "payer_name",
        encoding,
    )?;
    fixed::put_number(&mut record, 80..90, detail.amount, "detail", "amount")?;
    fixed::put_optional_text(
        &mut record,
        90..91,
        &detail.new_code,
        "detail",
        "new_code",
        encoding,
    )?;
    fixed::put_optional_text(
        &mut record,
        91..111,
        &detail.customer_number,
        "detail",
        "customer_number",
        encoding,
    )?;
    record[111] = b'0';
    Ok(record)
}

fn render_trailer(trailer: &Trailer) -> Result<[u8; RECORD_LEN], Error> {
    let mut record = fixed::blank_record(b'8');
    fixed::put_number(
        &mut record,
        1..7,
        trailer.record_count.into(),
        "trailer",
        "record_count",
    )?;
    fixed::put_number(
        &mut record,
        7..19,
        trailer.total_amount,
        "trailer",
        "total_amount",
    )?;
    fixed::put_number(&mut record, 19..25, 0, "trailer", "success_count")?;
    fixed::put_number(&mut record, 25..37, 0, "trailer", "success_amount")?;
    fixed::put_number(&mut record, 37..43, 0, "trailer", "failure_count")?;
    fixed::put_number(&mut record, 43..55, 0, "trailer", "failure_amount")?;
    Ok(record)
}

fn render_end(_end: &End) -> [u8; RECORD_LEN] {
    fixed::blank_record(b'9')
}

fn validate_header(header: &Header) -> Result<(), Error> {
    if header.kind_code != 91 {
        return Err(Error::Validation(format!(
            "header kind_code must be 91, got {}",
            header.kind_code
        )));
    }

    fixed::ensure_supported_code_division(header.code_division, "header", "code_division")?;
    fixed::validate_digit_str("header", "collector_code", &header.collector_code, 10)?;
    fixed::validate_text_value("header", "collector_name", &header.collector_name)?;
    fixed::validate_digit_str("header", "collection_date", &header.collection_date, 4)?;
    fixed::validate_text_value_allow_empty("header", "bank_code", &header.bank_code)?;
    fixed::validate_text_value_allow_empty("header", "bank_name", &header.bank_name)?;
    fixed::validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    fixed::validate_text_value_allow_empty("header", "branch_name", &header.branch_name)?;
    fixed::validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    fixed::validate_digit_str("header", "account_number", &header.account_number, 7)?;
    Ok(())
}

fn validate_detail(detail: &Detail) -> Result<(), Error> {
    fixed::validate_text_value_allow_empty("detail", "bank_code", &detail.bank_code)?;
    fixed::validate_text_value_allow_empty("detail", "bank_name", &detail.bank_name)?;
    fixed::validate_digit_str("detail", "branch_code", &detail.branch_code, 3)?;
    fixed::validate_text_value_allow_empty("detail", "branch_name", &detail.branch_name)?;
    fixed::validate_numeric_width("detail", "account_type", detail.account_type.into(), 1)?;
    fixed::validate_digit_str("detail", "account_number", &detail.account_number, 7)?;
    fixed::validate_text_value("detail", "payer_name", &detail.payer_name)?;
    fixed::validate_numeric_width("detail", "amount", detail.amount, 10)?;
    fixed::validate_text_value_allow_empty("detail", "new_code", &detail.new_code)?;
    fixed::validate_text_value_allow_empty("detail", "customer_number", &detail.customer_number)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Header, RECORD_LEN, Trailer, parse};
    use crate::{CodeDivision, Encoding, Error, LineEnding, OutputFormat, to_bytes};

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: CodeDivision::Jis,
                collector_code: "1234567890".to_string(),
                collector_name: "ACME COLLECTOR".to_string(),
                collection_date: "0422".to_string(),
                bank_code: "0185".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "040".to_string(),
                branch_name: "MAIN BRANCH".to_string(),
                account_type: 1,
                account_number: "5000001".to_string(),
            },
            details: vec![
                Detail {
                    bank_code: "0185".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "040".to_string(),
                    branch_name: "WEST".to_string(),
                    account_type: 1,
                    account_number: "6000001".to_string(),
                    payer_name: "ALPHA INC".to_string(),
                    amount: 1000,
                    new_code: "0".to_string(),
                    customer_number: "01234567890123450001".to_string(),
                },
                Detail {
                    bank_code: "0185".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "041".to_string(),
                    branch_name: "EAST".to_string(),
                    account_type: 2,
                    account_number: "6000002".to_string(),
                    payer_name: "BETA LLC".to_string(),
                    amount: 2000,
                    new_code: "1".to_string(),
                    customer_number: "01234567890123450002".to_string(),
                },
            ],
            trailer: Trailer {
                record_count: 2,
                total_amount: 3000,
            },
            end: End,
        }
    }

    fn pad_text(value: &str, width: usize) -> String {
        format!("{value:<width$}")
    }

    fn pad_number<T>(value: T, width: usize) -> String
    where
        T: core::fmt::Display,
    {
        format!("{value:0width$}")
    }

    fn sample_bytes() -> Vec<u8> {
        let file = sample_file();
        let mut lines = Vec::new();

        lines.push(
            format!(
                "1{:02}{}{}{}{}{}{}{}{}{}{}",
                file.header.kind_code,
                file.header.code_division.as_u8(),
                file.header.collector_code,
                pad_text(&file.header.collector_name, 40),
                file.header.collection_date,
                pad_text(&file.header.bank_code, 4),
                pad_text(&file.header.bank_name, 15),
                file.header.branch_code,
                pad_text(&file.header.branch_name, 15),
                file.header.account_type,
                file.header.account_number,
            ) + &" ".repeat(17),
        );

        for detail in &file.details {
            lines.push(
                format!(
                    "2{}{}{}{}{}{}{}{}{}{}{}",
                    pad_text(&detail.bank_code, 4),
                    pad_text(&detail.bank_name, 15),
                    detail.branch_code,
                    pad_text(&detail.branch_name, 15),
                    " ".repeat(4),
                    detail.account_type,
                    detail.account_number,
                    pad_text(&detail.payer_name, 30),
                    pad_number(detail.amount, 10),
                    pad_text(&detail.new_code, 1),
                    pad_text(&detail.customer_number, 20),
                ) + &format!("{}{}", 0, " ".repeat(8)),
            );
        }

        lines.push(
            format!(
                "8{}{}{}{}{}{}",
                pad_number(file.trailer.record_count, 6),
                pad_number(file.trailer.total_amount, 12),
                pad_number(0, 6),
                pad_number(0, 12),
                pad_number(0, 6),
                pad_number(0, 12),
            ) + &" ".repeat(65),
        );
        lines.push(format!("9{}", " ".repeat(119)));

        for line in &lines {
            assert_eq!(line.len(), 120);
        }

        lines.join("\r\n").into_bytes()
    }

    fn sample_canonical_bytes() -> Vec<u8> {
        sample_bytes()
            .into_iter()
            .filter(|byte| *byte != b'\r' && *byte != b'\n')
            .collect()
    }

    #[test]
    fn parses_request_file_layout() {
        let decoded = parse(&sample_bytes()).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn writes_documented_request_layout() {
        let encoded = to_bytes(
            &sample_file(),
            OutputFormat {
                encoding: Encoding::Jis,
                line_ending: LineEnding::None,
                eof: false,
            },
        )
        .unwrap();

        assert_eq!(encoded, sample_canonical_bytes());
    }

    #[test]
    fn short_customer_number_is_space_padded_on_write() {
        let mut file = sample_file();
        file.details[0].customer_number = "1043".to_string();

        let encoded = to_bytes(
            &file,
            OutputFormat {
                encoding: Encoding::Ascii,
                line_ending: LineEnding::None,
                eof: false,
            },
        )
        .unwrap();

        let first_detail_offset = RECORD_LEN;
        assert_eq!(
            &encoded[first_detail_offset + 91..first_detail_offset + 111],
            b"1043                "
        );
    }

    #[test]
    fn rejects_non_zero_result_codes() {
        let mut bytes = sample_bytes();
        let detail_offset = 122;
        bytes[detail_offset + 111] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("detail.result_code"));
    }

    #[test]
    fn rejects_non_zero_request_summary_fields() {
        let mut bytes = sample_bytes();
        let trailer_offset = 122 * 3;
        bytes[trailer_offset + 24] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("trailer.success_count"));
    }

    #[test]
    fn accepts_blank_optional_fields_from_yucho_style_layout() {
        let mut bytes = sample_bytes();

        bytes[58..77].fill(b' ');
        bytes[80..95].fill(b' ');

        let detail_offset = 122;
        bytes[detail_offset + 1..detail_offset + 20].fill(b' ');
        bytes[detail_offset + 23..detail_offset + 38].fill(b' ');
        bytes[detail_offset + 90] = b' ';
        bytes[detail_offset + 91..detail_offset + 111].fill(b' ');

        let decoded = parse(&bytes).unwrap();
        assert_eq!(decoded.header.bank_code, "");
        assert_eq!(decoded.header.bank_name, "");
        assert_eq!(decoded.header.branch_name, "");
        assert_eq!(decoded.details[0].bank_code, "");
        assert_eq!(decoded.details[0].bank_name, "");
        assert_eq!(decoded.details[0].branch_name, "");
        assert_eq!(decoded.details[0].new_code, "");
        assert_eq!(decoded.details[0].customer_number, "");
    }

    #[test]
    fn rejects_ebcdic_request_files() {
        let error = crate::to_bytes(
            &sample_file(),
            crate::OutputFormat {
                encoding: Encoding::Ebcdic,
                line_ending: crate::LineEnding::None,
                eof: false,
            },
        )
        .unwrap_err();

        assert!(matches!(
            error,
            Error::UnsupportedEncoding(Encoding::Ebcdic)
        ));
    }

    #[test]
    fn rejects_ebcdic_request_input() {
        let mut bytes = sample_bytes();
        bytes[3] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(matches!(
            error,
            Error::UnsupportedEncoding(Encoding::Ebcdic)
        ));
    }

    #[test]
    fn rejects_non_space_request_padding() {
        for (offset, field) in [
            (103, "header.padding"),
            (122 + 38, "detail.bank_padding"),
            (122 * 3 + 55, "trailer.padding"),
            (122 * 4 + 1, "end.padding"),
        ] {
            let mut bytes = sample_bytes();
            bytes[offset] = b'X';

            let error = parse(&bytes).unwrap_err();
            assert!(
                error.to_string().contains(field),
                "expected error to contain {field}, got {error}"
            );
        }
    }
}
