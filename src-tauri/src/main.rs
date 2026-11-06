mod commands;
mod detect;
mod tasks;

fn main() {
    tauri::Builder::default()
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_data,
            commands::refresh,
            commands::set_pm,
            commands::run_task,
            commands::list_packages,
            commands::remove_packages,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LinInstall");
}
