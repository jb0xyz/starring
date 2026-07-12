use std::collections::BTreeMap;
use std::env;
use std::time::Duration;

use automation_core::RunningRuleSetIdentity;
use automation_instance::InstanceRuleSetVersion;
use automation_ruleset::{RuleSetKey, RuleSetStore};
use automation_runtime::gateway;
use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceKind,
    InstanceRef, InstanceResourceRefs, InteractionRule, InteractionRuleSet, ModalFieldSpec,
    ModalFieldStyle, ModalSpec, OverwriteTargetSpec, PanelSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, Permissions, RoleId, UserId};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::id::Id;

mod random_instance_id;

const DEFAULT_RULESET_KEY: &str = "studyroom_demo";
const BUTTON_KEY: &str = "create_study_room";
const MODAL_KEY: &str = "create_study_modal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixtureVariant {
    V1,
    V2,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Seed {
        variant: FixtureVariant,
        activate: bool,
        force_activate: bool,
    },
    Activate {
        version: u32,
    },
    Run,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedCli {
    ruleset_key: Option<String>,
    command: Command,
}

#[derive(Debug, PartialEq, Eq)]
enum CliError {
    MissingRulesetKeyValue,
    DuplicateRulesetKey,
    MissingVariantValue,
    DuplicateVariant,
    InvalidVariant(String),
    UnknownFlag(String),
    MissingVariantForSeed,
    VariantNotAllowed,
    ActivateFlagNotAllowed,
    MissingActivateVersion,
    InvalidActivateVersion(String),
    UnknownMode(String),
    UnexpectedPositional(String),
    InvalidRulesetKey(String),
}

fn parse_cli(args: &[String]) -> Result<ParsedCli, CliError> {
    let mut ruleset_key = None;
    let mut variant = None;
    let mut activate = false;
    let mut force_activate = false;
    let mut positionals = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--ruleset-key" => {
                if ruleset_key.is_some() {
                    return Err(CliError::DuplicateRulesetKey);
                }
                let value = args
                    .get(index + 1)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(CliError::MissingRulesetKeyValue)?;
                ruleset_key = Some(value.clone());
                index += 2;
            }
            "--variant" => {
                if variant.is_some() {
                    return Err(CliError::DuplicateVariant);
                }
                let value = args
                    .get(index + 1)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(CliError::MissingVariantValue)?;
                variant = Some(match value.as_str() {
                    "v1" => FixtureVariant::V1,
                    "v2" => FixtureVariant::V2,
                    _ => return Err(CliError::InvalidVariant(value.clone())),
                });
                index += 2;
            }
            "--activate" => {
                activate = true;
                index += 1;
            }
            "--force-activate" => {
                force_activate = true;
                index += 1;
            }
            value if value.starts_with("--") => {
                return Err(CliError::UnknownFlag(value.to_string()));
            }
            value => {
                positionals.push(value.to_string());
                index += 1;
            }
        }
    }

    let mode = positionals.first().map(String::as_str).unwrap_or("run");
    let command = match mode {
        "seed-studyroom" => {
            let variant = variant.ok_or(CliError::MissingVariantForSeed)?;
            if let Some(extra) = positionals.get(1) {
                return Err(CliError::UnexpectedPositional(extra.clone()));
            }
            Command::Seed {
                variant,
                activate,
                force_activate,
            }
        }
        "activate" => {
            if variant.is_some() {
                return Err(CliError::VariantNotAllowed);
            }
            if activate || force_activate {
                return Err(CliError::ActivateFlagNotAllowed);
            }
            let raw = positionals.get(1).ok_or(CliError::MissingActivateVersion)?;
            if let Some(extra) = positionals.get(2) {
                return Err(CliError::UnexpectedPositional(extra.clone()));
            }
            let version = raw
                .parse()
                .map_err(|_| CliError::InvalidActivateVersion(raw.clone()))?;
            Command::Activate { version }
        }
        "run" => {
            if variant.is_some() {
                return Err(CliError::VariantNotAllowed);
            }
            if activate || force_activate {
                return Err(CliError::ActivateFlagNotAllowed);
            }
            if let Some(extra) = positionals.get(1) {
                return Err(CliError::UnexpectedPositional(extra.clone()));
            }
            Command::Run
        }
        other => return Err(CliError::UnknownMode(other.to_string())),
    };

    Ok(ParsedCli {
        ruleset_key,
        command,
    })
}

