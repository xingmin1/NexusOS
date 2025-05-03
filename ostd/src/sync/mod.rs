// SPDX-License-Identifier: MPL-2.0

//! Useful synchronization primitives.

mod guard;
mod guard_rwarc;
mod guard_rwlock;
mod guard_spin;
mod rcu;

pub use maitake::sync::*;

pub(crate) use self::{guard::GuardTransfer, rcu::finish_grace_period};
pub use self::{
    guard::{LocalIrqDisabled, PreemptDisabled, WriteIrqDisabled},
    guard_rwarc::{GuardRoArc, GuardRwArc},
    guard_rwlock::{
        ArcRwLockReadGuard, ArcRwLockUpgradeableGuard, ArcRwLockWriteGuard, GuardRwLock,
        RwLockReadGuard, RwLockUpgradeableGuard, RwLockWriteGuard,
    },
    guard_spin::{ArcSpinLockGuard, GuardSpinLock, SpinLockGuard},
    rcu::{OwnerPtr, Rcu, RcuOption, RcuReadGuard},
};

pub(crate) fn init() {
    rcu::init();
}
