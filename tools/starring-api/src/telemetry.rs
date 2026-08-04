use std::io::{self, Write};

use authoring_application::ProductRequestIdV1;
use serde::Serialize;

use crate::error::MappedApplyErrorV1;

const MAX_APPLY_ERROR_RECORD_BYTES: usize = 512;

#[derive(Serialize)]
struct ApplyInternalErrorRecordV1<'a> {
    schema_version: u8,
    event: &'static str,
    operation: &'static str,
    request_id: &'a str,
    public_code: &'static str,
    internal_code: &'static str,
}

pub(crate) fn emit_apply_error(request_id: &ProductRequestIdV1, mapped: MappedApplyErrorV1) {
    let mut stderr = io::stderr().lock();
    let _write_result = write_apply_error(&mut stderr, request_id, mapped);
}

fn write_apply_error<W: Write>(
    writer: &mut W,
    request_id: &ProductRequestIdV1,
    mapped: MappedApplyErrorV1,
) -> io::Result<()> {
    let Some(internal_code) = mapped.internal_code() else {
        return Ok(());
    };
    let record = ApplyInternalErrorRecordV1 {
        schema_version: 1,
        event: "starring_api_internal_error",
        operation: "apply",
        request_id: request_id.as_str(),
        public_code: mapped.public_code(),
        internal_code,
    };
    let mut line = serde_json::to_vec(&record).map_err(io::Error::other)?;
    line.push(b'\n');
    if line.len() > MAX_APPLY_ERROR_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "apply error telemetry record exceeds bound",
        ));
    }
    writer.write_all(&line)
}

#[cfg(test)]
mod tests {
    use authoring_application::{ProductApplicationError, ProductControlPortError};

    use crate::error::map_apply_error;

    use super::*;

    #[test]
    fn unknown_backend_detail_is_replaced_by_an_allowlisted_code() {
        let secret = "unknown-secret\nresource=promotion-123";
        let mapped = map_apply_error(ProductApplicationError::Control(
            ProductControlPortError::Backend(secret.to_string()),
        ));
        let request_id = ProductRequestIdV1::parse("request-1").unwrap();
        let mut output = Vec::new();

        write_apply_error(&mut output, &request_id, mapped).unwrap();

        let line = String::from_utf8(output).unwrap();
        assert!(!line.contains(secret));
        assert!(!line.contains("unknown-secret"));
        assert!(!line.contains("promotion-123"));
        assert!(line.contains("\"internal_code\":\"control_backend\""));
    }

    #[test]
    fn serialization_is_single_line_and_bounded_for_the_largest_request_id() {
        let request_id = ProductRequestIdV1::parse(&"a".repeat(128)).unwrap();
        let mapped = map_apply_error(ProductApplicationError::InvalidProjection);
        let mut output = Vec::new();

        write_apply_error(&mut output, &request_id, mapped).unwrap();

        assert!(output.len() <= MAX_APPLY_ERROR_RECORD_BYTES);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_eq!(output.last(), Some(&b'\n'));
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["event"], "starring_api_internal_error");
        assert_eq!(value["operation"], "apply");
        assert_eq!(value["request_id"], request_id.as_str());
        assert_eq!(value["public_code"], "internal_error");
        assert_eq!(value["internal_code"], "invalid_projection");
    }

    #[test]
    fn client_faults_emit_no_internal_error_record() {
        let request_id = ProductRequestIdV1::parse("request-1").unwrap();
        let mapped = map_apply_error(ProductApplicationError::Control(
            ProductControlPortError::RevisionConflict,
        ));
        let mut output = Vec::new();

        write_apply_error(&mut output, &request_id, mapped).unwrap();

        assert!(output.is_empty());
    }
}
