use std::io;

fn main() -> serde_json::Result<()> {
    serde_json::to_writer(
        io::stdout().lock(),
        // The checked-in client schema also serves the Dev Zone, so it must
        // include the dev-only observation endpoints even though production
        // runtime registration remains configuration-gated.
        &sybil_api::app::openapi_document(true),
    )
}
