use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::process::ExitCode;

use sqlx::postgres::PgConnectOptions;
use starring_db_bootstrap::{
    bootstrap_staging_database_with_authentication, parse_admin_connect_options,
    parse_keychain_admin_connect_options, peer_bootstrap_connect_options,
    read_admin_url_from_keychain, BootstrapAuthenticationV1, StagingAcknowledgementV1,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdminInputModeV1 {
    Interactive,
    Keychain,
    TemporaryPeer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CommandArgumentsV1<'a> {
    mode: AdminInputModeV1,
    system_identifier: &'a str,
    acknowledgement: &'a str,
}

struct EchoGuard {
    fd: RawFd,
    original: libc::termios,
    armed: bool,
}

impl EchoGuard {
    fn disable(fd: RawFd) -> Result<Self, ()> {
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(());
        }
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } != 0 {
            return Err(());
        }
        Ok(Self {
            fd,
            original,
            armed: true,
        })
    }

    fn restore(&mut self) -> Result<(), ()> {
        if self.armed {
            if unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.original) } != 0 {
                return Err(());
            }
            self.armed = false;
        }
        Ok(())
    }
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        if self.armed {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.original);
            }
        }
    }
}

fn read_interactive_admin_url() -> Result<Zeroizing<String>, ()> {
    let mut terminal = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| ())?;
    let mut guard = EchoGuard::disable(terminal.as_raw_fd())?;
    terminal
        .write_all(b"PostgreSQL cluster-administrator URL: ")
        .map_err(|_| ())?;
    terminal.flush().map_err(|_| ())?;
    let reader_file = terminal.try_clone().map_err(|_| ())?;
    let mut reader = BufReader::new(reader_file);
    let mut value = Zeroizing::new(String::new());
    let read = reader.read_line(&mut value).map_err(|_| ())?;
    guard.restore()?;
    terminal.write_all(b"\n").map_err(|_| ())?;
    terminal.flush().map_err(|_| ())?;
    if read == 0 {
        return Err(());
    }
    while value.ends_with(['\r', '\n']) {
        value.pop();
    }
    if value.is_empty() {
        return Err(());
    }
    Ok(value)
}

fn postgres_environment_is_present() -> bool {
    std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("PG"))
}

fn parse_command_arguments<'a>(arguments: &[&'a str]) -> Result<CommandArgumentsV1<'a>, ()> {
    match arguments {
        [system_identifier, acknowledgement] if !system_identifier.starts_with('-') => {
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::Interactive,
                system_identifier,
                acknowledgement,
            })
        }
        [mode, system_identifier, acknowledgement] if *mode == "--keychain-admin" => {
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::Keychain,
                system_identifier,
                acknowledgement,
            })
        }
        [mode, system_identifier, acknowledgement] if *mode == "--peer-bootstrap" => {
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::TemporaryPeer,
                system_identifier,
                acknowledgement,
            })
        }
        _ => Err(()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if postgres_environment_is_present() {
        eprintln!("postgres_environment_not_allowed");
        return ExitCode::from(64);
    }
    let raw_arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(arguments) = raw_arguments
        .iter()
        .map(|argument| argument.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        eprintln!("command_line_arguments_not_allowed");
        return ExitCode::from(64);
    };
    let command = match parse_command_arguments(&arguments) {
        Ok(command) => command,
        Err(()) => {
            eprintln!("command_line_arguments_not_allowed");
            return ExitCode::from(64);
        }
    };
    let acknowledgement =
        match StagingAcknowledgementV1::parse(command.system_identifier, command.acknowledgement) {
            Ok(acknowledgement) => acknowledgement,
            Err(error) => {
                eprintln!("{}", error.code());
                return ExitCode::from(64);
            }
        };
    let (options, authentication): (PgConnectOptions, BootstrapAuthenticationV1) =
        match command.mode {
            AdminInputModeV1::Interactive => {
                let admin_url = match read_interactive_admin_url() {
                    Ok(value) => value,
                    Err(()) => {
                        eprintln!("admin_url_input_failed");
                        return ExitCode::from(1);
                    }
                };
                let options = match parse_admin_connect_options(&admin_url) {
                    Ok(options) => options,
                    Err(error) => {
                        eprintln!("{}", error.code());
                        return ExitCode::from(1);
                    }
                };
                drop(admin_url);
                (options, BootstrapAuthenticationV1::AuthenticatedUrl)
            }
            AdminInputModeV1::Keychain => {
                let admin_url = match read_admin_url_from_keychain() {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("{}", error.code());
                        return ExitCode::from(1);
                    }
                };
                let options = match parse_keychain_admin_connect_options(&admin_url) {
                    Ok(options) => options,
                    Err(error) => {
                        eprintln!("{}", error.code());
                        return ExitCode::from(1);
                    }
                };
                drop(admin_url);
                (options, BootstrapAuthenticationV1::AuthenticatedUrl)
            }
            AdminInputModeV1::TemporaryPeer => (
                peer_bootstrap_connect_options(),
                BootstrapAuthenticationV1::TemporaryPeer,
            ),
        };
    match bootstrap_staging_database_with_authentication(options, authentication, acknowledgement)
        .await
    {
        Ok(report) => {
            println!(
                "database=starring_runtime_staging owner=starring_owner migrations={} relations={} capability_functions={}",
                report.migration_count(),
                report.relation_count(),
                report.capability_function_count()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.code());
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_IDENTIFIER: &str = "7623456789012345678";
    const ACKNOWLEDGEMENT: &str = "acknowledgement";

    #[test]
    fn command_dispatch_preserves_existing_modes_and_adds_explicit_keychain_mode() {
        assert_eq!(
            parse_command_arguments(&[SYSTEM_IDENTIFIER, ACKNOWLEDGEMENT]),
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::Interactive,
                system_identifier: SYSTEM_IDENTIFIER,
                acknowledgement: ACKNOWLEDGEMENT,
            })
        );
        assert_eq!(
            parse_command_arguments(&["--keychain-admin", SYSTEM_IDENTIFIER, ACKNOWLEDGEMENT]),
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::Keychain,
                system_identifier: SYSTEM_IDENTIFIER,
                acknowledgement: ACKNOWLEDGEMENT,
            })
        );
        assert_eq!(
            parse_command_arguments(&["--peer-bootstrap", SYSTEM_IDENTIFIER, ACKNOWLEDGEMENT]),
            Ok(CommandArgumentsV1 {
                mode: AdminInputModeV1::TemporaryPeer,
                system_identifier: SYSTEM_IDENTIFIER,
                acknowledgement: ACKNOWLEDGEMENT,
            })
        );
    }

    #[test]
    fn command_dispatch_rejects_implicit_or_ambiguous_modes() {
        for arguments in [
            Vec::<&str>::new(),
            vec!["--keychain-admin"],
            vec!["--keychain-admin", SYSTEM_IDENTIFIER],
            vec!["--unknown", SYSTEM_IDENTIFIER, ACKNOWLEDGEMENT],
            vec![
                "--keychain-admin",
                "--peer-bootstrap",
                SYSTEM_IDENTIFIER,
                ACKNOWLEDGEMENT,
            ],
            vec![SYSTEM_IDENTIFIER, ACKNOWLEDGEMENT, "extra"],
        ] {
            assert_eq!(parse_command_arguments(&arguments), Err(()));
        }
    }
}
