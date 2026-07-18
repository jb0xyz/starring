use std::fmt::Display;

use product_control_http::{
    ApplyCommand, CsrfSecret, CurrentPrincipalView, DecisionCommand, DiscordAuthorizationRequest,
    IdempotencyKey, OAuthCallbackCommand, OAuthCallbackResult, OAuthCode, OAuthStartCommand,
    OAuthStartResult, OAuthState, PromoteCommand, RejectCommand, SessionCredential,
};

const SESSION: &str = "sssssssssssssssssssssssssssssssssssssssssss";
const CSRF: &str = "ccccccccccccccccccccccccccccccccccccccccccc";
const STATE: &str = "ttttttttttttttttttttttttttttttttttttttttttg";
const NONCE: &str = "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnng";
const CODE: &str = "oauth-authorization-code-secret";

trait AmbiguousIfClone<A> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}
impl<T: Clone> AmbiguousIfClone<u8> for T {}

trait AmbiguousIfDisplay<A> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfDisplay<()> for T {}
impl<T: ?Sized + Display> AmbiguousIfDisplay<u8> for T {}

macro_rules! assert_not_clone {
    ($value:ty) => {
        let _ = <$value as AmbiguousIfClone<_>>::marker;
    };
}

macro_rules! assert_not_display {
    ($value:ty) => {
        let _ = <$value as AmbiguousIfDisplay<_>>::marker;
    };
}

#[test]
fn public_transport_contract_intentionally_omits_clone_for_secret_bearing_types() {
    assert_not_clone!(SessionCredential);
    assert_not_clone!(CsrfSecret);
    assert_not_clone!(OAuthState);
    assert_not_clone!(OAuthCode);
    assert_not_clone!(IdempotencyKey);
    assert_not_clone!(OAuthStartResult);
    assert_not_clone!(OAuthCallbackCommand);
    assert_not_clone!(OAuthCallbackResult);
    assert_not_clone!(PromoteCommand);
    assert_not_clone!(DecisionCommand);
    assert_not_clone!(RejectCommand);
    assert_not_clone!(ApplyCommand);
    assert_not_clone!(CurrentPrincipalView);
}

#[test]
fn transport_secrets_do_not_offer_display_surfaces() {
    assert_not_display!(SessionCredential);
    assert_not_display!(CsrfSecret);
    assert_not_display!(OAuthState);
    assert_not_display!(OAuthCode);
    assert_not_display!(IdempotencyKey);
}

#[test]
fn non_secret_oauth_start_command_remains_a_cloneable_value_contract() {
    let command = OAuthStartCommand {
        return_to: Some("/app".to_string()),
    };
    assert_eq!(command.clone(), command);
}

#[test]
fn discord_authorization_request_contract_contains_only_client_and_callback() {
    let request = DiscordAuthorizationRequest {
        client_id: "123456789012345678".to_string(),
        callback_url: "https://starring.example/oauth/discord/callback".to_string(),
    };
    let DiscordAuthorizationRequest {
        client_id,
        callback_url,
    } = request;
    assert_eq!(client_id, "123456789012345678");
    assert_eq!(
        callback_url,
        "https://starring.example/oauth/discord/callback"
    );
}

#[test]
fn secret_debug_and_composite_debug_are_redacted() {
    let session = SessionCredential::parse(SESSION).unwrap();
    let csrf = CsrfSecret::parse(CSRF).unwrap();
    let state = OAuthState::parse(STATE).unwrap();
    let nonce = OAuthState::parse(NONCE).unwrap();
    let code = OAuthCode::parse(CODE).unwrap();
    let idempotency_key = IdempotencyKey::parse("private-request-1").unwrap();
    for (rendered, secret) in [
        (format!("{session:?}"), SESSION),
        (format!("{csrf:?}"), CSRF),
        (format!("{state:?}"), STATE),
        (format!("{nonce:?}"), NONCE),
        (format!("{code:?}"), CODE),
        (format!("{idempotency_key:?}"), "private-request-1"),
    ] {
        assert!(!rendered.contains(secret));
        assert!(rendered.contains("<redacted>"));
    }

    let callback = OAuthCallbackCommand {
        code: OAuthCode::parse(CODE).unwrap(),
        state: OAuthState::parse(STATE).unwrap(),
        browser_nonce: OAuthState::parse(NONCE).unwrap(),
    };
    let callback_debug = format!("{callback:?}");
    for secret in [CODE, STATE, NONCE] {
        assert!(!callback_debug.contains(secret));
    }

    let callback_result = OAuthCallbackResult {
        session: SessionCredential::parse(SESSION).unwrap(),
        csrf: CsrfSecret::parse(CSRF).unwrap(),
        return_to: "/app".to_string(),
        max_age_seconds: 60,
    };
    let callback_result_debug = format!("{callback_result:?}");
    assert!(!callback_result_debug.contains(SESSION));
    assert!(!callback_result_debug.contains(CSRF));

    let start = OAuthStartResult {
        authorization_request: DiscordAuthorizationRequest {
            client_id: "123456789012345678".to_string(),
            callback_url: "https://starring.example/oauth/discord/callback".to_string(),
        },
        authorization_state: OAuthState::parse(STATE).unwrap(),
        browser_nonce: OAuthState::parse(NONCE).unwrap(),
        max_age_seconds: 60,
    };
    let start_debug = format!("{start:?}");
    assert!(!start_debug.contains(STATE));
    assert!(!start_debug.contains(NONCE));
    assert!(start_debug.contains("123456789012345678"));
    assert!(start_debug.contains("starring.example"));
}

#[test]
fn idempotency_key_keeps_the_existing_exact_transport_validation() {
    for valid in ["a", "request-1", "Request_1.2:retry"] {
        assert_eq!(IdempotencyKey::parse(valid).unwrap().expose_secret(), valid);
    }
    for invalid in ["", "contains space", "slash/value", "line\nbreak"] {
        assert!(IdempotencyKey::parse(invalid).is_err());
    }
    assert!(IdempotencyKey::parse(&"a".repeat(128)).is_ok());
    assert!(IdempotencyKey::parse(&"a".repeat(129)).is_err());
}

#[test]
fn secret_equality_preserves_only_boolean_semantics() {
    let first = OAuthState::parse(STATE).unwrap();
    let same = OAuthState::parse(STATE).unwrap();
    let different = OAuthState::parse(NONCE).unwrap();
    assert!(first == same);
    assert!(first != different);
}
