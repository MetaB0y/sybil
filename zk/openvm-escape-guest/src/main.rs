use openvm::io::{read_vec, reveal_bytes32};
#[allow(unused_imports)]
use openvm_p256::P256Point;
use sybil_escape_claim::{EscapeClaimGuestInput, verify_escape_claim};

openvm::init!("openvm_init.rs");

fn main() {
    let input = read_guest_input();
    let public_input_hash = verify_escape_claim(&input).expect("invalid Sybil escape claim");
    reveal_bytes32(public_input_hash);
}

fn read_guest_input() -> EscapeClaimGuestInput {
    let bytes = read_vec();
    let mut chunks = bytes.chunks_exact(core::mem::size_of::<u32>());
    let words = chunks
        .by_ref()
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("u32 chunk")))
        .collect::<Vec<_>>();
    assert!(
        chunks.remainder().is_empty(),
        "OpenVM guest input is not u32-aligned"
    );
    openvm::serde::from_slice(words.as_slice()).expect("invalid OpenVM escape guest input")
}
