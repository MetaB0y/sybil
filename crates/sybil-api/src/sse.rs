use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use matching_sequencer::SequencerHandle;

use crate::convert::public_block_to_response;
use crate::routes::blocks::PublicStreamPermit;

pub(crate) async fn block_stream(
    handle: &SequencerHandle,
    permit: PublicStreamPermit,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>> + use<>>,
    crate::types::AppError,
> {
    let rx = handle.subscribe_blocks().await?;
    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        // The stream closure owns the permit, so dropping the response body
        // releases capacity even if no block is ever emitted.
        let _permit = &permit;
        match result {
            Ok(block) => {
                let response = public_block_to_response(&block);
                let json = serde_json::to_string(&response).unwrap_or_default();
                Some(Ok(Event::default().data(json).event("block")))
            }
            Err(_) => None,
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
