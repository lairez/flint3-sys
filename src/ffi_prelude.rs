//! Foreign substrate types intentionally used by the generated FLINT bindings.
//!
//! This module is not a place to model GMP, MPFR, or FLINT internals. If a
//! generated declaration needs those types, that declaration should be excluded
//! from the curated FFI for now.

pub use libc::{
    c_char, c_double, c_int, c_long, c_longlong, c_schar, c_short, c_uchar, c_uint, c_ulong,
    c_ulonglong, c_ushort, c_void,
};

pub use libc::FILE;

pub use libc::{
    pthread_attr_t, pthread_barrier_t, pthread_barrierattr_t, pthread_cond_t, pthread_condattr_t,
    pthread_key_t, pthread_mutex_t, pthread_mutexattr_t, pthread_once_t, pthread_rwlock_t,
    pthread_rwlockattr_t, pthread_spinlock_t, pthread_t,
};

pub type size_t = libc::size_t;
pub type ssize_t = libc::ssize_t;
