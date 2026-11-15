//! libFuzzer shim for the `crx_bitstream` target (structure-aware: capped CMP1
//! geometry from the harness, fuzz bytes replace the CRX sample payload).
//! Run for real:
//! `cargo fuzz run crx_bitstream --features libfuzzer -- -max_len=8388608 …`
#![cfg_attr(feature = "libfuzzer", no_main)]

#[cfg(feature = "libfuzzer")]
#[unsafe(no_mangle)]
pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> core::ffi::c_int {
    // SAFETY: libFuzzer passes a `data` pointer valid for `size` reads for the
    // duration of this call; the body copies nothing that outlives it.
    let bytes = unsafe { core::slice::from_raw_parts(data, size) };
    lightcraft_fuzz::crx_bitstream(bytes);
    0
}

#[cfg(not(feature = "libfuzzer"))]
fn main() {
    eprintln!(
        "`crx_bitstream` is a libFuzzer target; build it with `cargo +nightly fuzz run crx_bitstream --features libfuzzer` (Linux/Unix).\n\
         On Windows or stable toolchains, smoke-test the same entry point with `cargo run -p lightcraft-fuzz --bin mutate-smoke -- crx_bitstream fuzz/seeds/crx_bitstream`."
    );
    std::process::exit(2);
}
