use super::*;

pub const MAX_BLOCK_HISTORY_QUERY_BLOCKS: usize = 500;
pub const DEFAULT_PRICE_HISTORY_QUERY_POINTS: usize = 500;
pub const MAX_PRICE_HISTORY_QUERY_POINTS: usize = 5_000;

pub(super) fn limit_price_point_page(
    mut points: Vec<PricePoint>,
    before_height: Option<u64>,
    limit: usize,
) -> PriceHistoryPage {
    if let Some(before_height) = before_height {
        points.retain(|point| point.height < before_height);
    }
    if limit == 0 {
        return PriceHistoryPage {
            points: Vec::new(),
            next_before_height: None,
            retention_min_height: None,
        };
    }

    if points.len() > limit {
        let page = points.split_off(points.len() - limit);
        PriceHistoryPage {
            next_before_height: page.first().map(|point| point.height),
            points: page,
            retention_min_height: None,
        }
    } else {
        PriceHistoryPage {
            points,
            next_before_height: None,
            retention_min_height: None,
        }
    }
}

pub(super) fn price_candle_page_from_points(
    points: Vec<PricePoint>,
    resolution_secs: u32,
    from_ms: Option<u64>,
    to_ms: Option<u64>,
    before_ms: Option<u64>,
    limit: usize,
) -> PriceCandlePage {
    if resolution_secs == 0 || limit == 0 {
        return PriceCandlePage {
            resolution_secs,
            candles: Vec::new(),
            next_before_ms: None,
            retention_min_bucket_ms: None,
        };
    }

    let mut by_bucket = BTreeMap::<u64, PriceCandle>::new();
    for point in points {
        if from_ms.is_some_and(|from| point.timestamp_ms < from)
            || to_ms.is_some_and(|to| point.timestamp_ms > to)
        {
            continue;
        }
        let candle = PriceCandle::from_point(resolution_secs, &point);
        if before_ms.is_some_and(|before| candle.bucket_start_ms >= before) {
            continue;
        }
        by_bucket
            .entry(candle.bucket_start_ms)
            .and_modify(|existing| existing.merge_point(&point))
            .or_insert(candle);
    }

    let mut candles = VecDeque::new();
    for candle in by_bucket.into_values() {
        if candles.len() == limit.saturating_add(1) {
            candles.pop_front();
        }
        candles.push_back(candle);
    }
    let mut candles: Vec<_> = candles.into_iter().collect();
    let next_before_ms = if candles.len() > limit {
        candles.remove(0);
        candles.first().map(|candle| candle.bucket_start_ms)
    } else {
        None
    };
    PriceCandlePage {
        resolution_secs,
        candles,
        next_before_ms,
        retention_min_bucket_ms: None,
    }
}

/// A market search result enriched with metadata, prices, and volume.
#[derive(Clone, Debug)]
pub struct MarketSearchResult {
    pub market_id: MarketId,
    pub name: String,
    pub metadata: Option<MarketMetadata>,
    pub yes_price_nanos: Option<Nanos>,
    pub no_price_nanos: Option<Nanos>,
    pub volume_nanos: u64,
    pub status: MarketStatus,
}

#[derive(Clone, Debug)]
pub struct SequencerStateProof {
    pub block_height: u64,
    pub state_root: [u8; 32],
    pub slot: AccountSnapshotSlot,
    pub leaf_key: Vec<u8>,
    pub verified: bool,
    pub kind: SequencerStateProofKind,
}

#[derive(Clone, Debug)]
pub enum SequencerStateProofKind {
    Inclusion {
        leaf_value: Vec<u8>,
        proof: QmdbStateKeyValueProofParts,
    },
    Exclusion {
        proof: QmdbStateExclusionProofParts,
    },
}

