use super::*;

const QUERY_RPC_CAPACITY: usize = 1_024;
const WRITE_RPC_CAPACITY: usize = 1_024;
const CONTROL_RPC_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug)]
pub(crate) enum RpcClass {
    Query,
    Write,
    Control,
}

impl RpcClass {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Write => "write",
            Self::Control => "control",
        }
    }
}

struct RpcAdmissionPool {
    class: RpcClass,
    capacity: usize,
    semaphore: Arc<Semaphore>,
    in_flight: AtomicUsize,
    high_water: AtomicUsize,
}

impl RpcAdmissionPool {
    fn new(class: RpcClass, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            class,
            capacity,
            semaphore: Arc::new(Semaphore::new(capacity)),
            in_flight: AtomicUsize::new(0),
            high_water: AtomicUsize::new(0),
        })
    }

    fn try_admit(self: &Arc<Self>) -> Result<RpcAdmissionPermit, SequencerError> {
        let permit = self.semaphore.clone().try_acquire_owned().map_err(|_| {
            metrics::counter!(
                "sybil_sequencer_rpc_admission_rejections_total",
                "class" => self.class.label()
            )
            .increment(1);
            SequencerError::ActorOverloaded {
                class: self.class.label(),
            }
        })?;
        let in_flight = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        self.high_water.fetch_max(in_flight, Ordering::Relaxed);
        metrics::gauge!(
            "sybil_sequencer_rpc_admission_in_flight",
            "class" => self.class.label()
        )
        .set(in_flight as f64);
        metrics::gauge!(
            "sybil_sequencer_rpc_admission_capacity",
            "class" => self.class.label()
        )
        .set(self.capacity as f64);
        metrics::gauge!(
            "sybil_sequencer_rpc_admission_high_water",
            "class" => self.class.label()
        )
        .set(self.high_water.load(Ordering::Relaxed) as f64);
        metrics::histogram!(
            "sybil_sequencer_rpc_admission_wait_seconds",
            "class" => self.class.label()
        )
        .record(0.0);
        Ok(RpcAdmissionPermit {
            pool: self.clone(),
            _permit: permit,
        })
    }
}

pub(crate) struct RpcAdmissionPermit {
    pool: Arc<RpcAdmissionPool>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for RpcAdmissionPermit {
    fn drop(&mut self) {
        let in_flight = self.pool.in_flight.fetch_sub(1, Ordering::Relaxed) - 1;
        metrics::gauge!(
            "sybil_sequencer_rpc_admission_in_flight",
            "class" => self.pool.class.label()
        )
        .set(in_flight as f64);
    }
}

#[derive(Clone)]
pub(crate) struct RpcAdmission {
    query: Arc<RpcAdmissionPool>,
    write: Arc<RpcAdmissionPool>,
    control: Arc<RpcAdmissionPool>,
}

impl RpcAdmission {
    pub(crate) fn new() -> Self {
        Self::from_capacities(QUERY_RPC_CAPACITY, WRITE_RPC_CAPACITY, CONTROL_RPC_CAPACITY)
    }

    #[cfg(test)]
    pub(crate) fn with_capacities(query: usize, write: usize, control: usize) -> Self {
        Self::from_capacities(query, write, control)
    }

    fn from_capacities(query: usize, write: usize, control: usize) -> Self {
        Self {
            query: RpcAdmissionPool::new(RpcClass::Query, query),
            write: RpcAdmissionPool::new(RpcClass::Write, write),
            control: RpcAdmissionPool::new(RpcClass::Control, control),
        }
    }

    pub(crate) fn try_admit(&self, class: RpcClass) -> Result<RpcAdmissionPermit, SequencerError> {
        match class {
            RpcClass::Query => self.query.try_admit(),
            RpcClass::Write => self.write.try_admit(),
            RpcClass::Control => self.control.try_admit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_admission_is_bounded_and_reserved_by_class() {
        let admission = RpcAdmission::with_capacities(1, 1, 1);
        let write = admission.try_admit(RpcClass::Write).unwrap();
        assert!(matches!(
            admission.try_admit(RpcClass::Write),
            Err(SequencerError::ActorOverloaded { class: "write" })
        ));

        let query = admission.try_admit(RpcClass::Query).unwrap();
        let control = admission.try_admit(RpcClass::Control).unwrap();
        assert!(matches!(
            admission.try_admit(RpcClass::Query),
            Err(SequencerError::ActorOverloaded { class: "query" })
        ));
        assert!(matches!(
            admission.try_admit(RpcClass::Control),
            Err(SequencerError::ActorOverloaded { class: "control" })
        ));

        drop(write);
        assert!(admission.try_admit(RpcClass::Write).is_ok());
        drop(query);
        drop(control);
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ScheduledTickGate {
    queued: Arc<AtomicBool>,
}

impl ScheduledTickGate {
    pub(crate) fn try_queue(&self) -> bool {
        self.queued
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn started(&self) {
        self.queued.store(false, Ordering::Release);
    }
}

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

pub(crate) fn rate_limiter(rate: u32, burst: u32) -> Ratelimiter {
    assert!(rate > 0, "rate limits must have a positive refill rate");
    assert!(burst > 0, "rate limits must have positive burst capacity");
    Ratelimiter::builder(u64::from(rate))
        .max_tokens(u64::from(burst))
        .initial_available(u64::from(burst))
        .build()
        .expect("validated rate limit")
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
