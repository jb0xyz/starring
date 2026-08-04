import datetime
import hashlib
import json
import re
import urllib.parse


SCHEMA_VERSION = 1
IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9:._-]{0,191}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
PROCESS_INSTANCE_PATTERN = re.compile(r"^[0-9a-f]{32}$")
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
UTC_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
FORBIDDEN_KEY_COMPONENTS = ("token", "secret", "password", "cookie")
SAFE_NON_SECRET_KEYS = {"route_controller_fencing_token"}
FORBIDDEN_KEYS = {
    "assistant_message",
    "authorization_code",
    "csrf",
    "database_url",
    "full_user_transcript",
    "idempotency_key",
    "key_material",
    "oauth_code",
    "prompt",
    "session_credential",
    "user_message",
}
FORBIDDEN_VALUE_PATTERNS = (
    re.compile(r"postgres(?:ql)?://[^\s]+", re.IGNORECASE),
    re.compile(r"(?:Bearer|Bot)\s+[A-Za-z0-9._~-]+", re.IGNORECASE),
    re.compile(r"-----BEGIN AGE ENCRYPTED FILE-----"),
    re.compile(r"\bcf(?:at|ut)_[A-Za-z0-9_-]+\b"),
)
ROUTE_IDENTITY_FIELDS = (
    "deployment_id",
    "runtime_generation",
    "route_controller_fencing_token",
    "route_incarnation",
    "origin_process_instance_id",
    "origin_serving_lease_epoch",
    "origin_serving_revision",
    "origin_gateway_shard_id",
    "origin_gateway_owner_lease_epoch",
    "origin_gateway_owner_revision",
)
SERVING_IDENTITY_FIELDS = (
    "guild_id",
    "ruleset_key",
    "tenant_id",
    "installation_id",
    "deployment_id",
    "attestation_id",
    "process_instance_id",
    "runtime_generation",
    "target_version",
    "target_content_hash",
    "binding_revision",
    "binding_fingerprint",
    "lease_epoch",
    "revision",
)
EFFECT_IDENTITY_FIELDS = (
    "application_id",
    "interaction_id",
    "action_index",
)


class EvidenceContractError(ValueError):
    pass


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _strict_json_object(pairs):
    value = {}
    for key, nested in pairs:
        if key in value:
            _fail("evidence_duplicate_key")
        value[key] = nested
    return value


def load_strict_json(raw):
    if isinstance(raw, str):
        encoded = raw.encode("utf-8")
    elif isinstance(raw, bytes):
        encoded = raw
    else:
        _fail("evidence_json_type_invalid")
    if not encoded or len(encoded) > 256 * 1024:
        _fail("evidence_json_size_invalid")
    try:
        value = json.loads(encoded, object_pairs_hook=_strict_json_object)
    except EvidenceContractError:
        raise
    except (UnicodeDecodeError, json.JSONDecodeError):
        _fail("evidence_json_invalid")
    _validate_safe(value)
    return value


def _fail(code):
    raise EvidenceContractError(code)


def _require_exact_object(value, fields, code):
    if not isinstance(value, dict) or set(value) != set(fields):
        _fail(code)
    return value


def _require_identifier(value, code):
    if not isinstance(value, str) or not IDENTIFIER_PATTERN.fullmatch(value):
        _fail(code)
    return value


def _require_digest(value, code):
    if not isinstance(value, str) or not DIGEST_PATTERN.fullmatch(value):
        _fail(code)
    return value


def _require_process_instance(value, code):
    if not isinstance(value, str) or not PROCESS_INSTANCE_PATTERN.fullmatch(value):
        _fail(code)
    return value


def _require_snowflake(value, code):
    if (
        not isinstance(value, str)
        or not SNOWFLAKE_PATTERN.fullmatch(value)
        or int(value) > 18446744073709551615
    ):
        _fail(code)
    return value


def _require_positive_integer(value, code):
    if type(value) is not int or value <= 0 or value > 9223372036854775807:
        _fail(code)
    return value


def _require_nonnegative_integer(value, code, maximum=9223372036854775807):
    if type(value) is not int or value < 0 or value > maximum:
        _fail(code)
    return value


def _require_boolean(value, code):
    if type(value) is not bool:
        _fail(code)
    return value


