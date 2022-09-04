use openmls::prelude::{ParseMessageError, WelcomeError};
use std::fmt::Display;

#[derive(Debug)] /* 1 */
pub struct NodeError(pub String); /* 2 */

impl std::error::Error for NodeError {} /* 3 */

/* 4 */
impl Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<WelcomeError> for NodeError {
    fn from(error: WelcomeError) -> Self {
        NodeError(error.to_string())
    }
}

impl From<ParseMessageError> for NodeError {
    fn from(error: ParseMessageError) -> Self {
        NodeError(error.to_string())
    }
}
