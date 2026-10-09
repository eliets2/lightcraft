//! libFuzzer shim for the `cr3_container` target. The fuzzed body lives in
//! `src/lib.rs` so `mutate-smoke` (any platform, stable toolchain) and this
//! binary (cargo fuzz, nightly Unix) exercise the same code. Run for real:
//! `cargo fuzz run cr3_container --features libfuzzer -- -max_len=1048576 …`
#![cfg_attr(feature = "libfuzzer", no_main)]

#[cfg(feature = "libfuzzer")]
#[unsafe(no_mangle)]
pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> core::ffi::c_int {
    // SAFETY: libFuzzer passes a `data` pointer valid for `size` reads for the
    // duration of this call; the body copies nothing that outlives it.
    let bytes = unsafe { core::slice::from_raw_parts(data, size) };
    lightcraft_fuzz::cr3_container(bytes);
    0
}

#[cfg(not(feature = "libfuzzer"))]
fn main() {
    eprintln!(
        "`cr3_container` is a libFuzzer target; build it with `cargo +nightly fuzz run cr3_container --features libfuzzer` (Linux/Unix).\n\
         On Windows or stable toolchains, smoke-test the same entry point with `cargo run -p lightcraft-fuzz --bin mutate-smoke -- cr3_container fuzz/seeds/cr3_container`."
    );
    std::process::exit(2);
}
