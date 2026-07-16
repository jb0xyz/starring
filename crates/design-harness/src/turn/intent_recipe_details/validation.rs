use std::collections::BTreeSet;

use crate::errors::StructuredError;
use crate::intent::{
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, RoomNamePatternV1,
};

use super::super::intent_interpretation::{
    normalize_private_study_room_details, PrivateStudyRoomControlsInterpretationV2,
};
use super::super::IntentRecipeDetailExpectationV4;
use super::{
    IntentRecipeDetailFacetV3, PrivateStudyRoomDetailsCandidateV1, PrivateStudyRoomDetailsV1,
};

impl PrivateStudyRoomDetailsV1 {
    pub(crate) fn validate_human_literals(
        &self,
        human_message: &str,
    ) -> Result<(), StructuredError> {
        let human_message = human_message.replace("\r\n", "\n");
        validate_optional_literal(
            self.copy.launcher_content.as_deref(),
            &human_message,
            "intent.details.copy.launcher_content",
        )?;
        validate_optional_literal(
            self.copy.create_button_label.as_deref(),
            &human_message,
            "intent.details.copy.create_button_label",
        )?;
        validate_optional_literal(
            self.copy.modal_title.as_deref(),
            &human_message,
            "intent.details.copy.modal_title",
        )?;
        validate_optional_literal(
            self.copy.room_name_label.as_deref(),
            &human_message,
            "intent.details.copy.room_name_label",
        )?;
        validate_optional_pattern(
            self.copy.welcome_content.as_ref(),
            &human_message,
            "intent.details.copy.welcome_content",
        )?;
        validate_optional_pattern(
            self.copy.hub_announcement.as_ref(),
            &human_message,
            "intent.details.copy.hub_announcement",
        )?;
        validate_optional_pattern(
            self.copy.completed_response.as_ref(),
            &human_message,
            "intent.details.copy.completed_response",
        )?;
        validate_optional_pattern(
            self.naming.channel_name.as_ref(),
            &human_message,
            "intent.details.naming.channel_name",
        )?;
        validate_optional_pattern(
            self.naming.member_role_name.as_ref(),
            &human_message,
            "intent.details.naming.member_role_name",
        )?;
        validate_optional_literal(
            self.controls.help_label.as_deref(),
            &human_message,
            "intent.details.controls.help_label",
        )?;
        validate_optional_literal(
            self.controls.help_response.as_deref(),
            &human_message,
            "intent.details.controls.help_response",
        )?;
        validate_optional_literal(
            self.controls.join_label.as_deref(),
            &human_message,
            "intent.details.controls.join_label",
        )?;
        validate_optional_literal(
            self.controls.joined_response.as_deref(),
            &human_message,
            "intent.details.controls.joined_response",
        )?;
        validate_optional_literal(
            self.controls.close_label.as_deref(),
            &human_message,
            "intent.details.controls.close_label",
        )?;
        validate_optional_literal(
            self.controls.closed_response.as_deref(),
            &human_message,
            "intent.details.controls.closed_response",
        )
    }
}

pub(super) fn finalize_private_study_room_details(
    mut input: PrivateStudyRoomDetailsCandidateV1,
    required_facets: &[IntentRecipeDetailFacetV3],
    expected_revision: u64,
    expected_core_semantic_digest: &str,
) -> Result<PrivateStudyRoomDetailsV1, StructuredError> {
    normalize_private_study_room_details(&mut input.copy, &mut input.naming, &mut input.controls)?;
    let covered_facets = validate_facets(&input, required_facets)?;
    Ok(PrivateStudyRoomDetailsV1 {
        expected_revision,
        core_semantic_digest: expected_core_semantic_digest.to_string(),
        copy: input.copy,
        naming: input.naming,
        controls: input.controls,
        covered_facets,
    })
}