def _require_timestamp(value, code):
    if not isinstance(value, str) or not UTC_PATTERN.fullmatch(value):
        _fail(code)
    normalized = value[:-1] + "+00:00"
    try:
        parsed = datetime.datetime.fromisoformat(normalized)
    except ValueError:
        _fail(code)
    if parsed.tzinfo != datetime.timezone.utc:
        _fail(code)
    return value


def _require_public_origin(value, code):
    if not isinstance(value, str) or len(value) > 2048:
        _fail(code)
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.path not in ("", "/")
    ):
        _fail(code)
    canonical = f"https://{parsed.hostname}"
    try:
        port = parsed.port
    except ValueError:
        _fail(code)
    if port is not None:
        canonical += f":{port}"
    if value.rstrip("/") != canonical:
        _fail(code)
    return canonical


def _validate_safe(value, path=(), depth=0):
    if depth > 8:
        _fail("evidence_nesting_too_deep")
    if isinstance(value, dict):
        if len(value) > 256:
            _fail("evidence_collection_too_large")
        for key, nested in value.items():
            if not isinstance(key, str) or not key:
                _fail("evidence_key_invalid")
            lowered = key.lower()
            if lowered in FORBIDDEN_KEYS or (
                lowered not in SAFE_NON_SECRET_KEYS
                and any(component in lowered for component in FORBIDDEN_KEY_COMPONENTS)
            ):
                _fail(f"evidence_forbidden_key:{'.'.join((*path, key))}")
            _validate_safe(nested, (*path, key), depth + 1)
        return
    if isinstance(value, list):
        if len(value) > 256:
            _fail("evidence_collection_too_large")
        for index, nested in enumerate(value):
            _validate_safe(nested, (*path, str(index)), depth + 1)
        return
    if isinstance(value, str):
        if len(value.encode("utf-8")) > 4096:
            _fail("evidence_string_too_large")
        if any(pattern.search(value) for pattern in FORBIDDEN_VALUE_PATTERNS):
            _fail("evidence_forbidden_value")
        return
    if value is None or isinstance(value, (bool, int)):
        return
    _fail("evidence_value_type_invalid")


def _identity_digest(value, fields, validator, kind):
    identity = _require_exact_object(value, fields, "identity_fields_invalid")
    validator(identity)
    payload = {"schema_version": SCHEMA_VERSION, "kind": kind, "identity": identity}
    return hashlib.sha256(canonical_json(payload).encode("utf-8")).hexdigest()


def _validate_route_identity(identity):
    _require_identifier(identity["deployment_id"], "route_deployment_id_invalid")
    _require_positive_integer(identity["runtime_generation"], "route_generation_invalid")
    _require_positive_integer(
        identity["route_controller_fencing_token"], "route_fencing_token_invalid"
    )
    _require_positive_integer(identity["route_incarnation"], "route_incarnation_invalid")
    _require_process_instance(
        identity["origin_process_instance_id"], "route_process_instance_invalid"
    )
    _require_positive_integer(
        identity["origin_serving_lease_epoch"], "route_serving_epoch_invalid"
    )
    _require_positive_integer(
        identity["origin_serving_revision"], "route_serving_revision_invalid"
    )
    _require_identifier(identity["origin_gateway_shard_id"], "route_gateway_shard_invalid")
    _require_positive_integer(
        identity["origin_gateway_owner_lease_epoch"], "route_gateway_epoch_invalid"
    )
    _require_positive_integer(
        identity["origin_gateway_owner_revision"], "route_gateway_revision_invalid"
    )


def canonical_route_identity_sha256(identity):
    return _identity_digest(
        identity,
        ROUTE_IDENTITY_FIELDS,
        _validate_route_identity,
        "starring.d2.route-identity.v1",
    )


def _validate_serving_identity(identity):
    _require_snowflake(identity["guild_id"], "serving_guild_id_invalid")
    for field in (
        "ruleset_key",
        "tenant_id",
        "installation_id",
        "deployment_id",
    ):
        _require_identifier(identity[field], f"serving_{field}_invalid")
    _require_digest(identity["attestation_id"], "serving_attestation_id_invalid")
    _require_process_instance(
        identity["process_instance_id"], "serving_process_instance_invalid"
    )
    for field in (
        "runtime_generation",
        "target_version",
        "binding_revision",
        "lease_epoch",
        "revision",
    ):
        _require_positive_integer(identity[field], f"serving_{field}_invalid")
    _require_digest(identity["target_content_hash"], "serving_target_content_hash_invalid")
    _require_digest(identity["binding_fingerprint"], "serving_binding_fingerprint_invalid")


