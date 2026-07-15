use std::io;

use sybil_api::app::openapi_document;

fn main() -> serde_json::Result<()> {
    serde_json::to_writer(io::stdout().lock(), &openapi_document(true))
}
