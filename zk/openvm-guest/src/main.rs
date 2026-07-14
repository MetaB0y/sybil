use openvm::io::{read_vec, reveal_bytes32};
#[allow(unused_imports)]
use openvm_p256::P256Point;
use sybil_zk::{EpochTransitionAccumulator, EpochTransitionHeader, StateTransitionGuestInput};

openvm::init!("openvm_init.rs");

fn main() {
    let header = read_epoch_header();
    header.validate().expect("invalid Sybil epoch header");
    let block_count = header.public_inputs.block_count;
    let mut accumulator = EpochTransitionAccumulator::new();
    for _ in 0..block_count {
        let input = read_guest_input();
        accumulator
            .push(&input)
            .expect("invalid Sybil epoch block transition");
    }
    let public_input_hash = accumulator
        .finish_and_verify(&header)
        .expect("Sybil epoch statement does not match streamed blocks");
    reveal_bytes32(public_input_hash);
}

fn read_epoch_header() -> EpochTransitionHeader {
    let words = read_stream_words();
    openvm::serde::from_slice(words.as_slice()).expect("invalid OpenVM epoch header")
}

fn read_guest_input() -> StateTransitionGuestInput {
    let words = read_stream_words();
    openvm::serde::from_slice(words.as_slice()).expect("invalid OpenVM guest block input")
}

fn read_stream_words() -> Vec<u32> {
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
    words
}