def canonical_serving_identity_sha256(identity):
    return _identity_digest(
        identity,
        SERVING_IDENTITY_FIELDS,
        _validate_serving_identity,
        "starring.d2.serving-identity.v1",
    )


def _validate_effect_identity(identity):
    _require_snowflake(identity["application_id"], "effect_application_id_invalid")
    _require_snowflake(identity["interaction_id"], "effect_interaction_id_invalid")
    _require_nonnegative_integer(identity["action_index"], "effect_action_index_invalid", 255)


def canonical_effect_identity_sha256(identity):
    return _identity_digest(
        identity,
        EFFECT_IDENTITY_FIELDS,
        _validate_effect_identity,
        "starring.d2.effect-identity.v1",
    )


def validate_canonical_identity_sha256(value, code):
    return _require_digest(value, code)


def _require_envelope(value, kind, fields):
    expected = {"schema_version", "kind", "observed_at", *fields}
    envelope = _require_exact_object(value, expected, f"{kind}:fields_invalid")
    _validate_safe(envelope)
    if envelope["schema_version"] != SCHEMA_VERSION or envelope["kind"] != kind:
        _fail(f"{kind}:identity_invalid")
    _require_timestamp(envelope["observed_at"], f"{kind}:observed_at_invalid")
    return envelope


def _same(left, right, code):
    if left != right:
        _fail(code)
    return left


def _require_http_status(value, code, accepted=None):
    if type(value) is not int or not 100 <= value <= 599:
        _fail(code)
    if accepted is not None and value not in accepted:
        _fail(code)
    return value


def _require_snowflake_list(value, code):
    if not isinstance(value, list) or not value or len(value) > 128:
        _fail(code)
    if len(set(value)) != len(value):
        _fail(code)
    for item in value:
        _require_snowflake(item, code)
    return value


def _require_state(value, accepted, code):
    if value not in accepted:
        _fail(code)
    return value


def assemble_authentication_evidence(browser):
    fields = {
        "public_origin",
        "me_status",
        "principal_id",
        "installation_id",
        "guild_id",
        "authority_check_status",
    }
    value = _require_envelope(
        browser, "starring.d2.browser-authentication-evidence.v1", fields
    )
    _require_public_origin(value["public_origin"], "browser_public_origin_invalid")
    _require_http_status(value["me_status"], "browser_me_status_invalid", {200})
    _require_identifier(value["principal_id"], "browser_principal_id_invalid")
    _require_identifier(value["installation_id"], "browser_installation_id_invalid")
    _require_snowflake(value["guild_id"], "browser_guild_id_invalid")
    _require_http_status(
        value["authority_check_status"], "browser_authority_status_invalid", {204}
    )
    return {
        "me_status": value["me_status"],
        "principal_id": value["principal_id"],
        "installation_id": value["installation_id"],
        "guild_id": value["guild_id"],
        "authority_check_status": value["authority_check_status"],
        "public_origin": value["public_origin"],
    }


def assemble_authoring_evidence(browser):
    fields = {
        "public_origin",
        "authoring_http_status",
        "authoring_session_id",
        "authoring_generation",
        "installation_id",
        "model",
        "provider",
        "reasoning_effort",
        "auth_mode",
        "one_shot",
    }
    value = _require_envelope(
        browser, "starring.d2.browser-authoring-evidence.v1", fields
    )
    _require_public_origin(value["public_origin"], "browser_public_origin_invalid")
    _require_http_status(
        value["authoring_http_status"], "authoring_http_status_invalid", {200, 201}
    )
    _require_identifier(value["authoring_session_id"], "authoring_session_id_invalid")
    _require_positive_integer(value["authoring_generation"], "authoring_generation_invalid")
    for field in ("installation_id", "model", "provider", "reasoning_effort", "auth_mode"):
        _require_identifier(value[field], f"authoring_{field}_invalid")
    if value["one_shot"] is not True:
        _fail("authoring_one_shot_invalid")
    return {field: value[field] for field in fields}


