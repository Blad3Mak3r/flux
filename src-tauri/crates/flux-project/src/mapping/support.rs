//! Test helpers shared by the tests of the mapping modules.

use super::*;

pub fn added() -> Vec<Slice> {
    let mut slices = Vec::new();
    apply(&mut slices, MappingOp::Add, || "s1".into()).unwrap();
    slices
}
