use automation_core::{CreateChannelSpec, CreateRoleSpec};
use automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1;
use discord_model::Permissions;

const ROLE_POSTIMAGE_DOMAIN_V1: &[u8] = b"starring.runtime.discord_effect.role_postimage.v1\0";
const CHANNEL_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.discord_effect.channel_postimage.v1\0";
const ROLE_MEMBERSHIP_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.discord_effect.role_membership_postimage.v1\0";
const OVERWRITE_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.discord_effect.overwrite_postimage.v1\0";
const PANEL_POSTIMAGE_DOMAIN_V1: &[u8] = b"starring.runtime.discord_effect.panel_postimage.v1\0";
const INSTANCE_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.discord_effect.instance_postimage.v1\0";
const RESPONSE_POSTIMAGE_DOMAIN_V1: &[u8] =
    b"starring.runtime.discord_effect.response_postimage.v1\0";

pub(crate) fn expected_created_role_postimage_digest_v1(
    spec: &CreateRoleSpec,
) -> InteractionEffectExpectedPostimageDigestV1 {
    created_role_postimage_digest_v1(&spec.name, 0, false)
}

pub(crate) fn created_role_postimage_digest_v1(
    name: &str,
    permissions: u64,
    managed: bool,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(ROLE_POSTIMAGE_DOMAIN_V1);
    frame.text(name);
    frame.u64(permissions);
    frame.bool(managed);
    frame.finish()
}

pub(crate) fn expected_created_channel_postimage_digest_v1(
    spec: &CreateChannelSpec,
) -> InteractionEffectExpectedPostimageDigestV1 {
    created_channel_postimage_digest_v1(&spec.name, "text")
}

pub(crate) fn created_channel_postimage_digest_v1(
    name: &str,
    channel_kind: &str,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(CHANNEL_POSTIMAGE_DOMAIN_V1);
    frame.text(name);
    frame.text(channel_kind);
    frame.finish()
}

pub(crate) fn role_membership_postimage_digest_v1(
    present: bool,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(ROLE_MEMBERSHIP_POSTIMAGE_DOMAIN_V1);
    frame.bool(present);
    frame.finish()
}

pub(crate) fn overwrite_postimage_digest_v1(
    allow: Permissions,
    deny: Permissions,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(OVERWRITE_POSTIMAGE_DOMAIN_V1);
    frame.u64(allow.bits());
    frame.u64(deny.bits());
    frame.finish()
}

pub(crate) fn panel_postimage_digest_v1(
    content: &str,
    buttons: &[(String, String)],
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(PANEL_POSTIMAGE_DOMAIN_V1);
    frame.text(content);
    frame.u64(u64::try_from(buttons.len()).expect("bounded Discord button count fits u64"));
    for (label, custom_id) in buttons {
        frame.text(label);
        frame.text(custom_id);
    }
    frame.finish()
}

pub(crate) fn instance_postimage_digest_v1(
    manifest_digest: &str,
    present: bool,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(INSTANCE_POSTIMAGE_DOMAIN_V1);
    frame.text(manifest_digest);
    frame.bool(present);
    frame.finish()
}

pub(crate) fn response_postimage_digest_v1(
    content: &str,
) -> InteractionEffectExpectedPostimageDigestV1 {
    let mut frame = CanonicalPostimageFrameV1::new(RESPONSE_POSTIMAGE_DOMAIN_V1);
    frame.text(content);
    frame.finish()
}

struct CanonicalPostimageFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalPostimageFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(domain.len() + 128),
        };
        frame.bytes(domain);
        frame
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(
            &u64::try_from(value.len())
                .expect("bounded canonical postimage field length fits u64")
                .to_be_bytes(),
        );
        self.bytes.extend_from_slice(value);
    }

    fn finish(self) -> InteractionEffectExpectedPostimageDigestV1 {
        InteractionEffectExpectedPostimageDigestV1::from_canonical_bytes(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postimage_digests_are_deterministic_and_effect_specific() {
        let role = expected_created_role_postimage_digest_v1(&CreateRoleSpec {
            name: "study".to_string(),
        });
        assert_eq!(role, created_role_postimage_digest_v1("study", 0, false));
        assert_ne!(
            role,
            expected_created_channel_postimage_digest_v1(&CreateChannelSpec {
                name: "study".to_string(),
            })
        );
    }

    #[test]
    fn panel_digest_binds_ordered_exact_custom_ids_without_storing_payload() {
        let first =
            panel_postimage_digest_v1("panel", &[("join".to_string(), "i:room:join".to_string())]);
        let changed =
            panel_postimage_digest_v1("panel", &[("join".to_string(), "i:other:join".to_string())]);
        assert_ne!(first, changed);
    }
}