def assemble_preview_evidence(database):
    fields = {
        "generation_encrypted",
        "projection_state",
        "generation",
        "payload_digest",
        "installation_id",
        "authoring_session_id",
    }
    value = _require_envelope(database, "starring.d2.db-authoring-evidence.v1", fields)
    if value["generation_encrypted"] is not True:
        _fail("db_generation_encrypted_invalid")
    _require_state(value["projection_state"], {"preview_ready"}, "db_projection_state_invalid")
    _require_positive_integer(value["generation"], "db_generation_invalid")
    _require_digest(value["payload_digest"], "db_payload_digest_invalid")
    _require_identifier(value["installation_id"], "db_installation_id_invalid")
    _require_identifier(value["authoring_session_id"], "db_authoring_session_id_invalid")
    return {field: value[field] for field in fields}


def assemble_decision_evidence(browser):
    fields = {
        "public_origin",
        "installation_id",
        "promotion_id",
        "preview_state",
        "approval_state",
        "apply_state",
    }
    value = _require_envelope(
        browser, "starring.d2.browser-product-decision-evidence.v1", fields
    )
    _require_public_origin(value["public_origin"], "browser_public_origin_invalid")
    _require_identifier(value["installation_id"], "decision_installation_id_invalid")
    _require_digest(value["promotion_id"], "decision_promotion_id_invalid")
    _require_state(value["preview_state"], {"pending_approval"}, "preview_state_invalid")
    _require_state(value["approval_state"], {"approved"}, "approval_state_invalid")
    _require_state(value["apply_state"], {"runtime_pending"}, "apply_state_invalid")
    return {field: value[field] for field in fields}


def assemble_live_evidence(browser, database):
    browser_fields = {
        "public_origin",
        "installation_id",
        "promotion_id",
        "pending_observed",
        "live_observed",
        "attempts",
        "product_state",
        "operational_state",
        "runtime_phase",
        "serving_state",
        "deployment_http_status",
        "operational_http_status",
    }
    database_fields = {
        "installation_id",
        "promotion_id",
        "deployment_id",
        "attestation_id",
        "route_identity",
        "serving_identity",
    }
    public = _require_envelope(
        browser, "starring.d2.browser-live-evidence.v1", browser_fields
    )
    durable = _require_envelope(database, "starring.d2.db-live-evidence.v1", database_fields)
    _require_public_origin(public["public_origin"], "browser_public_origin_invalid")
    for field in ("installation_id", "promotion_id"):
        _same(public[field], durable[field], f"live_{field}_mismatch")
    _require_identifier(public["installation_id"], "live_installation_id_invalid")
    _require_digest(public["promotion_id"], "live_promotion_id_invalid")
    if public["pending_observed"] is not True or public["live_observed"] is not True:
        _fail("live_transition_invalid")
    _require_positive_integer(public["attempts"], "live_attempts_invalid")
    for field in ("product_state", "operational_state", "runtime_phase"):
        _require_state(public[field], {"live"}, f"live_{field}_invalid")
    _require_state(public["serving_state"], {"fresh"}, "live_serving_state_invalid")
    _require_http_status(public["deployment_http_status"], "deployment_http_status_invalid", {200})
    _require_http_status(public["operational_http_status"], "operational_http_status_invalid", {200})
    _require_identifier(durable["deployment_id"], "live_deployment_id_invalid")
    _require_digest(durable["attestation_id"], "live_attestation_id_invalid")
    route_id = canonical_route_identity_sha256(durable["route_identity"])
    serving_id = canonical_serving_identity_sha256(durable["serving_identity"])
    if durable["route_identity"]["deployment_id"] != durable["deployment_id"]:
        _fail("live_route_deployment_mismatch")
    if durable["serving_identity"]["deployment_id"] != durable["deployment_id"]:
        _fail("live_serving_deployment_mismatch")
    if durable["serving_identity"]["attestation_id"] != durable["attestation_id"]:
        _fail("live_serving_attestation_mismatch")
    if durable["serving_identity"]["installation_id"] != public["installation_id"]:
        _fail("live_serving_installation_mismatch")
    route_identity = durable["route_identity"]
    serving_identity = durable["serving_identity"]
    if (
        route_identity["origin_process_instance_id"]
        != serving_identity["process_instance_id"]
        or route_identity["runtime_generation"]
        != serving_identity["runtime_generation"]
        or route_identity["origin_serving_lease_epoch"]
        != serving_identity["lease_epoch"]
        or route_identity["origin_serving_revision"] != serving_identity["revision"]
    ):
        _fail("live_route_serving_identity_mismatch")
    return {
        "pending_observed": True,
        "live_observed": True,
        "installation_id": public["installation_id"],
        "promotion_id": public["promotion_id"],
        "deployment_id": durable["deployment_id"],
        "route_id": route_id,
        "attestation_id": durable["attestation_id"],
        "serving_lease_id": serving_id,
        "public_origin": public["public_origin"],
    }


