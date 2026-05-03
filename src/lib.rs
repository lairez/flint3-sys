#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unnecessary_transmutes)]

pub use libc::{
    c_char, c_double, c_int, c_long, c_longlong, c_schar, c_short, c_uchar, c_uint, c_ulong,
    c_ulonglong, c_ushort, c_void,
};

pub use libc::FILE;

pub use libc::{
    pthread_attr_t, pthread_cond_t, pthread_condattr_t, pthread_key_t, pthread_mutex_t,
    pthread_mutexattr_t, pthread_once_t, pthread_rwlock_t, pthread_rwlockattr_t, pthread_t,
};

#[cfg(not(target_os = "macos"))]
pub use libc::{pthread_barrier_t, pthread_barrierattr_t, pthread_spinlock_t};

pub type size_t = libc::size_t;
pub type ssize_t = libc::ssize_t;

pub type __va_list_tag = u64;

include!(concat!(env!("OUT_DIR"), "/flint.rs"));
