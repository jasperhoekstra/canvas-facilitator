// Explicit IPC allowlist: only these commands get permissions (see capabilities/default.json).
const COMMANDS: &[&str] = &[
    "app_status", "retry_storage", "save_key", "delete_key", "test_connection", "audio_devices", "mic_test_start",
    "mic_test_stop", "get_settings", "save_settings", "get_pricing", "cost_estimate", "create_session",
    "start_session", "resume_session", "pause_session", "stop_session", "set_mute", "send_text", "next_question", "inspire", "refill_field", "switch_input",
    "live_snapshot", "list_sessions", "get_session", "canvas_definition", "edit_item", "confirm_step", "add_action",
    "delete_note", "correct_turn", "rename_session", "delete_session", "cost_overview", "export_session", "export_costs",
    "export_diagnostics",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