def assemble_interaction_evidence(database, transport):
    database_fields = {
        "create_interaction_id",
        "join_interaction_id",
        "deployment_id",
        "route_identity",
        "instance_id",
        "role_ids",
        "channel_ids",
        "panel_message_ids",
        "ephemeral_count",
    }
    transport_fields = {
        "role_ids",
        "channel_ids",
        "panel_message_ids",
        "transport_instance_id",
    }
    durable = _require_envelope(
        database, "starring.d2.db-interaction-evidence.v1", database_fields
    )
    inventory = _require_envelope(
        transport, "starring.d2.transport-resource-evidence.v1", transport_fields
    )
    for field in ("create_interaction_id", "join_interaction_id"):
        _require_snowflake(durable[field], f"interaction_{field}_invalid")
    if durable["create_interaction_id"] == durable["join_interaction_id"]:
        _fail("interaction_ids_not_distinct")
    _require_identifier(durable["deployment_id"], "interaction_deployment_id_invalid")
    _require_identifier(durable["instance_id"], "interaction_instance_id_invalid")
    route_id = canonical_route_identity_sha256(durable["route_identity"])
    if durable["route_identity"]["deployment_id"] != durable["deployment_id"]:
        _fail("interaction_route_deployment_mismatch")
    for field in ("role_ids", "channel_ids", "panel_message_ids"):
        durable_ids = _require_snowflake_list(durable[field], f"interaction_{field}_invalid")
        inventory_ids = _require_snowflake_list(inventory[field], f"transport_{field}_invalid")
        if set(durable_ids) != set(inventory_ids):
            _fail(f"interaction_{field}_mismatch")
    _require_positive_integer(durable["ephemeral_count"], "interaction_ephemeral_count_invalid")
    _require_identifier(inventory["transport_instance_id"], "transport_instance_id_invalid")
    return {
        "create_interaction_id": durable["create_interaction_id"],
        "join_interaction_id": durable["join_interaction_id"],
        "deployment_id": durable["deployment_id"],
        "route_id": route_id,
        "instance_id": durable["instance_id"],
        "role_ids": durable["role_ids"],
        "channel_ids": durable["channel_ids"],
        "panel_message_ids": durable["panel_message_ids"],
        "ephemeral_count": durable["ephemeral_count"],
        "transport_instance_id": inventory["transport_instance_id"],
    }


def assemble_duplicate_evidence(database, transport):
    database_fields = {
        "interaction_id",
        "effect_identity",
        "external_effect_count",
        "receipt_state",
    }
    transport_fields = {
        "interaction_id",
        "delivery_count",
        "transport_duplicate_injections",
        "transport_duplicate_delivery_count",
        "transport_last_duplicate_interaction_id",
        "transport_instance_id",
    }
    durable = _require_envelope(
        database, "starring.d2.db-duplicate-evidence.v1", database_fields
    )
    injected = _require_envelope(
        transport, "starring.d2.transport-duplicate-evidence.v1", transport_fields
    )
    interaction_id = _same(
        durable["interaction_id"], injected["interaction_id"], "duplicate_interaction_mismatch"
    )
    _require_snowflake(interaction_id, "duplicate_interaction_id_invalid")
    _same(
        interaction_id,
        injected["transport_last_duplicate_interaction_id"],
        "duplicate_transport_interaction_mismatch",
    )
    if durable["effect_identity"]["interaction_id"] != interaction_id:
        _fail("duplicate_effect_interaction_mismatch")
    effect_id = canonical_effect_identity_sha256(durable["effect_identity"])
    if durable["external_effect_count"] != 1 or durable["receipt_state"] != "completed":
        _fail("duplicate_durable_outcome_invalid")
    if (
        injected["delivery_count"] < 2
        or injected["transport_duplicate_injections"] != 1
        or injected["transport_duplicate_delivery_count"] != 2
    ):
        _fail("duplicate_transport_outcome_invalid")
    _require_identifier(injected["transport_instance_id"], "transport_instance_id_invalid")
    return {
        "interaction_id": interaction_id,
        "effect_id": effect_id,
        "delivery_count": injected["delivery_count"],
        "external_effect_count": durable["external_effect_count"],
        "receipt_state": durable["receipt_state"],
        "transport_duplicate_injections": injected["transport_duplicate_injections"],
        "transport_duplicate_delivery_count": injected["transport_duplicate_delivery_count"],
        "transport_last_duplicate_interaction_id": injected[
            "transport_last_duplicate_interaction_id"
        ],
        "transport_instance_id": injected["transport_instance_id"],
    }


