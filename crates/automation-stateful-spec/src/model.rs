use automation_spec::{
    ActionNodeV1, DeclaredPanelV1, ModalDefinitionV1, TriggerV1, WorkflowSpecV1,
};
use serde::{Deserialize, Serialize};

pub const STATEFUL_SPEC_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_SPEC_KIND_V1: &str = "starring.stateful-spec.v1";
pub const MAX_SAFE_INTEGER_V1: i64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSpecV1 {
    pub schema_version: u16,
    pub kind: String,
    pub key: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub panels: Vec<DeclaredPanelV1>,
    #[serde(default)]
    pub modals: Vec<ModalDefinitionV1>,
    #[serde(default)]
    pub stateless_workflows: Vec<WorkflowSpecV1>,
    #[serde(default)]
    pub state_variables: Vec<StateVariableV1>,
    #[serde(default)]
    pub stateful_workflows: Vec<StatefulWorkflowV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateVariableV1 {
    pub id: String,
    pub scope: StateScopeV1,
    pub value_type: StateValueTypeV1,
    pub initial_value: StateValueV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateScopeV1 {
    Installation,
    Actor,
    Instance,
    ActorInstance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateValueTypeV1 {
    Bool,
    Integer { min: i64, max: i64 },
    Text { max_utf8_bytes: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateValueV1 {
    Bool { value: bool },
    Integer { value: i64 },
    Text { value: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatePrimitiveTypeV1 {
    Bool,
    Integer,
    Text,
}

impl StateValueTypeV1 {
    pub fn primitive_type(&self) -> StatePrimitiveTypeV1 {
        match self {
            Self::Bool => StatePrimitiveTypeV1::Bool,
            Self::Integer { .. } => StatePrimitiveTypeV1::Integer,
            Self::Text { .. } => StatePrimitiveTypeV1::Text,
        }
    }

    pub fn accepts(&self, value: &StateValueV1) -> bool {
        match (self, value) {
            (Self::Bool, StateValueV1::Bool { .. }) => true,
            (Self::Integer { min, max }, StateValueV1::Integer { value }) => {
                (-MAX_SAFE_INTEGER_V1..=MAX_SAFE_INTEGER_V1).contains(value)
                    && (*min..=*max).contains(value)
            }
            (Self::Text { max_utf8_bytes }, StateValueV1::Text { value }) => {
                value.len() <= usize::from(*max_utf8_bytes)
            }
            _ => false,
        }
    }
}

impl StateValueV1 {
    pub fn primitive_type(&self) -> StatePrimitiveTypeV1 {
        match self {
            Self::Bool { .. } => StatePrimitiveTypeV1::Bool,
            Self::Integer { .. } => StatePrimitiveTypeV1::Integer,
            Self::Text { .. } => StatePrimitiveTypeV1::Text,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulWorkflowV1 {
    pub id: String,
    pub trigger: TriggerV1,
    #[serde(default)]
    pub condition: StatefulConditionExprV1,
    pub on_true: StatefulBranchV1,
    pub on_false: StatefulBranchV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulBranchV1 {
    #[serde(default)]
    pub state_actions: Vec<StateSetNodeV1>,
    #[serde(default)]
    pub effects: Vec<ActionNodeV1>,
    pub response: StatefulResponseNodeV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulResponseNodeV1 {
    pub id: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateSetNodeV1 {
    pub id: String,
    pub variable_id: String,
    pub value: StatefulValueExprV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatefulValueExprV1 {
    Literal {
        value: StateValueV1,
    },
    InputText {
        input_id: String,
    },
    State {
        variable_id: String,
    },
    CheckedAdd {
        left: Box<StatefulValueExprV1>,
        right: Box<StatefulValueExprV1>,
    },
    CheckedSub {
        left: Box<StatefulValueExprV1>,
        right: Box<StatefulValueExprV1>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatefulConditionExprV1 {
    #[default]
    Always,
    InputNonEmpty {
        input_id: String,
    },
    InputEquals {
        input_id: String,
        value: String,
    },
    StateEquals {
        variable_id: String,
        value: StatefulValueExprV1,
    },
    IntegerCompare {
        left: StatefulValueExprV1,
        operator: IntegerComparisonV1,
        right: StatefulValueExprV1,
    },
    All {
        conditions: Vec<StatefulConditionExprV1>,
    },
    Any {
        conditions: Vec<StatefulConditionExprV1>,
    },
    Not {
        condition: Box<StatefulConditionExprV1>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegerComparisonV1 {
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}
