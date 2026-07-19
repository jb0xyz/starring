use automation_runtime_convergence::{
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use sha2::{Digest, Sha256};

use crate::model::{AttestationRecordV1, RuntimeDigestV1};

const DESIRED_TARGET_DOMAIN: &[u8] = b"starring.runtime.desired_target.v1\0";
const LIVE_ATTESTATION_DOMAIN: &[u8] = b"starring.runtime.live_attestation.v1\0";

pub(crate) fn desired_target_digest(
    identity: &RuntimeDeploymentIdentityV1,
    target: &RuntimeDeploymentTargetV1,
    runtime_generation: u64,
    authority_revision: u64,
    previous_runtime: Option<&RuntimeProcessIdentityV1>,
) -> RuntimeDigestV1 {
    let mut digest = LengthFramedDigest::new(DESIRED_TARGET_DOMAIN);
    digest.update(identity.deployment_id.as_str().as_bytes());
    digest.update(identity.tenant_id.as_str().as_bytes());
    digest.update(identity.installation_id.as_str().as_bytes());
    digest.update(identity.promotion_id.as_str().as_bytes());
    digest.update(identity.activation_request_id.as_str().as_bytes());
    update_target(&mut digest, target);
    digest.update(&runtime_generation.to_be_bytes());
    digest.update(&authority_revision.to_be_bytes());
    match previous_runtime {
        Some(previous) => {
            digest.update(b"present");
            update_target(&mut digest, &previous.target);
            digest.update(&previous.runtime_generation.get().to_be_bytes());
            digest.update(previous.process_instance_id.as_str().as_bytes());
        }
        None => digest.update(b"absent"),
    }
    RuntimeDigestV1::from_sha256(digest.finalize())
}

pub(crate) fn live_attestation_digest(
    record: &AttestationRecordV1,
) -> Result<RuntimeDigestV1, serde_json::Error> {
    let encoded = serde_json::to_vec(record)?;
    let mut digest = LengthFramedDigest::new(LIVE_ATTESTATION_DOMAIN);
    digest.update(&encoded);
    Ok(RuntimeDigestV1::from_sha256(digest.finalize()))
}

fn update_target(digest: &mut LengthFramedDigest, target: &RuntimeDeploymentTargetV1) {
    digest.update(target.guild_id.to_string().as_bytes());
    digest.update(target.ruleset_key.as_str().as_bytes());
    digest.update(&target.version.get().to_be_bytes());
    digest.update(target.content_hash.to_hex().as_bytes());
    digest.update(&target.binding_revision.get().to_be_bytes());
    digest.update(target.binding_fingerprint.as_str().as_bytes());
}

struct LengthFramedDigest {
    hasher: Sha256,
}

impl LengthFramedDigest {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        update_length_framed(&mut hasher, domain);
        Self { hasher }
    }

    fn update(&mut self, value: &[u8]) {
        update_length_framed(&mut self.hasher, value);
    }

    fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