def assemble_reconstruction_evidence(database):
    fields = {
        "route_reconstructed",
        "instance_reconstructed",
        "deployment_id",
        "source_route_identity",
        "reconstructed_route_identity",
        "source_serving_identity",
        "reconstructed_serving_identity",
        "instance_id",
        "pinned_ruleset_digest",
        "probe_interaction_id",
        "process_instance_id",
    }
    value = _require_envelope(
        database, "starring.d2.db-reconstruction-evidence.v1", fields
    )
    if value["route_reconstructed"] is not True or value["instance_reconstructed"] is not True:
        _fail("reconstruction_state_invalid")
    _require_identifier(value["deployment_id"], "reconstruction_deployment_id_invalid")
    _require_identifier(value["instance_id"], "reconstruction_instance_id_invalid")
    _require_digest(value["pinned_ruleset_digest"], "pinned_ruleset_digest_invalid")
    _require_snowflake(value["probe_interaction_id"], "probe_interaction_id_invalid")
    _require_process_instance(value["process_instance_id"], "reconstruction_process_invalid")
    source_route_id = canonical_route_identity_sha256(value["source_route_identity"])
    reconstructed_route_id = canonical_route_identity_sha256(
        value["reconstructed_route_identity"]
    )
    source_serving_id = canonical_serving_identity_sha256(
        value["source_serving_identity"]
    )
    reconstructed_serving_id = canonical_serving_identity_sha256(
        value["reconstructed_serving_identity"]
    )
    for field in (
        "source_route_identity",
        "reconstructed_route_identity",
        "source_serving_identity",
        "reconstructed_serving_identity",
    ):
        if value[field]["deployment_id"] != value["deployment_id"]:
            _fail("reconstruction_route_deployment_mismatch")
    if (
        value["reconstructed_route_identity"]["origin_process_instance_id"]
        != value["process_instance_id"]
        or value["reconstructed_serving_identity"]["process_instance_id"]
        != value["process_instance_id"]
    ):
        _fail("reconstruction_route_process_mismatch")
    if (
        source_route_id == reconstructed_route_id
        or source_serving_id == reconstructed_serving_id
    ):
        _fail("reconstruction_identity_not_rotated")
    return {
        "route_reconstructed": True,
        "instance_reconstructed": True,
        "deployment_id": value["deployment_id"],
        "source_route_id": source_route_id,
        "reconstructed_route_id": reconstructed_route_id,
        "source_serving_lease_id": source_serving_id,
        "reconstructed_serving_lease_id": reconstructed_serving_id,
        "instance_id": value["instance_id"],
        "pinned_ruleset_digest": value["pinned_ruleset_digest"],
        "probe_interaction_id": value["probe_interaction_id"],
        "process_instance_id": value["process_instance_id"],
    }


