use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use thiserror::Error;

const MIN_PORT: u16 = 1024;
const CONTROL_SOCKET_NAME: &str = "transport-control.sock";

#[derive(Clone)]
pub struct Config {
    root: PathBuf,
    run_id: String,
    guild_id: String,
    hub_channel_id: String,
    actor_id: String,
    bot_user_id: String,
    gateway_listen: SocketAddrV4,
    http_listen: SocketAddrV4,
    gateway_upstream: String,
    http_upstream: String,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("argument_contract_invalid")]
    ArgumentContract,
    #[error("root_invalid")]
    Root,
    #[error("identity_invalid")]
    Identity,
    #[error("listen_address_invalid")]
    ListenAddress,
}

impl ConfigError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ArgumentContract => "argument_contract_invalid",
            Self::Root => "root_invalid",
            Self::Identity => "identity_invalid",
            Self::ListenAddress => "listen_address_invalid",
        }
    }
}

impl Config {
    pub fn from_process_arguments() -> Result<Self, ConfigError> {
        Self::from_arguments(env::args().skip(1))
    }

    fn from_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        let mut arguments = arguments.into_iter();
        while let Some(name) = arguments.next() {
            if !matches!(
                name.as_str(),
                "--root"
                    | "--run-id"
                    | "--guild-id"
                    | "--hub-channel-id"
                    | "--actor-id"
                    | "--bot-user-id"
                    | "--gateway-listen"
                    | "--http-listen"
            ) {
                return Err(ConfigError::ArgumentContract);
            }
            let value = arguments.next().ok_or(ConfigError::ArgumentContract)?;
            if values.insert(name, value).is_some() {
                return Err(ConfigError::ArgumentContract);
            }
        }
        if values.len() != 8 {
            return Err(ConfigError::ArgumentContract);
        }
        let root = PathBuf::from(
            values
                .remove("--root")
                .ok_or(ConfigError::ArgumentContract)?,
        );
        let run_id = values
            .remove("--run-id")
            .ok_or(ConfigError::ArgumentContract)?;
        let guild_id = values
            .remove("--guild-id")
            .ok_or(ConfigError::ArgumentContract)?;
        let hub_channel_id = values
            .remove("--hub-channel-id")
            .ok_or(ConfigError::ArgumentContract)?;
        let actor_id = values
            .remove("--actor-id")
            .ok_or(ConfigError::ArgumentContract)?;
        let bot_user_id = values
            .remove("--bot-user-id")
            .ok_or(ConfigError::ArgumentContract)?;
        let gateway_listen = parse_loopback(
            &values
                .remove("--gateway-listen")
                .ok_or(ConfigError::ArgumentContract)?,
        )?;
        let http_listen = parse_loopback(
            &values
                .remove("--http-listen")
                .ok_or(ConfigError::ArgumentContract)?,
        )?;
        Self::validated(
            root,
            run_id,
            guild_id,
            hub_channel_id,
            actor_id,
            bot_user_id,
            gateway_listen,
            http_listen,
            "wss://gateway.discord.gg/?v=10&encoding=json".to_owned(),
            "https://discord.com".to_owned(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn validated(
        root: PathBuf,
        run_id: String,
        guild_id: String,
        hub_channel_id: String,
        actor_id: String,
        bot_user_id: String,
        gateway_listen: SocketAddrV4,
        http_listen: SocketAddrV4,
        gateway_upstream: String,
        http_upstream: String,
    ) -> Result<Self, ConfigError> {
        let canonical_root = fs::canonicalize(&root).map_err(|_| ConfigError::Root)?;
        if !root.is_absolute() || canonical_root != root || !root_is_private(&canonical_root)? {
            return Err(ConfigError::Root);
        }
        if !valid_run_id(&run_id)
            || !valid_snowflake(&guild_id)
            || !valid_snowflake(&hub_channel_id)
            || !valid_snowflake(&actor_id)
            || !valid_snowflake(&bot_user_id)
            || actor_id == bot_user_id
            || hub_channel_id == guild_id
            || hub_channel_id == actor_id
            || hub_channel_id == bot_user_id
        {
            return Err(ConfigError::Identity);
        }
        if gateway_listen == http_listen {
            return Err(ConfigError::ListenAddress);
        }
        Ok(Self {
            root: canonical_root,
            run_id,
            guild_id,
            hub_channel_id,
            actor_id,
            bot_user_id,
            gateway_listen,
            http_listen,
            gateway_upstream,
            http_upstream,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn guild_id(&self) -> &str {
        &self.guild_id
    }

    pub fn hub_channel_id(&self) -> &str {
        &self.hub_channel_id
    }

    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    pub fn bot_user_id(&self) -> &str {
        &self.bot_user_id
    }

    pub const fn gateway_listen(&self) -> SocketAddrV4 {
        self.gateway_listen
    }

    pub const fn http_listen(&self) -> SocketAddrV4 {
        self.http_listen
    }

    pub fn control_socket(&self) -> PathBuf {
        self.root.join(CONTROL_SOCKET_NAME)
    }

    pub fn gateway_upstream(&self) -> &str {
        &self.gateway_upstream
    }

    pub fn http_upstream(&self) -> &str {
        &self.http_upstream
    }

    pub fn gateway_proxy_url(&self) -> String {
        format!("ws://{}", self.gateway_listen)
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn for_test(
        root: PathBuf,
        guild_id: &str,
        hub_channel_id: &str,
        actor_id: &str,
        bot_user_id: &str,
        gateway_listen: SocketAddrV4,
        http_listen: SocketAddrV4,
        gateway_upstream: String,
        http_upstream: String,
    ) -> Self {
        let root = fs::canonicalize(root).expect("canonical test root");
        Self::validated(
            root,
            "d2-test-run".to_owned(),
            guild_id.to_owned(),
            hub_channel_id.to_owned(),
            actor_id.to_owned(),
            bot_user_id.to_owned(),
            gateway_listen,
            http_listen,
            gateway_upstream,
            http_upstream,
        )
        .expect("valid test configuration")
    }
}

fn parse_loopback(value: &str) -> Result<SocketAddrV4, ConfigError> {
    let address: SocketAddr = value.parse().map_err(|_| ConfigError::ListenAddress)?;
    let SocketAddr::V4(address) = address else {
        return Err(ConfigError::ListenAddress);
    };
    if *address.ip() != Ipv4Addr::LOCALHOST
        || address.port() < MIN_PORT
        || value != address.to_string()
    {
        return Err(ConfigError::ListenAddress);
    }
    Ok(address)
}

fn root_is_private(root: &Path) -> Result<bool, ConfigError> {
    let metadata = fs::symlink_metadata(root).map_err(|_| ConfigError::Root)?;
    Ok(metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o077 == 0)
}

fn valid_run_id(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value.as_bytes().iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
}

pub(crate) fn valid_snowflake(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && value.as_bytes().iter().all(u8::is_ascii_digit)
        && !value.starts_with('0')
        && value.parse::<u64>().is_ok_and(|number| number > 0)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn production_arguments_are_exact_and_loopback_only() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let canonical_root = fs::canonicalize(root.path()).unwrap();
        let arguments = [
            "--root",
            canonical_root.to_str().unwrap(),
            "--run-id",
            "d2-release-1",
            "--guild-id",
            "123456789",
            "--hub-channel-id",
            "444444444",
            "--actor-id",
            "987654321",
            "--bot-user-id",
            "555555555",
            "--gateway-listen",
            "127.0.0.1:21001",
            "--http-listen",
            "127.0.0.1:21002",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let config = Config::from_arguments(arguments.clone()).unwrap();
        assert_eq!(
            config.control_socket(),
            canonical_root.join(CONTROL_SOCKET_NAME)
        );
        assert_eq!(
            config.gateway_upstream(),
            "wss://gateway.discord.gg/?v=10&encoding=json"
        );
        assert_eq!(config.http_upstream(), "https://discord.com");
        assert_eq!(config.hub_channel_id(), "444444444");
        let mut missing = arguments.clone();
        missing.drain(6..8);
        assert_eq!(
            Config::from_arguments(missing).err(),
            Some(ConfigError::ArgumentContract)
        );
        let mut duplicate = arguments.clone();
        duplicate.extend(["--hub-channel-id".to_owned(), "444444445".to_owned()]);
        assert_eq!(
            Config::from_arguments(duplicate).err(),
            Some(ConfigError::ArgumentContract)
        );
        let mut unknown = arguments;
        unknown.extend(["--channel-id".to_owned(), "444444444".to_owned()]);
        assert_eq!(
            Config::from_arguments(unknown).err(),
            Some(ConfigError::ArgumentContract)
        );
    }

    #[test]
    fn unsafe_roots_addresses_and_identities_fail_closed() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let result = Config::validated(
            root.path().to_path_buf(),
            "d2-release-1".to_owned(),
            "7".to_owned(),
            "5".to_owned(),
            "8".to_owned(),
            "9".to_owned(),
            "127.0.0.1:21001".parse().unwrap(),
            "127.0.0.1:21002".parse().unwrap(),
            String::new(),
            String::new(),
        );
        assert_eq!(result.err(), Some(ConfigError::Root));
        assert!(!valid_run_id("../escape"));
        assert!(!valid_snowflake("01"));
        assert!(parse_loopback("0.0.0.0:21001").is_err());
        assert!(parse_loopback("127.0.0.1:80").is_err());
        assert!(parse_loopback("[::1]:21001").is_err());

        let private_root = TempDir::new().unwrap();
        fs::set_permissions(private_root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let canonical_root = fs::canonicalize(private_root.path()).unwrap();
        for hub_channel_id in ["0", "7", "8", "9"] {
            let result = Config::validated(
                canonical_root.clone(),
                "d2-release-1".to_owned(),
                "7".to_owned(),
                hub_channel_id.to_owned(),
                "8".to_owned(),
                "9".to_owned(),
                "127.0.0.1:21001".parse().unwrap(),
                "127.0.0.1:21002".parse().unwrap(),
                String::new(),
                String::new(),
            );
            assert_eq!(result.err(), Some(ConfigError::Identity));
        }
    }
}
