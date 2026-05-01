use serde::{Deserialize, Serialize};

use crate::{CodeDivision, Encoding, Error, OutputFormat, fixed};

const RECORD_LEN: usize = 120;
const SUPPORTED_KIND_CODES: [u8; 4] = [11, 12, 71, 72];

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
    pub company_code: String,
    pub company_name: String,
    pub payment_date: String,
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
    pub account_holder_name: String,
    pub amount: u64,
    #[serde(default)]
    pub new_code: String,
    #[serde(default)]
    pub employee_number: String,
    #[serde(default)]
    pub department_code: String,
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
        company_code: fixed::parse_digit_string(record, 4..14, "header", "company_code")?,
        company_name: fixed::parse_required_text(record, 14..54, "header", "company_name")?,
        payment_date: fixed::parse_digit_string(record, 54..58, "header", "payment_date")?,
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
    fixed::ensure_spaces(record, 111..120, "detail", "padding")?;

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
        account_holder_name: fixed::parse_required_text(
            record,
            50..80,
            "detail",
            "account_holder_name",
        )?,
        amount: fixed::parse_number(record, 80..90, "detail", "amount")?,
        new_code: fixed::parse_optional_text(record, 90..91, "detail", "new_code")?,
        employee_number: fixed::parse_optional_text(record, 91..101, "detail", "employee_number")?,
        department_code: fixed::parse_optional_text(record, 101..111, "detail", "department_code")?,
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
        &header.company_code,
        "header",
        "company_code",
    )?;
    fixed::put_required_text(
        &mut record,
        14..54,
        &header.company_name,
        "header",
        "company_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        54..58,
        &header.payment_date,
        "header",
        "payment_date",
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
        &detail.account_holder_name,
        "detail",
        "account_holder_name",
        encoding,
    )?;
    fixed::put_number(&mut record, 80..90, detail.amount, "detail", "amount")?;
    fixed::put_optional_digits(&mut record, 90..91, &detail.new_code, "detail", "new_code")?;
    fixed::put_optional_digits(
        &mut record,
        91..101,
        &detail.employee_number,
        "detail",
        "employee_number",
    )?;
    fixed::put_optional_digits(
        &mut record,
        101..111,
        &detail.department_code,
        "detail",
        "department_code",
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
    if !SUPPORTED_KIND_CODES.contains(&header.kind_code) {
        return Err(Error::Validation(format!(
            "header kind_code must be one of 11, 12, 71, or 72, got {}",
            header.kind_code
        )));
    }

    fixed::ensure_supported_code_division(header.code_division, "header", "code_division")?;
    fixed::validate_digit_str("header", "company_code", &header.company_code, 10)?;
    fixed::validate_text_value("header", "company_name", &header.company_name)?;
    fixed::validate_digit_str("header", "payment_date", &header.payment_date, 4)?;
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
    fixed::validate_text_value("detail", "account_holder_name", &detail.account_holder_name)?;
    fixed::validate_numeric_width("detail", "amount", detail.amount, 10)?;
    fixed::validate_optional_digit_str("detail", "new_code", &detail.new_code, 1)?;
    fixed::validate_optional_digit_str("detail", "employee_number", &detail.employee_number, 10)?;
    fixed::validate_optional_digit_str("detail", "department_code", &detail.department_code, 10)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Header, Trailer, parse};
    use crate::{CodeDivision, OutputFormat, to_bytes};

    fn sample_file(kind_code: u8) -> File {
        File {
            header: Header {
                kind_code,
                code_division: CodeDivision::Jis,
                company_code: "1234567890".to_string(),
                company_name: "ACME PAYROLL".to_string(),
                payment_date: "0425".to_string(),
                bank_code: "0001".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "123".to_string(),
                branch_name: "MAIN".to_string(),
                account_type: 1,
                account_number: "7654321".to_string(),
            },
            details: vec![Detail {
                bank_code: "0005".to_string(),
                bank_name: "BANK BETA".to_string(),
                branch_code: "001".to_string(),
                branch_name: "WEST".to_string(),
                clearing_house_number: String::new(),
                account_type: 1,
                account_number: "1234567".to_string(),
                account_holder_name: "TARO YAMADA".to_string(),
                amount: 250000,
                new_code: "0".to_string(),
                employee_number: "0000001001".to_string(),
                department_code: "0000002002".to_string(),
            }],
            trailer: Trailer {
                total_count: 1,
                total_amount: 250000,
            },
            end: End,
        }
    }

    #[test]
    fn roundtrips_salary_transfer() {
        let file = sample_file(11);
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        let lines = encoded
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .collect::<Vec<_>>();
        for line in &lines {
            assert_eq!(line.len(), 120);
        }

        assert_eq!(&lines[0][1..3], b"11");
        assert_eq!(&lines[1][91..101], b"0000001001");
        assert_eq!(&lines[1][101..111], b"0000002002");
        assert_eq!(&lines[2][1..7], b"000001");

        let decoded = parse(&encoded).unwrap();
        assert_eq!(decoded, file);
    }

    #[test]
    fn roundtrips_bonus_transfer() {
        let file = sample_file(12);
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        let decoded = parse(&encoded).unwrap();
        assert_eq!(decoded.header.kind_code, 12);
        assert_eq!(decoded, file);
    }
}
