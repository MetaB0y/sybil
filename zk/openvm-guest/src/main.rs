use openvm::io::{read, reveal_bytes32};
use sybil_zk::{verify_state_transition_input, StateTransitionGuestInput};

fn main() {
    let input: StateTransitionGuestInput = read();
    let public_input_hash =
        verify_state_transition_input(&input).expect("invalid Sybil state transition witness");
    reveal_bytes32(public_input_hash);
}
