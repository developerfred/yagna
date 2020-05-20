use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

use ya_client::model::market::Proposal;
use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};

/// Requestor part of negotiation logic.
/// TODO: Too long name.
pub struct RequestorNegotiationEngine {
    db: DbExecutor,
    proposal_receiver: UnboundedReceiver<Proposal>,
}

impl RequestorNegotiationEngine {
    pub fn new(
        db: DbExecutor,
        proposal_receiver: UnboundedReceiver<Proposal>,
    ) -> Result<Arc<RequestorNegotiationEngine>, NegotiationInitError> {
        let engine = RequestorNegotiationEngine {
            db,
            proposal_receiver,
        };
        Ok(Arc::new(engine))
    }

    pub async fn bind_gsb(&self, prefix: String) -> Result<(), NegotiationInitError> {
        Ok(())
    }
}