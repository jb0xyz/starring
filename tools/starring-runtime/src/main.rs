use std::io::Write;
use std::process::ExitCode;

use starring_runtime::{
    resolve_runtime_secrets_v1, RuntimeConfigErrorV1, RuntimeConfigV1,
    RuntimeSecretsResolutionErrorV1,
};

fn main() -> ExitCode {
    install_panic_hook();
    match RuntimeConfigV1::from_process_environment() {
        Ok(config) => match resolve_runtime_secrets_v1(&config) {
            Ok(_) => {
                emit_status("runtime_not_composed", None);
                ExitCode::from(70)
            }
            Err(error) => {
                emit_secret_error(error);
                ExitCode::from(78)
            }
        },
        Err(error) => {
            emit_configuration_error(error);
            ExitCode::from(78)
        }
    }
}

fn emit_secret_error(error: RuntimeSecretsResolutionErrorV1) {
    emit_status(error.code(), error.context());
}

fn emit_configuration_error(error: RuntimeConfigErrorV1) {
    let context = error.field().map(|field| field.code());
    emit_status(error.code(), context);
}

fn emit_status(status: &str, context: Option<&str>) {
    let mut stderr = std::io::stderr().lock();
    if let Some(context) = context {
        let _write_result = writeln!(stderr, "starring_runtime_status={status} context={context}");
    } else {
        let _write_result = writeln!(stderr, "starring_runtime_status={status}");
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        let _write_result = std::io::stderr().write_all(b"starring_runtime_status=panic\n");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_errors_emit_only_finite_status_context() {
        let error = RuntimeConfigErrorV1::InvalidValue(
            starring_runtime::RuntimeConfigurationFieldV1::HealthBindAddress,
        );
        assert_eq!(error.code(), "runtime_config_invalid_value");
        assert_eq!(
            error.field().map(|field| field.code()),
            Some("health_bind_address")
        );
    }
}
