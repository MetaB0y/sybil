use super::*;

#[derive(Debug, Default)]
pub(crate) struct IndicativeSolveGate {
    in_flight: bool,
}

impl IndicativeSolveGate {
    pub(crate) fn try_start(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    pub(crate) fn finish(&mut self) {
        self.in_flight = false;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_second: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub(crate) fn new(refill_per_second: u32, capacity: u32, now: Instant) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_per_second: refill_per_second as f64,
            last_refill: now,
        }
    }

    pub(crate) fn allow(&mut self, now: Instant) -> Result<(), u64> {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        self.tokens =
            (self.tokens + elapsed.as_secs_f64() * self.refill_per_second).min(self.capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.retry_after_secs())
        }
    }

    fn retry_after_secs(&self) -> u64 {
        if self.refill_per_second <= 0.0 {
            return 1;
        }
        ((1.0 - self.tokens).max(0.0) / self.refill_per_second)
            .ceil()
            .max(1.0) as u64
    }
}

#[derive(Clone)]
pub(crate) struct MailboxMonitor {
    actor: &'static str,
    depth: Arc<AtomicUsize>,
    level: Arc<AtomicU8>,
    warn_depth: usize,
    error_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MailboxPressureLevel {
    Normal = 0,
    Warn = 1,
    Error = 2,
}

impl MailboxPressureLevel {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Warn,
            2 => Self::Error,
            _ => Self::Normal,
        }
    }
}

impl MailboxMonitor {
    pub(crate) fn new(actor: &'static str, warn_depth: usize, error_depth: usize) -> Self {
        Self {
            actor,
            depth: Arc::new(AtomicUsize::new(0)),
            level: Arc::new(AtomicU8::new(MailboxPressureLevel::Normal as u8)),
            warn_depth,
            error_depth,
        }
    }

    pub(crate) fn queued(&self) {
        let depth = self.depth.fetch_add(1, Ordering::Relaxed) + 1;
        self.record(depth);
    }

    pub(crate) fn started(&self) {
        let mut observed = self.depth.load(Ordering::Relaxed);
        loop {
            if observed == 0 {
                self.record(0);
                return;
            }

            match self.depth.compare_exchange_weak(
                observed,
                observed - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.record(observed - 1);
                    return;
                }
                Err(next) => observed = next,
            }
        }
    }

    pub(crate) fn send_failed(&self) {
        self.started();
    }

    pub(crate) fn reset(&self) {
        self.depth.store(0, Ordering::Relaxed);
        self.record(0);
    }

    #[cfg(test)]
    pub(crate) fn depth(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    fn pressure_level(&self, depth: usize) -> MailboxPressureLevel {
        if self.error_depth > 0 && depth >= self.error_depth {
            MailboxPressureLevel::Error
        } else if self.warn_depth > 0 && depth >= self.warn_depth {
            MailboxPressureLevel::Warn
        } else {
            MailboxPressureLevel::Normal
        }
    }

    fn record(&self, depth: usize) {
        metrics::gauge!("sybil_actor_queue_depth", "actor" => self.actor).set(depth as f64);

        let level = self.pressure_level(depth);
        let previous =
            MailboxPressureLevel::from_u8(self.level.swap(level as u8, Ordering::Relaxed));

        if level == previous {
            return;
        }

        match level {
            MailboxPressureLevel::Error => {
                tracing::error!(
                    actor = self.actor,
                    depth,
                    error_depth = self.error_depth,
                    "actor mailbox queue depth is critical"
                );
            }
            MailboxPressureLevel::Warn => {
                tracing::warn!(
                    actor = self.actor,
                    depth,
                    warn_depth = self.warn_depth,
                    "actor mailbox queue depth is high"
                );
            }
            MailboxPressureLevel::Normal => {
                tracing::info!(
                    actor = self.actor,
                    depth,
                    "actor mailbox queue depth recovered"
                );
            }
        }
    }
}
