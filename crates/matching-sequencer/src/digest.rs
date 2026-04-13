use matching_engine::{MarketId, MintAdjustment, Nanos, Qty};

pub fn update_digest(current: &[u8; 32], event_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(current);
    hasher.update(event_bytes);
    *hasher.finalize().as_bytes()
}

pub fn encode_fill_event(
    order_id: u64,
    fill_qty: Qty,
    fill_price: Nanos,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 * 4);
    bytes.push(0x01);
    bytes.extend_from_slice(&order_id.to_le_bytes());
    bytes.extend_from_slice(&fill_qty.to_le_bytes());
    bytes.extend_from_slice(&fill_price.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_deposit_event(amount: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8);
    bytes.push(0x02);
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_resolution_event(
    market_id: MarketId,
    payout_nanos: Nanos,
    block_height: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 4 + 8 + 8);
    bytes.push(0x03);
    bytes.extend_from_slice(&market_id.0.to_le_bytes());
    bytes.extend_from_slice(&payout_nanos.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_create_account_event(initial_balance: i64, block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + 8);
    bytes.push(0x04);
    bytes.extend_from_slice(&initial_balance.to_le_bytes());
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

pub fn encode_mint_event(adjustments: &[MintAdjustment], block_height: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + adjustments.len() * (4 + 1 + 8 + 8));
    bytes.push(0x05);
    bytes.extend_from_slice(&(adjustments.len() as u64).to_le_bytes());
    for adjustment in adjustments {
        bytes.extend_from_slice(&adjustment.market_id.0.to_le_bytes());
        bytes.push(adjustment.outcome);
        bytes.extend_from_slice(&adjustment.position_delta.to_le_bytes());
        bytes.extend_from_slice(&adjustment.balance_delta.to_le_bytes());
    }
    bytes.extend_from_slice(&block_height.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digest_deterministic() {
        let event = encode_fill_event(7, 10, 500_000_000, 12);
        assert_eq!(
            update_digest(&[0u8; 32], &event),
            update_digest(&[0u8; 32], &event)
        );
    }

    #[test]
    fn test_digest_sensitive_to_event_bytes() {
        let fill = encode_fill_event(7, 10, 500_000_000, 12);
        let deposit = encode_deposit_event(500_000_000, 12);
        assert_ne!(
            update_digest(&[0u8; 32], &fill),
            update_digest(&[0u8; 32], &deposit)
        );
    }
}
