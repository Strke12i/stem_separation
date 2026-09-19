use local_music_analyzer_desktop::amt::{AmtReport, AmtService};
use local_music_analyzer_desktop::audio::AudioManager;
use local_music_analyzer_desktop::doctor::{DoctorReport, WorkerManager};
use local_music_analyzer_desktop::harmony::{HarmonyReport, HarmonyService};
use local_music_analyzer_desktop::ingest::{IngestService, IngestedTrack};
use local_music_analyzer_desktop::library::{
    LibraryEntry, LibraryQuery, LibraryService, LibrarySyncReport, LibraryTag,
};
use local_music_analyzer_desktop::pitch::{PitchReport, PitchService};
use local_music_analyzer_desktop::rhythm::{RhythmReport, RhythmService};
use local_music_analyzer_desktop::scheduler::ResourceScheduler;
use local_music_analyzer_desktop::separation::{
    ModelInfo, SeparationReport, SeparationService, SeparationStatus,
};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Tauri runs non-async commands on the main thread, so anything that can wait
/// on the disk or SQLite (`cached_*` read and validate JSON, library queries,
/// the library rebuild scans the workspace) would freeze the window. Run such
/// work on the blocking pool instead.
///
/// The playback controls stay synchronous on purpose: they are instant, and
/// running them on the pool would let two quick calls (dragging a volume
/// slider) reach the engine out of order.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("Background task failed: {error}"))?
}

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
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<Option<IngestedTrack>, String> {
    let ingest = Arc::clone(state.inner());
    let imported = tauri::async_runtime::spawn_blocking(move || {
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
    .map_err(|error| format!("Audio import task failed: {error}"))??;
    if let Some(track) = &imported {
        if let Err(error) = library.refresh(&track.track_id) {
            tracing::warn!(%error, "could not refresh the library index after import");
        }
    }
    Ok(imported)
}

#[tauri::command]
async fn load_original_track(
    track_id: String,
    ingest: tauri::State<'_, Arc<IngestService>>,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    let ingest = Arc::clone(ingest.inner());
    let audio = Arc::clone(audio.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let normalized_path = ingest
            .normalized_source_for(&track_id)
            .map_err(|error| error.to_string())?;
        audio.load_normalized(&normalized_path)
    })
    .await
    .map_err(|error| format!("Audio load task failed: {error}"))?
}

#[tauri::command]
async fn load_stem_mix(
    track_id: String,
    model_id: String,
    separation: tauri::State<'_, Arc<SeparationService>>,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    let separation = Arc::clone(separation.inner());
    let audio = Arc::clone(audio.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let stems = separation
            .stem_files(&track_id, &model_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|stem| analyzer_audio::StemInput {
                stem: stem.stem,
                path: stem.path,
            })
            .collect();
        audio.load_stems(stems)
    })
    .await
    .map_err(|error| format!("Stem mixer load task failed: {error}"))?
}

#[tauri::command]
fn audio_state(
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.snapshot()
}

#[tauri::command]
fn play_audio(
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.play()
}

#[tauri::command]
fn pause_audio(
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.pause()
}

#[tauri::command]
fn stop_audio(
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.stop()
}

#[tauri::command]
fn seek_audio(
    seconds: f64,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.seek(seconds)
}

#[tauri::command]
fn set_master_volume(
    volume: f32,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.set_volume(volume)
}

#[tauri::command]
fn set_stem_volume(
    stem: String,
    volume: f32,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.set_stem_volume(&stem, volume)
}

#[tauri::command]
fn set_stem_muted(
    stem: String,
    muted: bool,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.set_stem_muted(&stem, muted)
}

#[tauri::command]
fn set_stem_solo(
    stem: String,
    solo: bool,
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    audio.set_stem_solo(&stem, solo)
}

#[tauri::command]
async fn reopen_audio_device(
    audio: tauri::State<'_, Arc<AudioManager>>,
) -> Result<analyzer_audio::AudioState, String> {
    let audio = Arc::clone(audio.inner());
    blocking(move || audio.reopen_output_device()).await
}

#[tauri::command]
fn separation_models(separation: tauri::State<'_, Arc<SeparationService>>) -> Vec<ModelInfo> {
    separation.models()
}

