mod binary;
mod commands;
mod exiftool;
mod write;

use tauri::Manager;

use commands::ExifToolState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(ExifToolState::default())
        .invoke_handler(tauri::generate_handler![
            commands::read_metadata,
            commands::exiftool_version,
            commands::startup_paths,
            commands::get_embedded_preview,
            commands::export_metadata,
            write::writable_info,
            write::write_metadata,
            write::save_as,
            binary::read_binary,
            binary::save_binary,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                commands::shutdown(&app.state::<ExifToolState>());
            }
        });
}
