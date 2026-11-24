#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("../../logos/files/ADBHUB-LINK.ico");
    if let Err(e) = res.compile() {
        eprintln!("Warning: Failed to compile Windows resource: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