#[tauri::command]
async fn separate_track(
    track_id: String,
    model_id: String,
    separation: tauri::State<'_, Arc<SeparationService>>,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<SeparationReport, String> {
    let result = Arc::clone(separation.inner())
        .separate(track_id.clone(), model_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = library.refresh(&track_id) {
        tracing::warn!(%error, "could not refresh the library index after separation");
    }
    Ok(result)
}

#[tauri::command]
async fn cancel_separation(
    track_id: String,
    separation: tauri::State<'_, Arc<SeparationService>>,
) -> Result<bool, String> {
    Ok(separation.cancel_for_track(&track_id).await)
}

#[tauri::command]
async fn separation_status(
    track_id: String,
    separation: tauri::State<'_, Arc<SeparationService>>,
) -> Result<Option<SeparationStatus>, String> {
    Ok(separation.status_for_track(&track_id).await)
}

#[tauri::command]
async fn analyze_rhythm(
    track_id: String,
    rhythm: tauri::State<'_, Arc<RhythmService>>,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<RhythmReport, String> {
    let result = Arc::clone(rhythm.inner())
        .analyze(track_id.clone())
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = library.refresh(&track_id) {
        tracing::warn!(%error, "could not refresh the library index after rhythm analysis");
    }
    Ok(result)
}

#[tauri::command]
async fn cached_rhythm(
    track_id: String,
    rhythm: tauri::State<'_, Arc<RhythmService>>,
) -> Result<Option<RhythmReport>, String> {
    let rhythm = Arc::clone(rhythm.inner());
    blocking(move || rhythm.cached(&track_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn analyze_harmony(
    track_id: String,
    harmony: tauri::State<'_, Arc<HarmonyService>>,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<HarmonyReport, String> {
    let result = Arc::clone(harmony.inner())
        .analyze(track_id.clone())
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = library.refresh(&track_id) {
        tracing::warn!(%error, "could not refresh the library index after harmony analysis");
    }
    Ok(result)
}

#[tauri::command]
async fn cached_harmony(
    track_id: String,
    harmony: tauri::State<'_, Arc<HarmonyService>>,
) -> Result<Option<HarmonyReport>, String> {
    let harmony = Arc::clone(harmony.inner());
    blocking(move || harmony.cached(&track_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn analyze_pitch(
    track_id: String,
    stem: String,
    pitch: tauri::State<'_, Arc<PitchService>>,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<PitchReport, String> {
    let result = Arc::clone(pitch.inner())
        .analyze(track_id.clone(), stem)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = library.refresh(&track_id) {
        tracing::warn!(%error, "could not refresh the library index after pitch analysis");
    }
    Ok(result)
}

#[tauri::command]
async fn cached_pitch(
    track_id: String,
    stem: String,
    pitch: tauri::State<'_, Arc<PitchService>>,
) -> Result<Option<PitchReport>, String> {
    let pitch = Arc::clone(pitch.inner());
    blocking(move || {
        pitch
            .cached(&track_id, &stem)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn transcribe_track(
    track_id: String,
    amt: tauri::State<'_, Arc<AmtService>>,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<AmtReport, String> {
    let result = Arc::clone(amt.inner())
        .transcribe(track_id.clone())
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = library.refresh(&track_id) {
        tracing::warn!(%error, "could not refresh the library index after transcription");
    }
    Ok(result)
}

#[tauri::command]
async fn cached_amt(
    track_id: String,
    amt: tauri::State<'_, Arc<AmtService>>,
) -> Result<Option<AmtReport>, String> {
    let amt = Arc::clone(amt.inner());
    blocking(move || amt.cached(&track_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn save_amt_midi(
    track_id: String,
    suggested_name: String,
    amt: tauri::State<'_, Arc<AmtService>>,
) -> Result<Option<String>, String> {
    let amt = Arc::clone(amt.inner());
    // The dialog and the write both happen in Rust: the UI never sees a path, and a
    // failure (no permission, disk full) comes back as an error instead of a
    // silently missing download. Read first so a missing transcription fails
    // before the user is asked where to save.
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = amt
            .midi_bytes(&track_id)
            .map_err(|error| error.to_string())?;
        let file_name = std::path::Path::new(&suggested_name)
            .file_name()
            .map_or_else(
                || "transcription.mid".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
        let Some(destination) = rfd::FileDialog::new()
            .set_file_name(&file_name)
            .add_filter("MIDI", &["mid"])
            .save_file()
        else {
            return Ok(None);
        };
        std::fs::write(&destination, bytes)
            .map_err(|error| format!("Could not save the MIDI file: {error}"))?;
        Ok(destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| format!("MIDI save task failed: {error}"))?
}

#[tauri::command]
async fn library_list(
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<Vec<LibraryEntry>, String> {
    let library = Arc::clone(library.inner());
    blocking(move || library.list().map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn library_search(
    query: LibraryQuery,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<Vec<LibraryEntry>, String> {
    let library = Arc::clone(library.inner());
    blocking(move || library.search(&query).map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn library_tags(
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<Vec<LibraryTag>, String> {
    let library = Arc::clone(library.inner());
    blocking(move || library.tags().map_err(|error| error.to_string())).await
}

#[tauri::command]
async fn library_add_tag(
    track_id: String,
    tag: String,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<LibraryEntry, String> {
    let library = Arc::clone(library.inner());
    blocking(move || {
        library
            .tag(&track_id, &tag)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn library_remove_tag(
    track_id: String,
    tag: String,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<LibraryEntry, String> {
    let library = Arc::clone(library.inner());
    blocking(move || {
        library
            .untag(&track_id, &tag)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn library_open_track(
    track_id: String,
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<LibraryEntry, String> {
    let library = Arc::clone(library.inner());
    blocking(move || {
        library
            .touch_opened(&track_id)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
async fn library_rebuild(
    library: tauri::State<'_, Arc<LibraryService>>,
) -> Result<LibrarySyncReport, String> {
    let library = Arc::clone(library.inner());
    blocking(move || library.rebuild().map_err(|error| error.to_string())).await
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let workers = Arc::new(WorkerManager::development());
    let ingest = Arc::new(IngestService::development());
    if let Err(error) = ingest.cleanup_stale_temporary() {
        tracing::warn!(%error, "could not clean stale temporary workspaces at startup");
    }
    if let Err(error) = ingest.repair_interrupted_workspaces() {
        tracing::warn!(%error, "could not repair interrupted workspaces at startup");
    }
    let library = Arc::new(match LibraryService::open(Arc::clone(&ingest)) {
        Ok(service) => service,
        Err(error) => {
            tracing::warn!(
                %error,
                "could not open the on-disk library index; using an in-memory index for this session"
            );
            // An in-memory SQLite connection has no filesystem dependency,
            // so failing here indicates a systemic problem (e.g. memory
            // exhaustion) rather than anything specific to the library
            // feature; there is no reasonable degraded mode left to fall
            // back to.
            LibraryService::in_memory(Arc::clone(&ingest))
                .expect("could not build even an in-memory library index")
        }
    });
    if let Err(error) = library.reconcile() {
        tracing::warn!(%error, "could not index the library at startup");
    }
    let audio = Arc::new(AudioManager::new());
    let scheduler = Arc::new(ResourceScheduler::new());
    let separation = Arc::new(SeparationService::new(
        Arc::clone(&ingest),
        Arc::clone(&workers),
        Arc::clone(&scheduler),
    ));
    let rhythm = Arc::new(RhythmService::new(
        Arc::clone(&ingest),
        Arc::clone(&workers),
    ));
    let harmony = Arc::new(HarmonyService::new(
        Arc::clone(&ingest),
        Arc::clone(&workers),
    ));
    let pitch = Arc::new(PitchService::new(Arc::clone(&ingest), Arc::clone(&workers)));
    let amt = Arc::new(AmtService::development(
        Arc::clone(&ingest),
        Arc::clone(&scheduler),
    ));
    let startup_workers = Arc::clone(&workers);
    let exit_workers = Arc::clone(&workers);
    let exit_amt = Arc::clone(&amt);

    let app = tauri::Builder::default()
        .manage(workers)
        .manage(ingest)
        .manage(audio)
        .manage(separation)
        .manage(rhythm)
        .manage(harmony)
        .manage(pitch)
        .manage(amt)
        .manage(library)
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
            pick_and_import,
            load_original_track,
            load_stem_mix,
            audio_state,
            play_audio,
            pause_audio,
            stop_audio,
            seek_audio,
            set_master_volume,
            set_stem_volume,
            set_stem_muted,
            set_stem_solo,
            reopen_audio_device,
            separation_models,
            separate_track,
            cancel_separation,
            separation_status,
            analyze_rhythm,
            cached_rhythm,
            analyze_harmony,
            cached_harmony,
            analyze_pitch,
            cached_pitch,
            transcribe_track,
            cached_amt,
            save_amt_midi,
            library_list,
            library_search,
            library_tags,
            library_add_tag,
            library_remove_tag,
            library_open_track,
            library_rebuild
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(move |_app_handle, event| {
            // Sidecar children are spawned with `kill_on_drop`, but that only
            // fires once every Arc<WorkerManager/AmtService> is dropped, which
            // is not guaranteed to happen before the process itself exits.
            // Shut them down explicitly and synchronously on the way out so a
            // `uv`/python child holding model memory never outlives the app.
            if let tauri::RunEvent::Exit = event {
                let workers = Arc::clone(&exit_workers);
                let amt = Arc::clone(&exit_amt);
                tauri::async_runtime::block_on(async move {
                    workers.shutdown().await;
                    amt.shutdown().await;
                });
            }
        }),
        Err(error) => tracing::error!(%error, "failed to build Tauri desktop application"),
    }
}
