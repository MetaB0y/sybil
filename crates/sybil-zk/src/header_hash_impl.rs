pub fn hash_header(header: &WitnessBlockHeader) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.height.to_le_bytes());
    hasher.update(&header.parent_hash);
    hasher.update(&header.state_root);
    hasher.update(&header.events_root);
    hasher.update(&header.order_count.to_le_bytes());
    hasher.update(&header.fill_count.to_le_bytes());
    hasher.update(&header.timestamp_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}
