//! The Bottles importer's tests (`pindu-core/tests/import/bottles.rs`) run against the
//! built `pindu-import-bottles`: each import is a probe and an import request sent to the
//! program, and its answers are read back as Pindu reads them from a sub-sandboxed
//! helper. That the same assertions hold proves the snapshot written down is the snapshot.

#[path = "../../../pindu-core/tests/import/bottles.rs"]
mod bottles;
mod support;
