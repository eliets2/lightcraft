//! libFuzzer shim for the `cr3_whole_file` target (mutated real CR3s through
//! probe → probe_info → preview → decode → develop). Run for real:
//! `cargo fuzz run cr3_whole_file --features libfuzzer -- -max_len=0 …`
#![cfg_attr(feature = "libfuzzer", no_main)]

#[cfg(feature = "libfuzzer")]
#[unsafe(no_mangle)]
pub extern "C" fn LLVMFuzzerTestOneInput(data: *const u8, size: usize) -> core::ffi::c_int {
    // SAFETY: libFuzzer passes a `data` pointer valid for `size` reads for the
    // duration of this call; the body copies nothing that outlives it.
    let bytes = unsafe { core::slice::from_raw_parts(data, size) };
    lightcraft_fuzz::cr3_whole_file(bytes);
    0
}

#[cfg(not(feature = "libfuzzer"))]
fn main() {
    eprintln!(
        "`cr3_whole_file` is a libFuzzer target; build it with `cargo +nightly fuzz run cr3_whole_file --features libfuzzer` (Linux/Unix).\n\
         On Windows or stable toolchains, smoke-test the same entry point with `cargo run -p lightcraft-fuzz --bin mutate-smoke -- cr3_whole_file <dir-with-real-CR3s>`."
    );
    std::process::exit(2);
}
