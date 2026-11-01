use std::ffi::CStr;

pub(crate) struct Segment {
    pub addr: usize,
    pub size: usize,
    pub prot: i32,
}

pub(crate) struct Binding {
    pub addr: usize,
    pub name: &'static CStr,
    pub addend: i64,
    pub kind: u8,
}

pub(crate) struct Compact {
    pub start: u32,
    pub length: u32,
    pub encoding: u32,
    pub lsda: u32,
}

mod generated {
    use super::{Binding, Compact, Segment};

    include!(concat!(env!("OUT_DIR"), "/image.rs"));
}

#[cfg(feature = "embedded")]
pub(super) use generated::{BINDINGS, ENTRY, INITIALIZERS};
pub(super) use generated::{COMPACT, PERSONALITIES, SEGMENTS};
