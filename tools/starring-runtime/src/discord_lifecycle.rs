use std::num::NonZeroU64;
use std::time::Instant;

use automation_runtime::{
    GatewayAdmissionRevisionV3, GatewayAdmissionSequenceV3, GatewayAdmissionSnapshotV3,
    GatewayConnectionEpochV3, GatewayPauseTokenV3,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeDiscordPauseReservationIdentityV2 {
    epoch: GatewayConnectionEpochV3,
    admission_revision: GatewayAdmissionRevisionV3,
    transition_sequence: GatewayAdmissionSequenceV3,
}

impl RuntimeDiscordPauseReservationIdentityV2 {
    pub(crate) fn from_token(
        token: &GatewayPauseTokenV3,
        snapshot: GatewayAdmissionSnapshotV3,
    ) -> Option<Self> {
        let epoch = token.epoch()?;
        if token.admission_revision() != snapshot.admission_revision()
            || token.transition_sequence() != snapshot.transition_sequence()
            || snapshot.connection().current_epoch() != Some(epoch)
        {
            return None;
        }
        Some(Self {
            epoch,
            admission_revision: token.admission_revision(),
            transition_sequence: token.transition_sequence(),
        })
    }

    pub(crate) fn epoch(self) -> GatewayConnectionEpochV3 {
        self.epoch
    }

    pub(crate) fn admission_revision(self) -> GatewayAdmissionRevisionV3 {
        self.admission_revision
    }

    pub(crate) fn transition_sequence(self) -> GatewayAdmissionSequenceV3 {
        self.transition_sequence
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeDiscordAdmissionReservationSnapshotV2 {
    admission: GatewayAdmissionSnapshotV3,
    reservation: Option<RuntimeDiscordPauseReservationIdentityV2>,
}

impl RuntimeDiscordAdmissionReservationSnapshotV2 {
    pub(crate) fn unreserved(admission: GatewayAdmissionSnapshotV3) -> Self {
        Self {
            admission,
            reservation: None,
        }
    }

    pub(crate) fn reserved(
        admission: GatewayAdmissionSnapshotV3,
        token: &GatewayPauseTokenV3,
    ) -> Option<Self> {
        Some(Self {
            admission,
            reservation: Some(RuntimeDiscordPauseReservationIdentityV2::from_token(
                token, admission,
            )?),
        })
    }

    pub(crate) fn admission(self) -> GatewayAdmissionSnapshotV3 {
        self.admission
    }

    pub(crate) fn reservation(self) -> Option<RuntimeDiscordPauseReservationIdentityV2> {
        self.reservation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordActorModeV2 {
    StartupPaused {
        operation_cutoff: Instant,
    },
    ProcessSupervised {
        process_generation: NonZeroU64,
    },
    Draining {
        shutdown_generation: NonZeroU64,
        deadline: Instant,
    },
}

impl RuntimeDiscordActorModeV2 {
    pub(crate) fn deadline(self) -> Option<Instant> {
        match self {
            Self::StartupPaused { operation_cutoff } => Some(operation_cutoff),
            Self::ProcessSupervised { .. } => None,
            Self::Draining { deadline, .. } => Some(deadline),
        }
    }

    pub(crate) fn process_generation(self) -> Option<NonZeroU64> {
        match self {
            Self::ProcessSupervised { process_generation } => Some(process_generation),
            Self::StartupPaused { .. } | Self::Draining { .. } => None,
        }
    }
}