impl SequencerActorState {
    pub(super) fn handle_search_markets(
        &self,
        query: MarketSearchQuery,
    ) -> Vec<MarketSearchResult> {
        let markets = self.sequencer.markets();
        let mut results: Vec<MarketSearchResult> = Vec::new();

        for market in markets.iter() {
            let mid = market.id;
            let metadata = self.sequencer.market_metadata(mid);
            let status = self.sequencer.market_status(mid);

            if let Some(ref status_filter) = query.status {
                if status.as_str() != status_filter.as_str() {
                    continue;
                }
            }

            if let Some(ref text) = query.text {
                let text_lower = text.to_lowercase();
                let name_matches = market.name.to_lowercase().contains(&text_lower);
                let desc_matches = metadata
                    .as_ref()
                    .map(|m| m.description.to_lowercase().contains(&text_lower))
                    .unwrap_or(false);
                if !name_matches && !desc_matches {
                    continue;
                }
            }

            if let Some(ref filter_tags) = query.tags {
                let has_match = metadata
                    .as_ref()
                    .map(|m| filter_tags.iter().any(|t| m.tags.contains(t)))
                    .unwrap_or(false);
                if !has_match {
                    continue;
                }
            }

            if let Some(ref cat) = query.category {
                let matches = metadata
                    .as_ref()
                    .map(|m| &m.category == cat)
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
            }

            let market_prices = self.sequencer.analytics().last_clearing_prices().get(&mid);
            let yes_price = market_prices.and_then(|p| p.first().copied());
            let no_price = market_prices.and_then(|p| p.get(1).copied());
            let volume = self.sequencer.analytics().market_volume(mid);

            if let Some(min_p) = query.min_yes_price {
                if yes_price.unwrap_or(Nanos::ZERO) < min_p {
                    continue;
                }
            }
            if let Some(max_p) = query.max_yes_price {
                if yes_price.unwrap_or(Nanos::ZERO) > max_p {
                    continue;
                }
            }

            if let Some(min_vol) = query.min_volume {
                if volume < min_vol {
                    continue;
                }
            }

            results.push(MarketSearchResult {
                market_id: mid,
                name: market.name.clone(),
                metadata: metadata.cloned(),
                yes_price_nanos: yes_price,
                no_price_nanos: no_price,
                volume_nanos: volume,
                status,
            });
        }

        if let Some(ref sort_field) = query.sort_by {
            match sort_field {
                crate::market_info::MarketSortField::Volume => {
                    results.sort_by_key(|entry| std::cmp::Reverse(entry.volume_nanos));
                }
                crate::market_info::MarketSortField::CreatedAt => {
                    results.sort_by(|a, b| {
                        let a_ts = a.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        let b_ts = b.metadata.as_ref().map(|m| m.created_at_ms).unwrap_or(0);
                        b_ts.cmp(&a_ts)
                    });
                }
                crate::market_info::MarketSortField::Name => {
                    results.sort_by(|a, b| a.name.cmp(&b.name));
                }
                crate::market_info::MarketSortField::Price => {
                    results.sort_by(|a, b| {
                        b.yes_price_nanos
                            .unwrap_or(Nanos::ZERO)
                            .cmp(&a.yes_price_nanos.unwrap_or(Nanos::ZERO))
                    });
                }
            }
        }

        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(100);
        results.into_iter().skip(offset).take(limit).collect()
    }

    pub(super) async fn handle_state_proof(
        &self,
        leaf_key: Vec<u8>,
    ) -> Result<SequencerStateProof, SequencerError> {
        if leaf_key.len() > QMDB_STATE_MAX_KEY_BYTES {
            return Err(SequencerError::ProofUnavailable(format!(
                "state leaf key exceeds {QMDB_STATE_MAX_KEY_BYTES} bytes"
            )));
        }

        let Some(store) = &self.store else {
            return Err(SequencerError::ProofUnavailable(
                "state proofs require a persistent store".to_string(),
            ));
        };

        let root = store
            .current_state_qmdb_root()
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .ok_or(SequencerError::BlockNotFound)?;

        if let Some(proof) = store
            .current_state_qmdb_leaf_proof(&leaf_key)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
        {
            if proof.root != root.root || proof.slot != root.slot {
                return Err(SequencerError::Persistence(
                    "state proof root does not match committed qMDB root".to_string(),
                ));
            }
            return Ok(SequencerStateProof {
                block_height: self.sequencer.height(),
                state_root: proof.root,
                slot: proof.slot,
                leaf_key: proof.leaf_key.clone(),
                verified: proof.verify(),
                kind: SequencerStateProofKind::Inclusion {
                    leaf_value: proof.leaf_value.clone(),
                    proof: proof.proof_parts(),
                },
            });
        }

        let proof = store
            .current_state_qmdb_leaf_exclusion_proof(&leaf_key)
            .await
            .map_err(|error| SequencerError::Persistence(error.to_string()))?
            .ok_or_else(|| {
                SequencerError::Persistence(
                    "state qmdb returned neither inclusion nor exclusion proof".to_string(),
                )
            })?;
        if proof.root != root.root || proof.slot != root.slot {
            return Err(SequencerError::Persistence(
                "state proof root does not match committed qMDB root".to_string(),
            ));
        }

        Ok(SequencerStateProof {
            block_height: self.sequencer.height(),
            state_root: root.root,
            slot: proof.slot,
            leaf_key: proof.leaf_key.clone(),
            verified: proof.verify(),
            kind: SequencerStateProofKind::Exclusion {
                proof: proof.proof_parts(),
            },
        })
    }
}
