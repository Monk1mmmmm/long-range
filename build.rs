// Puts the program icon (res/longrange.ico) into the Windows executables. Other targets need nothing.
fn main() {
    println!("cargo:rerun-if-changed=res/longrange.rc");
    println!("cargo:rerun-if-changed=res/longrange.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // MSVC builds use rc.exe, the MinGW cross build x86_64-w64-mingw32-windres.
        embed_resource::compile("res/longrange.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
