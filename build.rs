fn main() {
    // Embed the Windows exe icon (and future version info) into pgqb.exe.
    // Only applies on Windows; no-op on other targets.
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.compile().expect("failed to compile Windows resources");
    }
}
