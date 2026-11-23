// Embeds the penny icon in the Windows .exe so Explorer and the taskbar show it.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "Penny Pincher");
        resource.set("FileDescription", "Penny Pincher");
        resource.compile().expect("embedding the Windows icon");
    }
}
