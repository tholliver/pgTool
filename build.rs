#[cfg(target_os = "windows")]
fn main() {
    // Embed the Windows exe icon (and future version info) into pgqb.exe.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app.ico");
    res.compile().expect("failed to compile Windows resources");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    // No-op on macOS/Linux — icon embedding only applies on Windows.
}
