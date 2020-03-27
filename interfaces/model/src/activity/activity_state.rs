/*
 * Yagna Activity API
 *
 * It conforms with capability level 1 of the [Activity API specification](https://docs.google.com/document/d/1BXaN32ediXdBHljEApmznSfbuudTU8TmvOmHKl0gmQM).
 *
 * The version of the OpenAPI document: v1
 *
 * Generated by: https://openapi-generator.tech
 */

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityState {
    #[serde(rename = "state")]
    pub state: StatePair,
    /// Reason for Activity termination (specified when Activity in Terminated state).
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// If error caused state change - error message shall be provided.
    #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl ActivityState {
    pub fn alive(&self) -> bool {
        self.state.alive()
    }
}

impl From<&StatePair> for ActivityState {
    fn from(pending: &StatePair) -> Self {
        ActivityState {
            state: pending.clone(),
            reason: None,
            error_message: None,
        }
    }
}

impl From<StatePair> for ActivityState {
    fn from(pending: StatePair) -> Self {
        ActivityState {
            state: pending,
            reason: None,
            error_message: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct StatePair(pub State, pub Option<State>);

impl StatePair {
    pub fn alive(&self) -> bool {
        match (&self.0, &self.1) {
            (State::Terminated, _) => false,
            (_, Some(State::Terminated)) => false,
            _ => true,
        }
    }

    pub fn to_pending(&self, state: State) -> Self {
        StatePair(self.0.clone(), Some(state))
    }
}

impl From<State> for StatePair {
    fn from(state: State) -> Self {
        StatePair(state, None)
    }
}

impl Default for StatePair {
    fn default() -> Self {
        StatePair(State::default(), None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum State {
    New,
    Deployed,
    Ready,
    Active,
    Terminated,
}

impl Default for State {
    fn default() -> Self {
        State::New
    }
}
