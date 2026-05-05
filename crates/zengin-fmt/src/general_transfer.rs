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
    pub remitter_code: String,
    pub remitter_name: String,
    pub transfer_date: String,
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
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    #[serde(default)]
    pub clearing_house_number: String,
    pub account_type: u8,
    pub account_number: String,
    pub recipient_name: String,
    pub amount: u64,
    #[serde(default)]
    pub new_code: String,
    #[serde(default)]
    pub customer_code1: String,
    #[serde(default)]
    pub customer_code2: String,
    #[serde(default)]
    pub edi_info: String,
    #[serde(default)]
    pub transfer_designated_type: String,
    #[serde(default)]
    pub identification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub total_count: u32,
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
        if self.trailer.total_count != expected_count {
            return Err(Error::Validation(format!(
                "trailer total_count must be {expected_count}, got {}",
                self.trailer.total_count
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

        fixed::validate_count_width("trailer", "total_count", self.trailer.total_count)?;
        fixed::validate_amount_width("trailer", "total_amount", self.trailer.total_amount)?;
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
        remitter_code: fixed::parse_digit_string(record, 4..14, "header", "remitter_code")?,
        remitter_name: fixed::parse_required_text(record, 14..54, "header", "remitter_name")?,
        transfer_date: fixed::parse_digit_string(record, 54..58, "header", "transfer_date")?,
        bank_code: fixed::parse_digit_string(record, 58..62, "header", "bank_code")?,
        bank_name: fixed::parse_optional_text(record, 62..77, "header", "bank_name")?,
        branch_code: fixed::parse_digit_string(record, 77..80, "header", "branch_code")?,
        branch_name: fixed::parse_optional_text(record, 80..95, "header", "branch_name")?,
        account_type: fixed::parse_number(record, 95..96, "header", "account_type")? as u8,
        account_number: fixed::parse_digit_string(record, 96..103, "header", "account_number")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<Detail, Error> {
    fixed::ensure_record_type(record, "detail", b'2')?;
    fixed::ensure_spaces(record, 113..120, "detail", "padding")?;

    let identification = fixed::parse_optional_text(record, 112..113, "detail", "identification")?;

    let (customer_code1, customer_code2, edi_info) = if identification == "Y" {
        (
            String::new(),
            String::new(),
            fixed::parse_optional_text(record, 91..111, "detail", "edi_info")?,
        )
    } else {
        (
            fixed::parse_optional_text(record, 91..101, "detail", "customer_code1")?,
            fixed::parse_optional_text(record, 101..111, "detail", "customer_code2")?,
            String::new(),
        )
    };

    Ok(Detail {
        bank_code: fixed::parse_digit_string(record, 1..5, "detail", "bank_code")?,
        bank_name: fixed::parse_optional_text(record, 5..20, "detail", "bank_name")?,
        branch_code: fixed::parse_digit_string(record, 20..23, "detail", "branch_code")?,
        branch_name: fixed::parse_optional_text(record, 23..38, "detail", "branch_name")?,
        clearing_house_number: fixed::parse_optional_text(
            record,
            38..42,
            "detail",
            "clearing_house_number",
        )?,
        account_type: fixed::parse_number(record, 42..43, "detail", "account_type")? as u8,
        account_number: fixed::parse_digit_string(record, 43..50, "detail", "account_number")?,
        recipient_name: fixed::parse_required_text(record, 50..80, "detail", "recipient_name")?,
        amount: fixed::parse_number(record, 80..90, "detail", "amount")?,
        new_code: fixed::parse_optional_text(record, 90..91, "detail", "new_code")?,
        customer_code1,
        customer_code2,
        edi_info,
        transfer_designated_type: fixed::parse_optional_text(
            record,
            111..112,
            "detail",
            "transfer_designated_type",
        )?,
        identification,
    })
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    fixed::ensure_record_type(record, "trailer", b'8')?;
    fixed::ensure_spaces(record, 19..120, "trailer", "padding")?;

    Ok(Trailer {
        total_count: fixed::parse_number(record, 1..7, "trailer", "total_count")? as u32,
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
        &header.remitter_code,
        "header",
        "remitter_code",
    )?;
    fixed::put_required_text(
        &mut record,
        14..54,
        &header.remitter_name,
        "header",
        "remitter_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        54..58,
        &header.transfer_date,
        "header",
        "transfer_date",
    )?;
    fixed::put_digits(
        &mut record,
        58..62,
        &header.bank_code,
        "header",
        "bank_code",
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
    fixed::put_digits(&mut record, 1..5, &detail.bank_code, "detail", "bank_code")?;
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
    fixed::put_optional_digits(
        &mut record,
        38..42,
        &detail.clearing_house_number,
        "detail",
        "clearing_house_number",
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
        &detail.recipient_name,
        "detail",
        "recipient_name",
        encoding,
    )?;
    fixed::put_number(&mut record, 80..90, detail.amount, "detail", "amount")?;
    fixed::put_optional_digits(&mut record, 90..91, &detail.new_code, "detail", "new_code")?;

    if detail.identification == "Y" {
        fixed::put_optional_text(
            &mut record,
            91..111,
            &detail.edi_info,
            "detail",
            "edi_info",
            encoding,
        )?;
    } else {
        fixed::put_optional_digits(
            &mut record,
            91..101,
            &detail.customer_code1,
            "detail",
            "customer_code1",
        )?;
        fixed::put_optional_digits(
            &mut record,
            101..111,
            &detail.customer_code2,
            "detail",
            "customer_code2",
        )?;
    }

    fixed::put_optional_digits(
        &mut record,
        111..112,
        &detail.transfer_designated_type,
        "detail",
        "transfer_designated_type",
    )?;
    fixed::put_optional_text(
        &mut record,
        112..113,
        &detail.identification,
        "detail",
        "identification",
        encoding,
    )?;
    Ok(record)
}

fn render_trailer(trailer: &Trailer) -> Result<[u8; RECORD_LEN], Error> {
    let mut record = fixed::blank_record(b'8');
    fixed::put_number(
        &mut record,
        1..7,
        trailer.total_count.into(),
        "trailer",
        "total_count",
    )?;
    fixed::put_number(
        &mut record,
        7..19,
        trailer.total_amount,
        "trailer",
        "total_amount",
    )?;
    Ok(record)
}

fn render_end(_end: &End) -> [u8; RECORD_LEN] {
    fixed::blank_record(b'9')
}

fn validate_header(header: &Header) -> Result<(), Error> {
    if header.kind_code != 21 {
        return Err(Error::Validation(format!(
            "header kind_code must be 21, got {}",
            header.kind_code
        )));
    }

    fixed::ensure_supported_code_division(header.code_division, "header", "code_division")?;
    fixed::validate_digit_str("header", "remitter_code", &header.remitter_code, 10)?;
    fixed::validate_text_value("header", "remitter_name", &header.remitter_name)?;
    fixed::validate_digit_str("header", "transfer_date", &header.transfer_date, 4)?;
    fixed::validate_digit_str("header", "bank_code", &header.bank_code, 4)?;
    fixed::validate_text_value_allow_empty("header", "bank_name", &header.bank_name)?;
    fixed::validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    fixed::validate_text_value_allow_empty("header", "branch_name", &header.branch_name)?;
    fixed::validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    fixed::validate_digit_str("header", "account_number", &header.account_number, 7)?;
    Ok(())
}

fn validate_detail(detail: &Detail) -> Result<(), Error> {
    fixed::validate_digit_str("detail", "bank_code", &detail.bank_code, 4)?;
    fixed::validate_text_value_allow_empty("detail", "bank_name", &detail.bank_name)?;
    fixed::validate_digit_str("detail", "branch_code", &detail.branch_code, 3)?;
    fixed::validate_text_value_allow_empty("detail", "branch_name", &detail.branch_name)?;
    fixed::validate_optional_digit_str(
        "detail",
        "clearing_house_number",
        &detail.clearing_house_number,
        4,
    )?;
    fixed::validate_numeric_width("detail", "account_type", detail.account_type.into(), 1)?;
    fixed::validate_digit_str("detail", "account_number", &detail.account_number, 7)?;
    fixed::validate_text_value("detail", "recipient_name", &detail.recipient_name)?;
    fixed::validate_numeric_width("detail", "amount", detail.amount, 10)?;
    fixed::validate_optional_digit_str("detail", "new_code", &detail.new_code, 1)?;
    fixed::validate_optional_digit_str("detail", "customer_code1", &detail.customer_code1, 10)?;
    fixed::validate_optional_digit_str("detail", "customer_code2", &detail.customer_code2, 10)?;
    fixed::validate_text_value_allow_empty("detail", "edi_info", &detail.edi_info)?;
    fixed::validate_optional_digit_str(
        "detail",
        "transfer_designated_type",
        &detail.transfer_designated_type,
        1,
    )?;
    fixed::validate_text_value_allow_empty("detail", "identification", &detail.identification)?;

    if detail.identification == "Y" {
        if !detail.customer_code1.is_empty() || !detail.customer_code2.is_empty() {
            return Err(Error::Validation(
                "customer codes must be empty when identification is Y".to_string(),
            ));
        }
    } else if !detail.edi_info.is_empty() {
        return Err(Error::Validation(
            "edi_info requires detail identification to be Y".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Header, Trailer, parse};
    use crate::{CodeDivision, OutputFormat, to_bytes};

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 21,
                code_division: CodeDivision::Jis,
                remitter_code: "1234567890".to_string(),
                remitter_name: "ACME REMITTER".to_string(),
                transfer_date: "0430".to_string(),
                bank_code: "0001".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "123".to_string(),
                branch_name: "MAIN".to_string(),
                account_type: 1,
                account_number: "7654321".to_string(),
            },
            details: vec![
                Detail {
                    bank_code: "0005".to_string(),
                    bank_name: "BANK BETA".to_string(),
                    branch_code: "001".to_string(),
                    branch_name: "WEST".to_string(),
                    clearing_house_number: String::new(),
                    account_type: 1,
                    account_number: "1234567".to_string(),
                    recipient_name: "TARO YAMADA".to_string(),
                    amount: 1200,
                    new_code: "0".to_string(),
                    customer_code1: "0000000001".to_string(),
                    customer_code2: "0000000002".to_string(),
                    edi_info: String::new(),
                    transfer_designated_type: "7".to_string(),
                    identification: String::new(),
                },
                Detail {
                    bank_code: "0006".to_string(),
                    bank_name: "BANK GAMMA".to_string(),
                    branch_code: "002".to_string(),
                    branch_name: "EAST".to_string(),
                    clearing_house_number: String::new(),
                    account_type: 2,
                    account_number: "7654321".to_string(),
                    recipient_name: "HANAKO SUZUKI".to_string(),
                    amount: 3400,
                    new_code: "1".to_string(),
                    customer_code1: String::new(),
                    customer_code2: String::new(),
                    edi_info: "EDI12345678901234567".to_string(),
                    transfer_designated_type: "8".to_string(),
                    identification: "Y".to_string(),
                },
            ],
            trailer: Trailer {
                total_count: 2,
                total_amount: 4600,
            },
            end: End,
        }
    }

    #[test]
    fn roundtrips_general_transfer() {
        let file = sample_file();
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        let lines = encoded
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .collect::<Vec<_>>();
        for line in &lines {
            assert_eq!(line.len(), 120);
        }

        assert_eq!(&lines[0][1..3], b"21");
        assert_eq!(&lines[1][91..101], b"0000000001");
        assert_eq!(&lines[1][101..111], b"0000000002");
        assert_eq!(&lines[2][91..111], b"EDI12345678901234567");
        assert_eq!(&lines[2][111..113], b"8Y");

        let decoded = parse(&encoded).unwrap();
        assert_eq!(decoded, file);
    }
}
