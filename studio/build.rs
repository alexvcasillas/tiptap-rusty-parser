fn main() {
    println!("cargo:rerun-if-changed=packaging/icon/icon.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("packaging/icon/icon.ico")
        .set("ProductName", "Tiptap Studio")
        .compile()
        .expect("embed the Windows icon");
}
