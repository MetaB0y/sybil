use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use matching_sequencer::SequencerHandle;

use crate::convert::block_to_response;

pub async fn block_stream(
    handle: &SequencerHandle,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
    crate::types::AppError,
> {
    let rx = handle.subscribe_blocks().await?;
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(block) => {
            let response = block_to_response(&block);
            let json = serde_json::to_string(&response).unwrap_or_default();
            Some(Ok(Event::default().data(json).event("block")))
        }
        Err(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
