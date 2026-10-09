//! libFuzzer shim for the `iso_bmff_boxes` target (structure-aware box-graph
//! generator from the known CR3 fourcc pool). Run for real:
//! `cargo fuzz run iso_bmff_boxes --features libfuzzer -- -max_len=1048576 …`
#![cfg_attr(feature = "libfuzzer", no_main)]

#[cfg(feature = "libfuzzer")]
#[unsafe(no_mangle)]
pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> core::ffi::c_int {
    // SAFETY: libFuzzer passes a `data` pointer valid for `size` reads for the
    // duration of this call; the body copies nothing that outlives it.
    let bytes = unsafe { core::slice::from_raw_parts(data, size) };
    lightcraft_fuzz::iso_bmff_boxes(bytes);
    0
}

#[cfg(not(feature = "libfuzzer"))]
fn main() {
    eprintln!(
        "`iso_bmff_boxes` is a libFuzzer target; build it with `cargo +nightly fuzz run iso_bmff_boxes --features libfuzzer` (Linux/Unix).\n\
         On Windows or stable toolchains, smoke-test the same entry point with `cargo run -p lightcraft-fuzz --bin mutate-smoke -- iso_bmff_boxes fuzz/seeds/iso_bmff_boxes`."
    );
    std::process::exit(2);
}