fn resolve_ruleset_key(
    cli_value: Option<&str>,
    env_value: Option<&str>,
) -> Result<RuleSetKey, CliError> {
    let raw = cli_value.or(env_value).unwrap_or(DEFAULT_RULESET_KEY);
    RuleSetKey::parse(raw).map_err(|error| CliError::InvalidRulesetKey(format!("{error:?}")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse_cli(&args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!(
                "usage: interaction-smoke [--ruleset-key <key>] [run | activate <version> | seed-studyroom --variant <v1|v2> [--activate] [--force-activate]]"
            );
            return Err(format!("invalid command line: {error:?}").into());
        }
    };
    let env_ruleset_key = env::var("STARRING_RULESET_KEY").ok();
    let ruleset_key = match resolve_ruleset_key(
        parsed.ruleset_key.as_deref(),
        env_ruleset_key.as_deref(),
    ) {
        Ok(key) => key,
        Err(error) => {
            eprintln!(
                "usage: interaction-smoke [--ruleset-key <key>] [run | activate <version> | seed-studyroom --variant <v1|v2> [--activate] [--force-activate]]"
            );
            return Err(format!("invalid command line: {error:?}").into());
        }
    };

    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let database_url = env::var("STARRING_DATABASE_URL")?;

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .map_err(report_connect_error)?;
    automation_instance_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: instance migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;
    automation_ruleset_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: ruleset migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;
    automation_panel_installation_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: panel installation migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;

    let ruleset_store = automation_ruleset_postgres::PostgresRuleSetStore::new(pool.clone());
    match parsed.command {
        Command::Seed {
            variant,
            activate,
            force_activate,
        } => {
            return seed_studyroom(
                &ruleset_store,
                guild_id,
                &ruleset_key,
                variant,
                activate,
                force_activate,
            )
            .await;
        }
        Command::Activate {
            version: version_arg,
        } => {
            let version = automation_ruleset::RuleSetVersionId::new(version_arg)
                .map_err(|e| format!("invalid version: {e:?}"))?;
            if ruleset_store
                .get_version(GuildId(guild_id), &ruleset_key, version)
                .await
                .map_err(|e| format!("version lookup failed: {e:?}"))?
                .is_none()
            {
                return Err(format!("version {version} not found; nothing activated").into());
            }
            let token = env::var("DISCORD_TEST_TOKEN")?;
            let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
            let bindings = bindings(channel_id);
            let http = Client::new(token);
            let (caps, role_permissions) = readiness_context(&http, guild_id, &bindings).await?;
            let outcome = automation_ruleset_readiness::activate_if_ready(
                &ruleset_store,
                GuildId(guild_id),
                &ruleset_key,
                version,
                &bindings,
                &caps,
                &role_permissions,
            )
            .await
            .map_err(|e| format!("activation failed (active pointer unchanged): {e:?}"))?;
            eprintln!(
                "activated {} ({} notices)",
                outcome.activation.active_version,
                outcome.runtime_ruleset.notices.len()
            );
            return Ok(());
        }
        Command::Run => {}
    }

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let bindings = bindings(channel_id);

    let http = Client::new(token.clone());
    let (guild_capabilities, role_permissions) =
        readiness_context(&http, guild_id, &bindings).await?;

    let runtime = automation_ruleset_readiness::hydrate_active_ruleset(
        &ruleset_store,
        GuildId(guild_id),
        &ruleset_key,
        &bindings,
        &guild_capabilities,
        &role_permissions,
    )
    .await
    .map_err(|e| format!("hydration failed (fail-closed, not starting): {e:?}"))?;

    let instances = automation_instance_postgres::PostgresInstanceStore::new(pool.clone());
    let teardown = automation_instance_teardown::Teardown::new(
        automation_instance_postgres::PostgresInstanceStore::new(pool.clone()),
        automation_runtime::TwilightInstanceDeleter::new(&http),
    );
    match automation_runtime::resume_deleting_instances(
        GuildId(guild_id),
        &instances,
        &teardown,
        automation_runtime::ResumeConfig {
            max_concurrency: 4,
            per_instance_timeout: Duration::from_secs(10),
        },
    )
    .await
    {
        Ok(report) => {
            for entry in report.entries {
                eprintln!("teardown resume: {entry:?}");
            }
        }
        Err(error) => {
            eprintln!("teardown resume list failed; continuing startup: {error:?}");
        }
    }

    let installation_store =
        automation_panel_installation_postgres::PostgresPanelInstallationStore::new(pool.clone());
    let panel_installer = automation_runtime::TwilightPanelInstaller::new(&http);
    let install_report = automation_panel_installation::install_declared_panels(
        GuildId(guild_id),
        &runtime.ruleset_key,
        runtime.version,
        automation_runtime::PANEL_RENDER_REVISION,
        &runtime.definition.panels,
        &bindings,
        &installation_store,
        &panel_installer,
    )
    .await
    .map_err(|error| format!("panel installation failed (fail-closed, not starting): {error:?}"))?;
    eprintln!(
        "hydrated ruleset {} v{} ({} notices); panel reconcile {:?}; starting gateway",
        runtime.ruleset_key,
        runtime.version,
        runtime.notices.len(),
        install_report.outcomes
    );
    let snapshot_provider = automation_runtime::TwilightGuildRoleSnapshotProvider::new(&http)
        .await
        .map_err(|error| format!("snapshot provider startup failed: {error:?}"))?;
    let instance_ids = random_instance_id::RandomInstanceIdGenerator::new();
    let identity = RunningRuleSetIdentity {
        key: runtime.ruleset_key.as_str().to_string(),
        version: InstanceRuleSetVersion::new(runtime.version.get())
            .map_err(|error| format!("invalid runtime ruleset version: {error:?}"))?,
    };
    gateway::run(
        token,
        identity,
        runtime.definition,
        bindings,
        "스터디룸 처리에 실패했습니다.".to_string(),
        instances,
        instance_ids,
        teardown,
        &ruleset_store,
        &snapshot_provider,
    )
    .await;
    Ok(())
}

async fn readiness_context(
    http: &Client,
    guild_id: u64,
    bindings: &ResourceBindingMap,
) -> Result<
    (
        automation_ruleset_readiness::GuildCapabilities,
        BTreeMap<ResourceKey, Permissions>,
    ),
    Box<dyn std::error::Error>,
> {
    let bot = http.current_user().await?.model().await?;
    let guild_roles = http.roles(Id::new(guild_id)).await?.model().await?;
    let bot_member = http
        .guild_member(Id::new(guild_id), bot.id)
        .await?
        .model()
        .await?;
    let roles_snapshot: BTreeMap<RoleId, Permissions> = guild_roles
        .iter()
        .map(|role| {
            (
                RoleId(role.id.get()),
                Permissions::from_bits_retain(role.permissions.bits()),
            )
        })
        .collect();
    let bot_role_ids: Vec<RoleId> = bot_member.roles.iter().map(|id| RoleId(id.get())).collect();
    automation_ruleset_readiness::build_readiness_context(
        GuildId(guild_id),
        bindings,
        &roles_snapshot,
        &bot_role_ids,
    )
    .map_err(|e| format!("readiness context failed: {e:?}").into())
}

async fn seed_studyroom(
    store: &impl RuleSetStore,
    guild_id: u64,
    key: &automation_ruleset::RuleSetKey,
    variant: FixtureVariant,
    activate: bool,
    force_activate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use automation_ruleset::{PublishOutcome, PublishRuleSetRequest};
    let published = store
        .publish(PublishRuleSetRequest {
            guild_id: GuildId(guild_id),
            ruleset_key: key.clone(),
            definition: studyroom_ruleset(variant),
            created_by: UserId(0),
        })
        .await
        .map_err(|e| format!("publish failed: {e:?}"))?;
    let version = match &published {
        PublishOutcome::Created(v) | PublishOutcome::Reused(v) => v.version,
    };
    eprintln!("seed: published {version}");
    if !activate {
        return Ok(());
    }
    if let Some(current) = store
        .active(GuildId(guild_id), key)
        .await
        .map_err(|e| format!("active lookup failed: {e:?}"))?
    {
        if current.version != version && !force_activate {
            return Err(format!(
                "seed: active version {} != target {version}; pass --force-activate to replace",
                current.version
            )
            .into());
        }
    }
    let token = env::var("DISCORD_TEST_TOKEN")?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let bindings = bindings(channel_id);
    let http = Client::new(token);
    let (caps, role_permissions) = readiness_context(&http, guild_id, &bindings).await?;
    let outcome = automation_ruleset_readiness::activate_if_ready(
        store,
        GuildId(guild_id),
        key,
        version,
        &bindings,
        &caps,
        &role_permissions,
    )
    .await
    .map_err(|e| format!("seed activation failed (published, active unchanged): {e:?}"))?;
    eprintln!(
        "seed: activated {} ({} notices)",
        outcome.activation.active_version,
        outcome.runtime_ruleset.notices.len()
    );
    Ok(())
}

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn report_connect_error(error: sqlx::Error) -> String {
    match &error {
        sqlx::Error::Configuration(_) => {
            eprintln!("postgres startup: connection failed (invalid configuration)");
        }
        other => {
            eprintln!("postgres startup: connection failed: {other}");
        }
    }
    "PostgreSQL startup failed during connection".to_string()
}

fn bindings(channel_id: u64) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("study_hub".to_string()), ChannelId(channel_id));
    bindings
}