def assemble_reconciliation_evidence(database, transport):
    database_fields = {
        "effect_identity",
        "interaction_id",
        "route_identity",
        "reconciliation_state",
        "duplicate_external_effect_count",
        "unsafe_deletion_count",
    }
    transport_fields = {
        "interaction_id",
        "injected_outcome",
        "transport_indeterminate_injections",
        "transport_last_audit_reason_sha256",
        "transport_last_upstream_status",
        "transport_instance_id",
    }
    durable = _require_envelope(
        database, "starring.d2.db-reconciliation-evidence.v1", database_fields
    )
    injected = _require_envelope(
        transport, "starring.d2.transport-indeterminate-evidence.v1", transport_fields
    )
    interaction_id = _same(
        durable["interaction_id"], injected["interaction_id"], "reconciliation_interaction_mismatch"
    )
    _require_snowflake(interaction_id, "reconciliation_interaction_id_invalid")
    if durable["effect_identity"]["interaction_id"] != interaction_id:
        _fail("reconciliation_effect_interaction_mismatch")
    effect_id = canonical_effect_identity_sha256(durable["effect_identity"])
    route_id = canonical_route_identity_sha256(durable["route_identity"])
    _require_state(
        durable["reconciliation_state"],
        {"known_success", "known_failure", "compensated"},
        "reconciliation_state_invalid",
    )
    if durable["duplicate_external_effect_count"] != 0 or durable["unsafe_deletion_count"] != 0:
        _fail("reconciliation_safety_invalid")
    if injected["injected_outcome"] != "indeterminate":
        _fail("indeterminate_outcome_invalid")
    if injected["transport_indeterminate_injections"] != 1:
        _fail("indeterminate_injection_count_invalid")
    _require_digest(
        injected["transport_last_audit_reason_sha256"], "indeterminate_audit_digest_invalid"
    )
    _require_http_status(
        injected["transport_last_upstream_status"],
        "indeterminate_upstream_status_invalid",
        set(range(200, 300)),
    )
    _require_identifier(injected["transport_instance_id"], "transport_instance_id_invalid")
    return {
        "effect_id": effect_id,
        "interaction_id": interaction_id,
        "route_id": route_id,
        "injected_outcome": injected["injected_outcome"],
        "reconciliation_state": durable["reconciliation_state"],
        "duplicate_external_effect_count": durable["duplicate_external_effect_count"],
        "unsafe_deletion_count": durable["unsafe_deletion_count"],
        "transport_indeterminate_injections": injected[
            "transport_indeterminate_injections"
        ],
        "transport_last_audit_reason_sha256": injected[
            "transport_last_audit_reason_sha256"
        ],
        "transport_last_upstream_status": injected["transport_last_upstream_status"],
        "transport_instance_id": injected["transport_instance_id"],
    }


def assemble_replacement_evidence(browser, database):
    browser_fields = {
        "public_origin",
        "installation_id",
        "source_promotion_id",
        "replacement_promotion_id",
        "replacement_kind",
        "preview_state",
        "approval_state",
        "apply_state",
        "pending_observed",
        "live_observed",
        "product_state",
        "operational_state",
        "runtime_phase",
        "serving_state",
        "drain_conflict_observed",
        "drain_attempts",
    }
    database_fields = {
        "installation_id",
        "source_promotion_id",
        "replacement_promotion_id",
        "source_deployment_id",
        "source_route_identity",
        "replacement_deployment_id",
        "replacement_route_identity",
        "previous_target_drained",
        "replacement_live",
        "prior_route_absent",
    }
    public = _require_envelope(
        browser, "starring.d2.browser-replacement-evidence.v1", browser_fields
    )
    durable = _require_envelope(
        database, "starring.d2.db-replacement-evidence.v1", database_fields
    )
    _require_public_origin(public["public_origin"], "browser_public_origin_invalid")
    for field in ("installation_id", "source_promotion_id", "replacement_promotion_id"):
        _same(public[field], durable[field], f"replacement_{field}_mismatch")
    _require_identifier(public["installation_id"], "replacement_installation_id_invalid")
    _require_digest(public["source_promotion_id"], "source_promotion_id_invalid")
    _require_digest(public["replacement_promotion_id"], "replacement_promotion_id_invalid")
    _require_state(public["replacement_kind"], {"update", "rollback"}, "replacement_kind_invalid")
    _require_state(public["preview_state"], {"pending_approval"}, "replacement_preview_invalid")
    _require_state(public["approval_state"], {"approved"}, "replacement_approval_invalid")
    _require_state(public["apply_state"], {"runtime_pending"}, "replacement_apply_invalid")
    for field in ("pending_observed", "live_observed"):
        if public[field] is not True:
            _fail(f"replacement_{field}_invalid")
    for field in ("product_state", "operational_state", "runtime_phase"):
        _require_state(public[field], {"live"}, f"replacement_{field}_invalid")
    _require_state(public["serving_state"], {"fresh"}, "replacement_serving_state_invalid")
    _require_boolean(public["drain_conflict_observed"], "drain_conflict_observed_invalid")
    _require_nonnegative_integer(public["drain_attempts"], "drain_attempts_invalid", 32)
    for field in (
        "source_deployment_id",
        "replacement_deployment_id",
    ):
        _require_identifier(durable[field], f"replacement_{field}_invalid")
    if durable["source_deployment_id"] == durable["replacement_deployment_id"]:
        _fail("replacement_deployment_not_rotated")
    for field in ("previous_target_drained", "replacement_live", "prior_route_absent"):
        if durable[field] is not True:
            _fail(f"replacement_{field}_invalid")
    source_route_id = canonical_route_identity_sha256(durable["source_route_identity"])
    replacement_route_id = canonical_route_identity_sha256(
        durable["replacement_route_identity"]
    )
    if durable["source_route_identity"]["deployment_id"] != durable["source_deployment_id"]:
        _fail("source_route_deployment_mismatch")
    if (
        durable["replacement_route_identity"]["deployment_id"]
        != durable["replacement_deployment_id"]
    ):
        _fail("replacement_route_deployment_mismatch")
    if source_route_id == replacement_route_id:
        _fail("replacement_route_not_rotated")
    return {
        "replacement_target_id": durable["replacement_deployment_id"],
        "replacement_kind": public["replacement_kind"],
        "source_deployment_id": durable["source_deployment_id"],
        "source_route_id": source_route_id,
        "replacement_deployment_id": durable["replacement_deployment_id"],
        "replacement_route_id": replacement_route_id,
        "previous_target_drained": True,
        "replacement_live": True,
        "prior_route_absent": True,
        "public_origin": public["public_origin"],
    }


