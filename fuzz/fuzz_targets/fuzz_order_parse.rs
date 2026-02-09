#![no_main]

use libfuzzer_sys::fuzz_target;
use sybil_api::types::request::SubmitOrderRequest;

// Fuzz the JSON parsing of SubmitOrderRequest.
// Must never panic — only Ok or Err.
fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<SubmitOrderRequest>(data);
});
