fn main() {
    #[cfg(target_os = "windows")]
    {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
        );
        let icon_path = manifest_dir.join("../../assets/app_icon.ico");

        println!("cargo:rerun-if-changed={}", icon_path.display());

        let mut res = winres::WindowsResource::new();
        res.set_icon(icon_path.to_str().expect("invalid icon path string"));
        res.set(
            "FileDescription",
            "skTerm GPU-accelerated Terminal Emulator",
        );
        res.set("ProductName", "skTerm");
        res.set("OriginalFilename", "skterm.exe");

        if let Err(e) = res.compile() {
            eprintln!("Failed to compile Windows resources: {e}");
            std::process::exit(1);
        }
    }
}