fn studyroom_ruleset(variant: FixtureVariant) -> InteractionRuleSet {
    let (create_panel_content, join_response) = match variant {
        FixtureVariant::V1 => ("스터디룸 만들기 · v1", "스터디룸에 참가했습니다. [v1]"),
        FixtureVariant::V2 => (
            "스터디룸 만들기 · v2",
            "스터디룸 참가가 완료되었습니다. [v2]",
        ),
    };
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "study_panel".to_string(),
            channel: ResourceKey("study_hub".to_string()),
            content: create_panel_content.to_string(),
            buttons: vec![ButtonSpec {
                label: "Create study room".to_string(),
                route: ButtonRoute::Static {
                    key: BUTTON_KEY.to_string(),
                },
            }],
        }],
        modals: vec![ModalSpec {
            key: MODAL_KEY.to_string(),
            title: "Create study room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
            }],
        }],
        rules: vec![
            InteractionRule {
                key: "open_study_modal".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: BUTTON_KEY.to_string(),
                },
                actions: vec![ActionSpec::OpenModal {
                    modal: MODAL_KEY.to_string(),
                }],
            },
            InteractionRule {
                key: "submit_study_modal".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: MODAL_KEY.to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::CreateRole {
                        key: "study_member_role".to_string(),
                        name: "${input.room_name} 멤버".to_string(),
                    },
                    ActionSpec::CreateChannel {
                        key: "study_channel".to_string(),
                        name: "study-${input.room_name}".to_string(),
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Everyone,
                        allow: Permissions::empty(),
                        deny: Permissions::VIEW_CHANNEL,
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Role(RoleRef::Created(created(
                            "study_member_role",
                        ))),
                        allow: Permissions::VIEW_CHANNEL,
                        deny: Permissions::empty(),
                    },
                    ActionSpec::GrantRole {
                        role: RoleRef::Created(created("study_member_role")),
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::PostPanel {
                        key: "study_welcome_panel".to_string(),
                        channel: ChannelRef::Created(created("study_channel")),
                        content:
                            "스터디룸 '${input.room_name}'이 생성되었습니다. 이 채널은 스터디 멤버만 볼 수 있어요."
                                .to_string(),
                        buttons: vec![ButtonSpec {
                            label: "도움말".to_string(),
                            route: ButtonRoute::Static {
                                key: "study_help".to_string(),
                            },
                        }, ButtonSpec {
                            label: "방 닫기".to_string(),
                            route: ButtonRoute::InstanceAction {
                                instance: InstanceRef::Created(created("study_room_instance")),
                                action: "close".to_string(),
                            },
                        }],
                    },
                    ActionSpec::PostPanel {
                        key: "study_hub_entry".to_string(),
                        channel: ChannelRef::Existing(ResourceKey("study_hub".to_string())),
                        content: "'${input.room_name}' 스터디룸이 열렸습니다.".to_string(),
                        buttons: vec![ButtonSpec {
                            label: "참가하기".to_string(),
                            route: ButtonRoute::InstanceAction {
                                instance: InstanceRef::Created(created("study_room_instance")),
                                action: "join".to_string(),
                            },
                        }],
                    },
                    ActionSpec::RegisterInstance {
                        key: "study_room_instance".to_string(),
                        kind: InstanceKind("study_room".to_string()),
                        resources: InstanceResourceRefs {
                            roles: BTreeMap::from([(
                                "member_role".to_string(),
                                created("study_member_role"),
                            )]),
                            channels: BTreeMap::from([(
                                "room_channel".to_string(),
                                created("study_channel"),
                            )]),
                            messages: BTreeMap::from([
                                (
                                    "welcome_panel".to_string(),
                                    created("study_welcome_panel"),
                                ),
                                ("hub_panel".to_string(), created("study_hub_entry")),
                            ]),
                        },
                    },
                    ActionSpec::EditResponse {
                        content: "스터디룸 '${input.room_name}' 생성 완료! 새 채널을 확인하세요."
                            .to_string(),
                    },
                ],
            },
            InteractionRule {
                key: "study_help_rule".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "study_help".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content:
                        "이 채널은 스터디 멤버만 볼 수 있는 비공개 스터디룸입니다."
                            .to_string(),
                }],
            },
            InteractionRule {
                key: "study_join_rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::GrantRole {
                        role: RoleRef::Instance {
                            instance: InstanceRef::Event,
                            alias: "member_role".to_string(),
                        },
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::EditResponse {
                        content: join_response.to_string(),
                    },
                ],
            },
            InteractionRule {
                key: "study_close_rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "close".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::TeardownInstance {
                        instance: InstanceRef::Event,
                    },
                    ActionSpec::EditResponse {
                        content: "스터디룸을 닫았습니다.".to_string(),
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_core::{analyze, validate};
    use automation_ruleset::{
        InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetStore, RuleSetVersionId,
    };
    use automation_ruleset_readiness::{policy_severity, required_capabilities};

    fn cli_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn register_resources(ruleset: &InteractionRuleSet) -> &InstanceResourceRefs {
        ruleset
            .rules
            .iter()
            .flat_map(|rule| &rule.actions)
            .find_map(|action| match action {
                ActionSpec::RegisterInstance { resources, .. } => Some(resources),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn parse_cli_accepts_position_independent_happy_paths() {
        assert_eq!(
            parse_cli(&[]).unwrap(),
            ParsedCli {
                ruleset_key: None,
                command: Command::Run,
            }
        );
        assert_eq!(
            parse_cli(&cli_args(&["run"])).unwrap(),
            ParsedCli {
                ruleset_key: None,
                command: Command::Run,
            }
        );
        assert_eq!(
            parse_cli(&cli_args(&["seed-studyroom", "--variant", "v1"])).unwrap(),
            ParsedCli {
                ruleset_key: None,
                command: Command::Seed {
                    variant: FixtureVariant::V1,
                    activate: false,
                    force_activate: false,
                },
            }
        );
        assert_eq!(
            parse_cli(&cli_args(&[
                "seed-studyroom",
                "--variant",
                "v2",
                "--activate",
                "--force-activate",
            ]))
            .unwrap(),
            ParsedCli {
                ruleset_key: None,
                command: Command::Seed {
                    variant: FixtureVariant::V2,
                    activate: true,
                    force_activate: true,
                },
            }
        );
        assert_eq!(
            parse_cli(&cli_args(&["activate", "7"])).unwrap(),
            ParsedCli {
                ruleset_key: None,
                command: Command::Activate { version: 7 },
            }
        );
        assert_eq!(
            parse_cli(&cli_args(&[
                "--ruleset-key",
                "rollback_key",
                "seed-studyroom",
                "--variant",
                "v1",
            ]))
            .unwrap()
            .ruleset_key,
            Some("rollback_key".to_string())
        );
        assert_eq!(
            parse_cli(&cli_args(&[
                "activate",
                "7",
                "--ruleset-key",
                "rollback_key",
            ]))
            .unwrap(),
            ParsedCli {
                ruleset_key: Some("rollback_key".to_string()),
                command: Command::Activate { version: 7 },
            }
        );
    }

    #[test]
    fn parse_cli_rejects_ambiguous_or_invalid_inputs() {
        let cases = vec![
            (
                cli_args(&["--ruleset-key", "one", "--ruleset-key", "two"]),
                CliError::DuplicateRulesetKey,
            ),
            (
                cli_args(&["--ruleset-key"]),
                CliError::MissingRulesetKeyValue,
            ),
            (
                cli_args(&["seed-studyroom", "--variant"]),
                CliError::MissingVariantValue,
            ),
            (
                cli_args(&["seed-studyroom", "--variant", "v1", "--variant", "v2"]),
                CliError::DuplicateVariant,
            ),
            (
                cli_args(&["seed-studyroom", "--variant", "v3"]),
                CliError::InvalidVariant("v3".to_string()),
            ),
            (
                cli_args(&["run", "--unknown"]),
                CliError::UnknownFlag("--unknown".to_string()),
            ),
            (
                cli_args(&["seed-studyroom"]),
                CliError::MissingVariantForSeed,
            ),
            (cli_args(&["activate"]), CliError::MissingActivateVersion),
            (
                cli_args(&["activate", "seven"]),
                CliError::InvalidActivateVersion("seven".to_string()),
            ),
            (
                cli_args(&["run", "--variant", "v1"]),
                CliError::VariantNotAllowed,
            ),
            (
                cli_args(&["activate", "7", "--variant", "v1"]),
                CliError::VariantNotAllowed,
            ),
            (
                cli_args(&["run", "--activate"]),
                CliError::ActivateFlagNotAllowed,
            ),
            (
                cli_args(&["activate", "7", "--force-activate"]),
                CliError::ActivateFlagNotAllowed,
            ),
            (
                cli_args(&["run", "extra"]),
                CliError::UnexpectedPositional("extra".to_string()),
            ),
            (
                cli_args(&["other"]),
                CliError::UnknownMode("other".to_string()),
            ),
        ];

        for (args, expected) in cases {
            assert_eq!(parse_cli(&args), Err(expected));
        }
    }

    #[test]
    fn resolve_ruleset_key_uses_cli_then_env_then_default() {
        assert_eq!(
            resolve_ruleset_key(Some("cli_key"), Some("env_key"))
                .unwrap()
                .as_str(),
            "cli_key"
        );
        assert_eq!(
            resolve_ruleset_key(None, Some("env_key")).unwrap().as_str(),
            "env_key"
        );
        assert_eq!(
            resolve_ruleset_key(None, None).unwrap().as_str(),
            "studyroom_demo"
        );
        assert!(matches!(
            resolve_ruleset_key(Some("bad key"), None),
            Err(CliError::InvalidRulesetKey(_))
        ));
    }

    #[tokio::test]
    async fn variant_definitions_differ_only_in_presentation() {
        let v1 = studyroom_ruleset(FixtureVariant::V1);
        let v2 = studyroom_ruleset(FixtureVariant::V2);
        let store = InMemoryRuleSetStore::default();
        let key = automation_ruleset::RuleSetKey::parse("rollback_fixture").unwrap();
        let first = store
            .publish(PublishRuleSetRequest {
                guild_id: GuildId(7),
                ruleset_key: key.clone(),
                definition: v1.clone(),
                created_by: UserId(1),
            })
            .await
            .unwrap();
        let second = store
            .publish(PublishRuleSetRequest {
                guild_id: GuildId(7),
                ruleset_key: key.clone(),
                definition: v2.clone(),
                created_by: UserId(1),
            })
            .await
            .unwrap();
        let reused = store
            .publish(PublishRuleSetRequest {
                guild_id: GuildId(7),
                ruleset_key: key,
                definition: v1.clone(),
                created_by: UserId(1),
            })
            .await
            .unwrap();
        let PublishOutcome::Created(first) = first else {
            panic!("first variant must be created")
        };
        let PublishOutcome::Created(second) = second else {
            panic!("second variant must be created")
        };
        let PublishOutcome::Reused(reused) = reused else {
            panic!("first variant must be reused")
        };

        assert_ne!(first.version, second.version);
        assert_ne!(first.content_hash, second.content_hash);
        assert_eq!(reused.version, first.version);
        assert_eq!(reused.content_hash, first.content_hash);
        validate(&v1, &bindings(1)).unwrap();
        validate(&v2, &bindings(1)).unwrap();
        assert_eq!(required_capabilities(&v1), required_capabilities(&v2));
        let v1_findings = analyze(&v1, &BTreeMap::new());
        let v2_findings = analyze(&v2, &BTreeMap::new());
        assert_eq!(v1_findings, v2_findings);
        assert_eq!(
            v1_findings.iter().map(policy_severity).collect::<Vec<_>>(),
            v2_findings.iter().map(policy_severity).collect::<Vec<_>>()
        );
        assert_eq!(register_resources(&v1), register_resources(&v2));
        assert_eq!(v1.version, 1);
        assert_eq!(v2.version, 1);
        assert_eq!(v1.panels[0].content, "스터디룸 만들기 · v1");
        assert_eq!(v2.panels[0].content, "스터디룸 만들기 · v2");

        let mut normalized = v1.clone();
        normalized.panels[0].content = v2.panels[0].content.clone();
        let join = normalized
            .rules
            .iter_mut()
            .find(|rule| {
                matches!(
                    rule.trigger,
                    TriggerSpec::InstanceAction { ref action } if action == "join"
                )
            })
            .unwrap();
        let ActionSpec::EditResponse { content } = join.actions.last_mut().unwrap() else {
            panic!("join response must be terminal")
        };
        *content = "스터디룸 참가가 완료되었습니다. [v2]".to_string();
        assert_eq!(normalized, v2);
    }

    #[test]
    fn studyroom_ruleset_validates() {
        validate(&studyroom_ruleset(FixtureVariant::V1), &bindings(1))
            .expect("studyroom ruleset should validate");
    }

    #[test]
    fn studyroom_registers_after_hub_panel_with_complete_manifest() {
        for variant in [FixtureVariant::V1, FixtureVariant::V2] {
            let ruleset = studyroom_ruleset(variant);
            let rule = ruleset
                .rules
                .iter()
                .find(|rule| rule.key == "submit_study_modal")
                .unwrap();
            let hub_index = rule
                .actions
                .iter()
                .position(|action| {
                    matches!(
                        action,
                        ActionSpec::PostPanel { key, .. } if key == "study_hub_entry"
                    )
                })
                .unwrap();
            let register_index = rule
                .actions
                .iter()
                .position(|action| matches!(action, ActionSpec::RegisterInstance { .. }))
                .unwrap();
            let ActionSpec::RegisterInstance { resources, .. } = &rule.actions[register_index]
            else {
                unreachable!()
            };

            assert!(hub_index < register_index);
            assert_eq!(
                resources.roles,
                BTreeMap::from([("member_role".to_string(), created("study_member_role"))])
            );
            assert_eq!(
                resources.channels,
                BTreeMap::from([("room_channel".to_string(), created("study_channel"))])
            );
            assert_eq!(
                resources.messages,
                BTreeMap::from([
                    ("welcome_panel".to_string(), created("study_welcome_panel"),),
                    ("hub_panel".to_string(), created("study_hub_entry")),
                ])
            );
        }
    }

    #[test]
    fn join_rule_uses_deferred_contract() {
        let ruleset = studyroom_ruleset(FixtureVariant::V1);
        let join = ruleset
            .rules
            .iter()
            .find(|rule| rule.key == "study_join_rule")
            .expect("join rule present");
        assert!(matches!(
            join.actions.first(),
            Some(ActionSpec::DeferEphemeral)
        ));
        assert!(matches!(
            join.actions.last(),
            Some(ActionSpec::EditResponse { .. })
        ));
    }

    #[test]
    fn close_rule_uses_deferred_teardown_contract() {
        let ruleset = studyroom_ruleset(FixtureVariant::V1);
        let close = ruleset
            .rules
            .iter()
            .find(|rule| rule.key == "study_close_rule")
            .unwrap();
        assert!(matches!(
            close.actions.as_slice(),
            [
                ActionSpec::DeferEphemeral,
                ActionSpec::TeardownInstance {
                    instance: InstanceRef::Event,
                },
                ActionSpec::EditResponse { .. },
            ]
        ));
    }

    #[tokio::test]
    async fn seed_without_activate_is_publish_only() {
        let store = InMemoryRuleSetStore::default();
        let key = automation_ruleset::RuleSetKey::parse(DEFAULT_RULESET_KEY).unwrap();
        seed_studyroom(&store, 7, &key, FixtureVariant::V1, false, false)
            .await
            .unwrap();
        assert!(store.active(GuildId(7), &key).await.unwrap().is_none());
        assert_eq!(
            store.list_versions(GuildId(7), &key).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn seed_activate_refuses_accidental_replacement() {
        let store = InMemoryRuleSetStore::default();
        let key = automation_ruleset::RuleSetKey::parse(DEFAULT_RULESET_KEY).unwrap();
        let mut old = studyroom_ruleset(FixtureVariant::V1);
        old.panels[0].content = "Old panel".to_string();
        store
            .publish(PublishRuleSetRequest {
                guild_id: GuildId(7),
                ruleset_key: key.clone(),
                definition: old,
                created_by: UserId(1),
            })
            .await
            .unwrap();
        store
            .activate(GuildId(7), &key, RuleSetVersionId::FIRST)
            .await
            .unwrap();

        let error = seed_studyroom(&store, 7, &key, FixtureVariant::V1, true, false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("pass --force-activate"));
        assert_eq!(
            store
                .active(GuildId(7), &key)
                .await
                .unwrap()
                .unwrap()
                .version,
            RuleSetVersionId::FIRST
        );
        assert_eq!(
            store.list_versions(GuildId(7), &key).await.unwrap().len(),
            2
        );
    }

    #[test]
    fn run_uses_ruleset_declared_panel_reconcile() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(production.contains("install_declared_panels("));
        assert!(!production.contains("async fn install_panel("));
    }

    #[test]
    fn default_ruleset_key_is_the_only_production_literal() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert_eq!(production.matches("studyroom_demo").count(), 1);
        assert!(production.contains("const DEFAULT_RULESET_KEY: &str = \"studyroom_demo\";"));
    }
}
