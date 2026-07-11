use std::process::Command;

/// Stage 5 runs this drill on a proving-capable box. Both `#[ignore]` and the
/// explicit environment gate are intentional: no default or accidentally
/// filtered test command may start Halo2/EVM proving on a development machine.
#[test]
#[ignore = "Stage 5 real-proof drill; requires a proving-capable box"]
fn real_openvm_escape_drill_requires_explicit_gate() {
    if std::env::var("SYBIL_ESCAPE_PROVE").as_deref() != Ok("1") {
        eprintln!("skipped: set SYBIL_ESCAPE_PROVE=1 only on the Stage 5 proving box");
        return;
    }

    let required = |name: &str| {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} is required for the Stage 5 drill"))
    };
    let status = Command::new(env!("CARGO_BIN_EXE_sybil-custody"))
        .args([
            "escape-claim",
            "--snapshot",
            &required("SYBIL_ESCAPE_SNAPSHOT"),
            "--rpc-url",
            &required("SYBIL_ESCAPE_RPC_URL"),
            "--settlement",
            &required("SYBIL_ESCAPE_SETTLEMENT"),
            "--vault",
            &required("SYBIL_ESCAPE_VAULT"),
            "--recipient",
            &required("SYBIL_ESCAPE_RECIPIENT"),
            "--p256-private-key",
            &required("SYBIL_ESCAPE_P256_PRIVATE_KEY"),
            "--eth-private-key",
            &required("SYBIL_ESCAPE_ETH_PRIVATE_KEY"),
            "--submit",
        ])
        .status()
        .expect("run real escape-claim CLI");
    assert!(status.success(), "real OpenVM escape drill failed");
}
