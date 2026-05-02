#[cfg(target_os = "zkvm")]
extern crate openvm_sha2_guest;

use openvm::io::{read_vec, reveal_bytes32};
use sybil_zk::{verify_state_transition_input, StateTransitionGuestInput};

fn main() {
    let input = read_guest_input();
    let public_input_hash =
        verify_state_transition_input(&input).expect("invalid Sybil state transition witness");
    reveal_bytes32(public_input_hash);
}

fn read_guest_input() -> StateTransitionGuestInput {
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
    openvm::serde::from_slice(words.as_slice()).expect("invalid OpenVM guest input")
}
