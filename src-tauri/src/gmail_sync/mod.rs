// Gmail synchronization commands and coordination.
pub mod service;
pub mod monitoring;
pub(crate) mod token_manager;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// Prevents overlapping sync jobs for the same Gmail account.
#[derive(Clone, Default)]
pub struct SyncCoordinator {
    active_accounts: Arc<Mutex<HashSet<i64>>>,
}

pub struct SyncPermit {
    active_accounts: Arc<Mutex<HashSet<i64>>>,
    account_id: i64,
}

impl SyncCoordinator {
    pub fn try_acquire(&self, account_id: i64) -> Result<SyncPermit, String> {
        let mut active = self
            .active_accounts
            .lock()
            .map_err(|error| format!("Sync coordinator lock failed: {error}"))?;

        if !active.insert(account_id) {
            return Err("Sinkronisasi untuk akun Gmail ini sedang berjalan.".into());
        }

        Ok(SyncPermit {
            active_accounts: Arc::clone(&self.active_accounts),
            account_id,
        })
    }
}

impl Drop for SyncPermit {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active_accounts.lock() {
            active.remove(&self.account_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_lock_is_exclusive_per_account_and_released_on_drop() {
        let coordinator = SyncCoordinator::default();
        let first = coordinator.try_acquire(10).unwrap();
        assert!(coordinator.try_acquire(10).is_err());
        assert!(coordinator.try_acquire(11).is_ok());
        drop(first);
        assert!(coordinator.try_acquire(10).is_ok());
    }
}
