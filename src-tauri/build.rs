fn main() {
    // The screencapturekit crate has Swift dependencies that reference
    // @rpath/libswift_Concurrency.dylib. On macOS 13+, the Swift Concurrency
    // runtime is part of the OS at /usr/lib/swift/. We add that as the primary
    // rpath so the system copy is used (avoiding duplicate class warnings from
    // the backport in CommandLineTools/swift-5.5).
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    tauri_build::build()
}