def assemble_live_loss_evidence(browser, transport):
    browser_fields = {
        "public_origin",
        "installation_id",
        "promotion_id",
        "live_lost",
        "deployment_http_status",
        "operational_http_status",
        "product_state",
        "operational_state",
        "runtime_phase",
        "serving_state",
        "public_code",
        "retryable",
    }
    transport_fields = {
        "gateway_disconnected",
        "runtime_ready_status",
        "route_identity",
        "transport_gateway_partitioned",
        "transport_gateway_partition_events",
        "transport_instance_id",
    }
    public = _require_envelope(
        browser, "starring.d2.browser-live-loss-evidence.v1", browser_fields
    )
    injected = _require_envelope(
        transport, "starring.d2.transport-gateway-loss-evidence.v1", transport_fields
    )
    _require_public_origin(public["public_origin"], "browser_public_origin_invalid")
    _require_identifier(public["installation_id"], "live_loss_installation_id_invalid")
    _require_digest(public["promotion_id"], "live_loss_promotion_id_invalid")
    if public["live_lost"] is not True or injected["gateway_disconnected"] is not True:
        _fail("live_loss_not_observed")
    _require_http_status(public["deployment_http_status"], "live_loss_deployment_status_invalid")
    _require_http_status(public["operational_http_status"], "live_loss_operational_status_invalid")
    _require_identifier(public["product_state"], "live_loss_product_state_invalid")
    _require_identifier(public["operational_state"], "live_loss_operational_state_invalid")
    _require_identifier(public["runtime_phase"], "live_loss_runtime_phase_invalid")
    _require_identifier(public["serving_state"], "live_loss_serving_state_invalid")
    _require_identifier(public["public_code"], "live_loss_public_code_invalid")
    _require_boolean(public["retryable"], "live_loss_retryable_invalid")
    if injected["runtime_ready_status"] != 503:
        _fail("live_loss_runtime_ready_status_invalid")
    if injected["transport_gateway_partitioned"] is not True:
        _fail("live_loss_transport_partition_invalid")
    _require_positive_integer(
        injected["transport_gateway_partition_events"], "live_loss_partition_events_invalid"
    )
    _require_identifier(injected["transport_instance_id"], "transport_instance_id_invalid")
    route_id = canonical_route_identity_sha256(injected["route_identity"])
    return {
        "gateway_disconnected": True,
        "live_lost": True,
        "runtime_ready_status": injected["runtime_ready_status"],
        "public_code": public["public_code"],
        "route_id": route_id,
        "transport_gateway_partitioned": True,
        "transport_gateway_partition_events": injected[
            "transport_gateway_partition_events"
        ],
        "transport_instance_id": injected["transport_instance_id"],
        "public_origin": public["public_origin"],
    }
