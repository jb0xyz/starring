use serde::{Deserialize, Serialize};

use crate::verdict::Verdict;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub verdict: Verdict,
    pub target: String,
    pub message: String,
}
