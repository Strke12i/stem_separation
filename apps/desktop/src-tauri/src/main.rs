use local_music_analyzer_desktop::doctor::{DoctorReport, WorkerManager};
use local_music_analyzer_desktop::ingest::{IngestService, IngestedTrack};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tauri::command]
async fn doctor(state: tauri::State<'_, Arc<WorkerManager>>) -> Result<DoctorReport, String> {
    Ok(Arc::clone(state.inner()).doctor().await)
}

#[tauri::command]
async fn restart_worker(
    state: tauri::State<'_, Arc<WorkerManager>>,
) -> Result<DoctorReport, String> {
    Ok(Arc::clone(state.inner()).restart().await)
}

#[tauri::command]
async fn pick_and_import(
    state: tauri::State<'_, Arc<IngestService>>,
) -> Result<Option<IngestedTrack>, String> {
    let ingest = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let selected = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["mp3", "wav", "flac", "m4a", "aac", "ogg", "opus"],
            )
            .pick_file();
        selected
            .map(|path| ingest.import(&path).map_err(|error| error.to_string()))
            .transpose()
    })
    .await
    .map_err(|error| format!("Audio import task failed: {error}"))?
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let workers = Arc::new(WorkerManager::development());
    let ingest = Arc::new(IngestService::development());
    let startup_workers = Arc::clone(&workers);

    let result = tauri::Builder::default()
        .manage(workers)
        .manage(ingest)
        .setup(move |_app| {
            tauri::async_runtime::spawn(async move {
                let report = startup_workers.doctor().await;
                info!(
                    worker_ok = report.analysis_worker.ok,
                    "initial worker doctor completed"
                );
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            doctor,
            restart_worker,
            pick_and_import
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        tracing::error!(%error, "failed to run Tauri desktop application");
    }
}