pub(super) fn validate_expected_detail_literals(
    details: &PrivateStudyRoomDetailsV1,
    expectations: &[IntentRecipeDetailExpectationV4],
) -> Result<(), StructuredError> {
    for expectation in expectations {
        let field = expectation.field();
        let actual = detail_literal(details, field);
        if actual != Some(expectation.literal()) {
            return Err(detail_error(
                "RECIPE_DETAIL_LITERAL_MISMATCH",
                format!(
                    "intent.details.{}.{}",
                    facet_name(field.facet()),
                    field.as_str()
                ),
                "A recipe detail value does not match its grounded human slot",
                "Copy the exact literal assigned to this exposed material leaf",
            ));
        }
    }
    Ok(())
}

fn detail_literal(
    details: &PrivateStudyRoomDetailsV1,
    field: super::super::IntentRecipeDetailFieldV4,
) -> Option<&str> {
    use super::super::IntentRecipeDetailFieldV4 as Field;

    match field {
        Field::LauncherContent => details.copy.launcher_content.as_deref(),
        Field::CreateButtonLabel => details.copy.create_button_label.as_deref(),
        Field::ModalTitle => details.copy.modal_title.as_deref(),
        Field::RoomNameLabel => details.copy.room_name_label.as_deref(),
        Field::WelcomeContentPrefix => details
            .copy
            .welcome_content
            .as_ref()
            .map(|value| value.prefix.as_str()),
        Field::WelcomeContentSuffix => details
            .copy
            .welcome_content
            .as_ref()
            .map(|value| value.suffix.as_str()),
        Field::HubAnnouncementPrefix => details
            .copy
            .hub_announcement
            .as_ref()
            .map(|value| value.prefix.as_str()),
        Field::HubAnnouncementSuffix => details
            .copy
            .hub_announcement
            .as_ref()
            .map(|value| value.suffix.as_str()),
        Field::CompletedResponsePrefix => details
            .copy
            .completed_response
            .as_ref()
            .map(|value| value.prefix.as_str()),
        Field::CompletedResponseSuffix => details
            .copy
            .completed_response
            .as_ref()
            .map(|value| value.suffix.as_str()),
        Field::ChannelNamePrefix => details
            .naming
            .channel_name
            .as_ref()
            .map(|value| value.prefix.as_str()),
        Field::ChannelNameSuffix => details
            .naming
            .channel_name
            .as_ref()
            .map(|value| value.suffix.as_str()),
        Field::MemberRoleNamePrefix => details
            .naming
            .member_role_name
            .as_ref()
            .map(|value| value.prefix.as_str()),
        Field::MemberRoleNameSuffix => details
            .naming
            .member_role_name
            .as_ref()
            .map(|value| value.suffix.as_str()),
        Field::HelpLabel => details.controls.help_label.as_deref(),
        Field::HelpResponse => details.controls.help_response.as_deref(),
        Field::JoinLabel => details.controls.join_label.as_deref(),
        Field::JoinedResponse => details.controls.joined_response.as_deref(),
        Field::CloseLabel => details.controls.close_label.as_deref(),
        Field::ClosedResponse => details.controls.closed_response.as_deref(),
    }
}

fn validate_optional_literal(
    value: Option<&str>,
    human_message: &str,
    location: &str,
) -> Result<(), StructuredError> {
    if value.is_some_and(|value| !human_message.contains(value)) {
        return Err(detail_error(
            "UNGROUNDED_RECIPE_DETAIL_LITERAL",
            location,
            "A recipe detail literal is not present in the current human turn",
            "Use one exact case-sensitive contiguous literal from the current human turn",
        ));
    }
    Ok(())
}

fn validate_optional_pattern(
    value: Option<&RoomNamePatternV1>,
    human_message: &str,
    location: &str,
) -> Result<(), StructuredError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.prefix.is_empty() && value.suffix.is_empty() {
        return Err(detail_error(
            "EMPTY_RECIPE_DETAIL_PATTERN",
            location,
            "A recipe detail pattern contains no literal affix",
            "Provide one exact non-empty prefix or suffix from the current human turn",
        ));
    }
    if !value.prefix.is_empty() {
        validate_optional_literal(
            Some(&value.prefix),
            human_message,
            &format!("{location}.prefix"),
        )?;
    }
    if !value.suffix.is_empty() {
        validate_optional_literal(
            Some(&value.suffix),
            human_message,
            &format!("{location}.suffix"),
        )?;
    }
    Ok(())
}

