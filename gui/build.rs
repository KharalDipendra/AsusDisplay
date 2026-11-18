fn main() {
    // Resource id 1 is the icon GPUI gives the window, and Explorer shows for the exe.
    #[cfg(windows)]
    embed_resource::compile("assets/icon.rc", embed_resource::NONE).manifest_optional().unwrap();
}
