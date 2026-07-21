SET LOCAL search_path = pg_catalog, public;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE public.ruleset_panel_installations
VALIDATE CONSTRAINT ruleset_panel_installations_guild_id_bounded,
VALIDATE CONSTRAINT ruleset_panel_installations_ruleset_key_bounded,
VALIDATE CONSTRAINT ruleset_panel_installations_panel_key_bounded,
VALIDATE CONSTRAINT ruleset_panel_installations_channel_id_bounded,
VALIDATE CONSTRAINT ruleset_panel_installations_message_id_bounded;
