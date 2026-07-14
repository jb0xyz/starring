use crate::errors::StructuredError;
use crate::intent::{
    intent_capability_manifest_v2, CapabilityStatusV2, IntentCapabilityBlockerV2,
    IntentCapabilityIdV2, IntentSafetyBoundaryIdV2, IntentSafetyBoundaryViolationV2,
};
use crate::turn::IntentLocaleHintV2;

use super::adjudication_error;

pub(super) fn capability_gap_response(
    blockers: &[IntentCapabilityBlockerV2],
    locale: IntentLocaleHintV2,
) -> Result<String, StructuredError> {
    let labels = blockers
        .iter()
        .map(|blocker| capability_label(blocker, locale))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(match locale {
        IntentLocaleHintV2::Ko => format!(
            "요청을 그대로 보존했지만 다음 필수 역량을 현재 제공할 수 없어 컴파일하지 않았습니다: {labels}. 일부만 만들거나 요구사항을 약화하지 않았습니다."
        ),
        IntentLocaleHintV2::En | IntentLocaleHintV2::Unspecified => format!(
            "I preserved the request, but did not compile it because these required capabilities are not currently supported: {labels}. I did not build a partial or weakened version."
        ),
    })
}

pub(super) fn reject_response(
    violations: &[IntentSafetyBoundaryViolationV2],
    locale: IntentLocaleHintV2,
) -> Result<String, StructuredError> {
    let labels = violations
        .iter()
        .map(|violation| safety_boundary_label(violation.id, locale))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(match locale {
        IntentLocaleHintV2::Ko => format!(
            "안전한 설계는 도울 수 있지만 다음 요청된 안전 경계는 넘을 수 없습니다: {labels}. 검증, 미리보기, 사용자 승인, 비밀정보 보호는 계속 적용됩니다."
        ),
        IntentLocaleHintV2::En | IntentLocaleHintV2::Unspecified => format!(
            "I can help with a safe design, but cannot cross these requested safety boundaries: {labels}. Validation, preview, user approval, and secret protection remain enforced."
        ),
    })
}

pub(super) fn typed_planner_response(locale: IntentLocaleHintV2) -> &'static str {
    match locale {
        IntentLocaleHintV2::Ko => {
            "지원되는 정적 커스텀 자동화로 분류해 타입 기반 플래너로 전달했습니다. 라이브 시스템은 변경하지 않았습니다."
        }
        IntentLocaleHintV2::En | IntentLocaleHintV2::Unspecified => {
            "I routed this supported custom static automation to the typed planner. No live system was changed."
        }
    }
}

fn capability_label(
    blocker: &IntentCapabilityBlockerV2,
    locale: IntentLocaleHintV2,
) -> Result<String, StructuredError> {
    let manifest = intent_capability_manifest_v2();
    let descriptor = manifest
        .capabilities
        .iter()
        .find(|descriptor| descriptor.id == blocker.id)
        .ok_or_else(|| {
            adjudication_error(
                "UNKNOWN_INTENT_CAPABILITY",
                "intent.adjudication.blockers",
                format!(
                    "Capability {} is missing from the manifest",
                    blocker.id.as_str()
                ),
                "Use the current statically linked capability manifest",
            )
        })?;
    let (label, status) = match locale {
        IntentLocaleHintV2::Ko => (
            descriptor.label.ko.as_str(),
            capability_status_ko(blocker.status),
        ),
        IntentLocaleHintV2::En | IntentLocaleHintV2::Unspecified => (
            descriptor.label.en.as_str(),
            capability_status_en(blocker.status),
        ),
    };
    let base = format!("{label} ({status})");
    if blocker.id != IntentCapabilityIdV2::UnclassifiedIntentRequirement {
        return Ok(base);
    }
    let evidence = blocker
        .evidence
        .iter()
        .map(|evidence| evidence.description.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("{base}: {evidence}"))
}

fn safety_boundary_label(
    id: IntentSafetyBoundaryIdV2,
    locale: IntentLocaleHintV2,
) -> Result<String, StructuredError> {
    let manifest = intent_capability_manifest_v2();
    let descriptor = manifest
        .safety_boundaries
        .iter()
        .find(|descriptor| descriptor.id == id)
        .ok_or_else(|| {
            adjudication_error(
                "UNKNOWN_INTENT_SAFETY_BOUNDARY",
                "intent.adjudication.boundary_violations",
                format!(
                    "Safety boundary {} is missing from the manifest",
                    id.as_str()
                ),
                "Use the current statically linked capability manifest",
            )
        })?;
    Ok(match locale {
        IntentLocaleHintV2::Ko => descriptor.label.ko.clone(),
        IntentLocaleHintV2::En | IntentLocaleHintV2::Unspecified => descriptor.label.en.clone(),
    })
}

fn capability_status_en(status: CapabilityStatusV2) -> &'static str {
    match status {
        CapabilityStatusV2::Available => "available",
        CapabilityStatusV2::Unavailable => "unavailable",
        CapabilityStatusV2::ForbiddenPolicy => "forbidden by policy",
        CapabilityStatusV2::Unclassified => "unclassified",
    }
}

fn capability_status_ko(status: CapabilityStatusV2) -> &'static str {
    match status {
        CapabilityStatusV2::Available => "사용 가능",
        CapabilityStatusV2::Unavailable => "사용 불가",
        CapabilityStatusV2::ForbiddenPolicy => "정책상 금지",
        CapabilityStatusV2::Unclassified => "미분류",
    }
}
