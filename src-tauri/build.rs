fn main() {
    // Generate ACL permissions (allow-open-url / deny-open-url) for the
    // app-defined commands so capabilities/default.json can allow them —
    // app commands are ACL-gated like plugin commands in Tauri v2.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["open_url"])),
    )
    .expect("failed to run tauri-build");
}
