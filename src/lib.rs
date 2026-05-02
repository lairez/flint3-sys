#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unnecessary_transmutes)]

pub mod ffi_prelude;

include!(concat!(env!("OUT_DIR"), "/types.rs"));
