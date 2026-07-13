use std::io;

use sybil_api::app::ApiDoc;
use utoipa::OpenApi;

fn main() -> serde_json::Result<()> {
    serde_json::to_writer(io::stdout().lock(), &ApiDoc::openapi())
}