fn validate_facets(
    input: &PrivateStudyRoomDetailsCandidateV1,
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<Vec<IntentRecipeDetailFacetV3>, StructuredError> {
    let required = exact_facet_set(required_facets, "intent.details.required_facets")?;
    if required.is_empty() {
        return Err(detail_error(
            "EMPTY_RECIPE_DETAIL_REQUEST",
            "intent.details.required_facets",
            "The detail extractor has no active facet",
            "Use the deterministic default path when no detail facet is requested",
        ));
    }
    let unmapped = exact_facet_set(&input.unmapped_facets, "intent.details.unmapped_facets")?;
    if !unmapped.is_empty() {
        return Err(detail_error(
            "UNMAPPED_RECIPE_DETAIL_FACET",
            "intent.details.unmapped_facets",
            "At least one requested recipe detail facet was not mapped",
            "Map every selected copy, naming, or controls facet",
        ));
    }
    for facet in [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Naming,
        IntentRecipeDetailFacetV3::Controls,
    ] {
        let has_value = match facet {
            IntentRecipeDetailFacetV3::Copy => copy_has_value(&input.copy),
            IntentRecipeDetailFacetV3::Naming => naming_has_value(&input.naming),
            IntentRecipeDetailFacetV3::Controls => controls_have_value(&input.controls),
        };
        match (required.contains(&facet), has_value) {
            (true, false) => {
                return Err(detail_error(
                    "EMPTY_REQUIRED_RECIPE_DETAIL",
                    format!("intent.details.{}", facet_name(facet)),
                    "A selected recipe detail facet contains no value",
                    "Extract at least one explicit value for every selected facet",
                ));
            }
            (false, true) => {
                return Err(detail_error(
                    "UNREQUESTED_RECIPE_DETAIL",
                    format!("intent.details.{}", facet_name(facet)),
                    "An unselected recipe detail facet contains a value",
                    "Leave every unselected recipe detail object empty",
                ));
            }
            (true, true) | (false, false) => {}
        }
    }
    Ok(required.into_iter().collect())
}

pub(super) fn exact_facet_set(
    values: &[IntentRecipeDetailFacetV3],
    location: &str,
) -> Result<BTreeSet<IntentRecipeDetailFacetV3>, StructuredError> {
    if values.len() > 3 {
        return Err(detail_error(
            "TOO_MANY_RECIPE_DETAIL_FACETS",
            location,
            "More than three recipe detail facets were supplied",
            "Use only copy, naming, and controls",
        ));
    }
    let set = values.iter().copied().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(detail_error(
            "DUPLICATE_RECIPE_DETAIL_FACET",
            location,
            "A recipe detail facet appears more than once",
            "Provide each facet exactly once",
        ));
    }
    Ok(set)
}

fn copy_has_value(value: &PrivateStudyRoomCopyProposalV1) -> bool {
    value.launcher_content.is_some()
        || value.create_button_label.is_some()
        || value.modal_title.is_some()
        || value.room_name_label.is_some()
        || value.welcome_content.is_some()
        || value.hub_announcement.is_some()
        || value.completed_response.is_some()
}

fn naming_has_value(value: &PrivateStudyRoomNamingProposalV1) -> bool {
    value.channel_name.is_some() || value.member_role_name.is_some()
}

fn controls_have_value(value: &PrivateStudyRoomControlsInterpretationV2) -> bool {
    value.help_label.is_some()
        || value.help_response.is_some()
        || value.join_label.is_some()
        || value.joined_response.is_some()
        || value.close_label.is_some()
        || value.closed_response.is_some()
}

pub(super) fn facet_name(value: IntentRecipeDetailFacetV3) -> &'static str {
    match value {
        IntentRecipeDetailFacetV3::Copy => "copy",
        IntentRecipeDetailFacetV3::Naming => "naming",
        IntentRecipeDetailFacetV3::Controls => "controls",
    }
}

pub(super) fn detail_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError {
        code: code.into(),
        location: location.into(),
        message: message.into(),
        hint: hint.into(),
    }
}
