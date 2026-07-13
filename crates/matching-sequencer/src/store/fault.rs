use super::*;
#[cfg(test)]
use std::collections::VecDeque;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreFaultPoint {
    BeforeQmdbPersist,
    AfterQmdbPersistBeforeRedbFence,
    BeforeRedbFenceCommit,
    AfterRedbFenceCommit,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct StoreFaultInjection {
    pub(super) save_block_faults: VecDeque<StoreFaultPoint>,
}

#[cfg(test)]
pub(super) fn pop_save_block_fault(
    fault_injection: &Arc<Mutex<StoreFaultInjection>>,
    point: StoreFaultPoint,
) -> Result<(), StoreError> {
    let mut faults = fault_injection
        .lock()
        .expect("store fault-injection lock poisoned");
    if faults.save_block_faults.front().copied() == Some(point) {
        faults.save_block_faults.pop_front();
        return Err(StoreError::InjectedFault(format!("{point:?}")));
    }
    Ok(())
}

impl Store {
    #[cfg(test)]
    pub(crate) fn inject_next_save_block_fault(&self, point: StoreFaultPoint) {
        self.fault_injection
            .lock()
            .expect("store fault-injection lock poisoned")
            .save_block_faults
            .push_back(point);
    }

    #[cfg(test)]
    pub(super) fn fail_save_block_at(&self, point: StoreFaultPoint) -> Result<(), StoreError> {
        pop_save_block_fault(&self.fault_injection, point)
    }
    #[cfg(test)]
    pub(super) fn save_block_faults(&self) -> Arc<Mutex<StoreFaultInjection>> {
        Arc::clone(&self.fault_injection)
    }
}
