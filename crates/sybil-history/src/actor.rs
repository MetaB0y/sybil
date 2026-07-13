use std::time::Duration;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use sybil_history_types::{ApplyBatchResponse, CommittedHistoryBatchV1};

use crate::{HistoryError, HistoryStore};

const APPLY_TIMEOUT: Duration = Duration::from_secs(30);

enum HistoryMessage {
    Apply(
        CommittedHistoryBatchV1,
        RpcReplyPort<Result<ApplyBatchResponse, HistoryError>>,
    ),
}

struct HistoryProjector;

#[ractor::async_trait]
impl Actor for HistoryProjector {
    type Msg = HistoryMessage;
    type State = HistoryStore;
    type Arguments = HistoryStore;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        store: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(store)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        store: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            HistoryMessage::Apply(batch, reply) => {
                let store = store.clone();
                let result = tokio::task::spawn_blocking(move || store.apply_batch(batch))
                    .await
                    .map_err(|error| HistoryError::BlockingTask(error.to_string()))
                    .and_then(|result| result);
                let _ = reply.send(result);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct HistoryHandle {
    actor: ActorRef<HistoryMessage>,
}

impl HistoryHandle {
    pub fn spawn(store: HistoryStore) -> Self {
        let (actor, _) = ractor::ActorRuntime::spawn_instant(None, HistoryProjector, store)
            .expect("failed to spawn history projector");
        Self { actor }
    }

    pub async fn apply(
        &self,
        batch: CommittedHistoryBatchV1,
    ) -> Result<ApplyBatchResponse, HistoryError> {
        match self
            .actor
            .call(
                |reply| HistoryMessage::Apply(batch, reply),
                Some(APPLY_TIMEOUT),
            )
            .await
        {
            Ok(ractor::rpc::CallResult::Success(result)) => result,
            Ok(ractor::rpc::CallResult::Timeout) => Err(HistoryError::BlockingTask(
                "history apply timed out".to_string(),
            )),
            _ => Err(HistoryError::BlockingTask(
                "history projector unavailable".to_string(),
            )),
        }
    }

    pub async fn stop_and_wait(&self, timeout: Duration) -> bool {
        self.actor.stop_and_wait(None, Some(timeout)).await.is_ok()
    }
}
