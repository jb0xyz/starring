use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::process::ExitCode;

use sqlx::postgres::PgConnectOptions;
use starring_db_bootstrap::{
    bootstrap_staging_database_with_authentication, parse_admin_connect_options,
    peer_bootstrap_connect_options, BootstrapAuthenticationV1, StagingAcknowledgementV1,
};
use zeroize::Zeroizing;

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

fn read_admin_url() -> Result<Zeroizing<String>, ()> {
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
    let (options, authentication, acknowledgement): (
        PgConnectOptions,
        BootstrapAuthenticationV1,
        StagingAcknowledgementV1,
    ) = match arguments.as_slice() {
        [system_identifier, acknowledgement] => {
            let acknowledgement =
                match StagingAcknowledgementV1::parse(system_identifier, acknowledgement) {
                    Ok(acknowledgement) => acknowledgement,
                    Err(error) => {
                        eprintln!("{}", error.code());
                        return ExitCode::from(64);
                    }
                };
            let admin_url = match read_admin_url() {
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
            (
                options,
                BootstrapAuthenticationV1::AuthenticatedUrl,
                acknowledgement,
            )
        }
        [mode, system_identifier, acknowledgement] if *mode == "--peer-bootstrap" => {
            let acknowledgement =
                match StagingAcknowledgementV1::parse(system_identifier, acknowledgement) {
                    Ok(acknowledgement) => acknowledgement,
                    Err(error) => {
                        eprintln!("{}", error.code());
                        return ExitCode::from(64);
                    }
                };
            (
                peer_bootstrap_connect_options(),
                BootstrapAuthenticationV1::TemporaryPeer,
                acknowledgement,
            )
        }
        _ => {
            eprintln!("command_line_arguments_not_allowed");
            return ExitCode::from(64);
        }
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
