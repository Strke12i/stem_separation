<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';

  type ComponentStatus = { ok: boolean; detail: string };
  type DoctorReport = { desktopCore: ComponentStatus; analysisWorker: ComponentStatus; protocolVersion: number };
  type Track = { trackId: string; originalName: string; durationSeconds: number; sampleRate: number; channels: number };
  type AudioState = { status: 'empty' | 'paused' | 'playing' | 'ended' | 'device_error'; currentPositionSeconds: number; durationSeconds: number; volume: number; deviceError: string | null; waveform: { min: number[]; max: number[] } | null; stemMix: boolean; stems: StemState[] };
  type StemState = { stem: string; volume: number; muted: boolean; solo: boolean };
  type SeparationModel = { id: string; label: string; experimental: boolean; stems: string[]; installed: boolean; detail: string };
  type SeparationReport = { jobId: string; trackId: string; modelId: string; cacheHit: boolean; stems: string[]; progressEvents: number };
  type SeparationStatus = { jobId: string; modelId: string; stage: string; progress: number; elapsedSeconds: number; cancelRequested: boolean };
  type RhythmReport = { bpm: number; beatTimes: number[]; algorithm: string; stabilityScore: number; cacheHit: boolean };
  type HarmonyReport = { key: { label: string }; chords: { start: number; end: number; label: string }[]; algorithm: string; cacheHit: boolean };
  type PitchReport = { stem: string; engine: string; notes: unknown[]; cacheHit: boolean };
  type MidiNote = { start: number; end: number; midi: number; velocity: number };
  type AmtReport = { engine: string; model: string; notes: MidiNote[]; midiArtifact: string; cacheHit: boolean };
  type LibraryEntry = { trackId: string; originalName: string; durationSeconds: number; sampleRate: number; channels: number; sourceSha256: string; bpm: number | null; keyLabel: string | null; stemModels: string[]; analyzed: string[]; importedAt: string | null; lastOpenedAt: string | null; openCount: number; tags: string[] };
  type LibraryTag = { tag: string; trackCount: number };
  type LibrarySort = 'recent' | 'name';
  type LibraryQuery = { text: string; tags: string[]; sort: LibrarySort };
  type LibrarySyncReport = { indexed: number; updated: number; removed: number; rebuilt: boolean };
  type Tab = 'library' | 'workspace' | 'separation' | 'transcription' | 'analysis' | 'mixer';

  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: 'library', label: 'Library', icon: '▤' },
    { id: 'workspace', label: 'Workspace', icon: '◫' }, { id: 'separation', label: 'Stems', icon: '✦' },
    { id: 'transcription', label: 'MIDI', icon: '♫' }, { id: 'analysis', label: 'Analysis', icon: '⌁' }, { id: 'mixer', label: 'Mixer', icon: '≋' }
  ];
  const separationSteps = [
    { label: 'Prepare workspace', threshold: 0.05 }, { label: 'Load local model', threshold: 0.12 },
    { label: 'Separate stems', threshold: 0.2 }, { label: 'Validate audio', threshold: 0.92 }, { label: 'Publish results', threshold: 0.97 }
  ];
  const MIDI_WINDOW_SECONDS = 16;
  const MIDI_PREVIEW_SECONDS = 12;
  const MAX_PIANO_ROLL_NOTES = 900;
  const MAX_CHORDS = 320;
  const MAX_BEATS = 400;
  const MAX_PREVIEW_VOICES = 48;

  let report: DoctorReport | undefined;
  let track: Track | undefined;
  let audio: AudioState | undefined;
  let models: SeparationModel[] = [];
  let activeTab: Tab = 'workspace';
  let checking = true;
  let importing = false;
  let importError: string | undefined;
  let audioError: string | undefined;
  let separationError: string | undefined;
  let separating: string | undefined;
  let separationStatus: SeparationStatus | undefined;
  let separationResult: SeparationReport | undefined;
  let rhythm: RhythmReport | undefined;
  let harmony: HarmonyReport | undefined;
  let pitch: Record<string, PitchReport | undefined> = {};
  let analyzingRhythm = false;
  let analyzingHarmony = false;
  let analyzingPitch: string | undefined;
  let rhythmError: string | undefined;
  let harmonyError: string | undefined;
  let pitchError: string | undefined;
  let amt: AmtReport | undefined;
  let transcribing = false;
  let amtError: string | undefined;
  let midiWindowStart = 0;
  let midiPreviewPlaying = false;
  let waveformCanvas: HTMLCanvasElement | undefined;
  let seeking = false;
  let seekValue = 0;
  let volumeTimer: ReturnType<typeof window.setTimeout> | undefined;
  let midiTimer: ReturnType<typeof window.setTimeout> | undefined;
  let midiContext: AudioContext | undefined;
  let oscillators: OscillatorNode[] = [];
  let libraryEntries: LibraryEntry[] = [];
  let libraryTags: LibraryTag[] = [];
  let librarySearchText = '';
  let librarySelectedTags: string[] = [];
  let librarySort: LibrarySort = 'recent';
  let libraryError: string | undefined;
  let libraryBusy = false;
  let libraryTimer: ReturnType<typeof window.setTimeout> | undefined;
  let libraryTagDraft: Record<string, string> = {};

  onMount(() => {
    void runDoctor(); void loadModels();
    const timer = window.setInterval(() => {
      if (track !== undefined) {
        void refreshAudio();
        if (separating !== undefined || separationStatus !== undefined) void refreshSeparation();
      }
    }, 350);
    return () => { window.clearInterval(timer); if (volumeTimer) window.clearTimeout(volumeTimer); if (libraryTimer) window.clearTimeout(libraryTimer); stopMidiPreview(); };
  });

  async function runDoctor(restart = false): Promise<void> {
    checking = true;
    try { report = await invoke<DoctorReport>(restart ? 'restart_worker' : 'doctor'); }
    catch (error) { report = { desktopCore: { ok: true, detail: 'Rust host is running' }, analysisWorker: { ok: false, detail: String(error) }, protocolVersion: 1 }; }
    finally { checking = false; }
  }
  async function loadModels(): Promise<void> { try { models = await invoke<SeparationModel[]>('separation_models'); } catch (error) { separationError = String(error); } }
  function selectTab(tab: Tab): void { activeTab = tab; if (tab === 'library') void refreshLibrary(); void tick().then(drawWaveform); }
  async function refreshAudio(): Promise<void> { try { audio = await invoke<AudioState>('audio_state'); audioError = undefined; } catch (error) { audioError = String(error); } }
  async function refreshSeparation(): Promise<void> {
    if (!track) return;
    try { const value = await invoke<SeparationStatus | null>('separation_status', { trackId: track.trackId }); if (value !== null) separationStatus = value; else if (!separating) separationStatus = undefined; } catch { /* do not interrupt a long-running job */ }
  }
  async function adoptTrack(next: Track): Promise<void> {
    stopMidiPreview(); track = next; activeTab = 'workspace'; midiWindowStart = 0; rhythm = undefined; harmony = undefined; pitch = {}; amt = undefined;
    separationResult = undefined; separating = undefined; separationStatus = undefined; separationError = undefined;
    await loadOriginal(next.trackId);
    await Promise.all([loadCachedRhythm(next.trackId), loadCachedHarmony(next.trackId), loadCachedPitch(next.trackId, 'bass'), loadCachedPitch(next.trackId, 'vocals'), loadCachedAmt(next.trackId)]);
  }
  async function importTrack(): Promise<void> {
    importing = true; importError = undefined;
    try {
      const result = await invoke<Track | null>('pick_and_import');
      if (result) { await adoptTrack(result); void refreshLibrary(); }
    } catch (error) { importError = String(error); }
    finally { importing = false; }
  }
  async function openFromLibrary(entry: LibraryEntry): Promise<void> {
    try {
      const opened = await invoke<LibraryEntry>('library_open_track', { trackId: entry.trackId });
      await adoptTrack({ trackId: opened.trackId, originalName: opened.originalName, durationSeconds: opened.durationSeconds, sampleRate: opened.sampleRate, channels: opened.channels });
      void refreshLibrary();
    } catch (error) { libraryError = String(error); }
  }
  async function refreshLibrary(): Promise<void> {
    libraryBusy = true; libraryError = undefined;
    try {
      const [entries, tags] = await Promise.all([invoke<LibraryEntry[]>('library_list'), invoke<LibraryTag[]>('library_tags')]);
      libraryEntries = entries; libraryTags = tags;
    } catch (error) { libraryError = String(error); }
    finally { libraryBusy = false; }
  }
  async function runLibrarySearch(): Promise<void> {
    try {
      const query: LibraryQuery = { text: librarySearchText, tags: librarySelectedTags, sort: librarySort };
      libraryEntries = await invoke<LibraryEntry[]>('library_search', { query });
    } catch (error) { libraryError = String(error); }
  }
  function scheduleLibrarySearch(): void { if (libraryTimer) window.clearTimeout(libraryTimer); libraryTimer = window.setTimeout(() => void runLibrarySearch(), 120); }
  function toggleTagFilter(tag: string): void {
    librarySelectedTags = librarySelectedTags.includes(tag) ? librarySelectedTags.filter((item) => item !== tag) : [...librarySelectedTags, tag];
    void runLibrarySearch();
  }
  async function addTag(entry: LibraryEntry): Promise<void> {
    const tag = (libraryTagDraft[entry.trackId] ?? '').trim();
    if (!tag) return;
    try {
      const updated = await invoke<LibraryEntry>('library_add_tag', { trackId: entry.trackId, tag });
      libraryEntries = libraryEntries.map((item) => (item.trackId === updated.trackId ? updated : item));
      libraryTagDraft = { ...libraryTagDraft, [entry.trackId]: '' };
      libraryTags = await invoke<LibraryTag[]>('library_tags');
    } catch (error) { libraryError = String(error); }
  }
  async function removeTag(entry: LibraryEntry, tag: string): Promise<void> {
    try {
      const updated = await invoke<LibraryEntry>('library_remove_tag', { trackId: entry.trackId, tag });
      libraryEntries = libraryEntries.map((item) => (item.trackId === updated.trackId ? updated : item));
      libraryTags = await invoke<LibraryTag[]>('library_tags');
    } catch (error) { libraryError = String(error); }
  }
  async function rebuildLibrary(): Promise<void> {
    libraryBusy = true; libraryError = undefined;
    try { await invoke<LibrarySyncReport>('library_rebuild'); await refreshLibrary(); }
    catch (error) { libraryError = String(error); }
    finally { libraryBusy = false; }
  }
  async function loadOriginal(trackId: string): Promise<void> { try { audio = await invoke<AudioState>('load_original_track', { trackId }); audioError = undefined; await tick(); drawWaveform(); } catch (error) { audioError = String(error); } }
  async function loadStemMix(modelId: string): Promise<void> { if (!track) return; try { audio = await invoke<AudioState>('load_stem_mix', { trackId: track.trackId, modelId }); audioError = undefined; selectTab('mixer'); await tick(); drawWaveform(); } catch (error) { audioError = String(error); } }
  async function audioCommand(command: string, args: Record<string, unknown> = {}): Promise<void> { try { audio = await invoke<AudioState>(command, args); audioError = undefined; } catch (error) { audioError = String(error); } }
  function scheduleVolume(command: string, args: Record<string, unknown>): void { if (volumeTimer) window.clearTimeout(volumeTimer); volumeTimer = window.setTimeout(() => void audioCommand(command, args), 80); }

  async function separate(modelId: string): Promise<void> {
    if (!track) return;
    const trackId = track.trackId;
    selectTab('separation'); separating = modelId; separationResult = undefined; separationError = undefined;
    separationStatus = { jobId: 'starting', modelId, stage: 'Preparing local workspace', progress: 0.05, elapsedSeconds: 0, cancelRequested: false };
    try {
      const result = await invoke<SeparationReport>('separate_track', { trackId, modelId });
      // The track may have changed while this job was running (e.g. the user
      // replaced it mid-separation): only apply the result to the tab it
      // belongs to, so a stale job never overwrites the current track's view.
      if (track?.trackId === trackId) separationResult = result;
    } catch (error) {
      if (track?.trackId === trackId) separationError = String(error);
    } finally {
      if (track?.trackId === trackId) { separating = undefined; separationStatus = undefined; }
      await loadModels();
    }
  }
  async function cancelSeparation(): Promise<void> {
    if (!track) return;
    try {
      const accepted = await invoke<boolean>('cancel_separation', { trackId: track.trackId });
      if (accepted) { if (separationStatus) separationStatus = { ...separationStatus, cancelRequested: true }; }
      else separationError = 'The job has not started running yet; try cancelling again in a moment.';
    } catch (error) { separationError = String(error); }
  }

  async function loadCachedRhythm(id: string): Promise<void> { try { rhythm = await invoke<RhythmReport | null>('cached_rhythm', { trackId: id }) ?? undefined; } catch { rhythm = undefined; } }
  async function loadCachedHarmony(id: string): Promise<void> { try { harmony = await invoke<HarmonyReport | null>('cached_harmony', { trackId: id }) ?? undefined; } catch { harmony = undefined; } }
  async function loadCachedAmt(id: string): Promise<void> { try { amt = await invoke<AmtReport | null>('cached_amt', { trackId: id }) ?? undefined; } catch { amt = undefined; } }
  async function loadCachedPitch(id: string, stem: string): Promise<void> { try { pitch = { ...pitch, [stem]: await invoke<PitchReport | null>('cached_pitch', { trackId: id, stem }) ?? undefined }; } catch { pitch = { ...pitch, [stem]: undefined }; } }
  async function analyzeRhythm(): Promise<void> { if (!track) return; analyzingRhythm = true; rhythmError = undefined; try { rhythm = await invoke<RhythmReport>('analyze_rhythm', { trackId: track.trackId }); } catch (error) { rhythmError = String(error); } finally { analyzingRhythm = false; } }
  async function analyzeHarmony(): Promise<void> { if (!track) return; analyzingHarmony = true; harmonyError = undefined; try { harmony = await invoke<HarmonyReport>('analyze_harmony', { trackId: track.trackId }); } catch (error) { harmonyError = String(error); } finally { analyzingHarmony = false; } }
  async function analyzePitch(stem: string): Promise<void> { if (!track) return; analyzingPitch = stem; pitchError = undefined; try { pitch = { ...pitch, [stem]: await invoke<PitchReport>('analyze_pitch', { trackId: track.trackId, stem }) }; } catch (error) { pitchError = String(error); } finally { analyzingPitch = undefined; } }
  async function transcribe(): Promise<void> { if (!track) return; transcribing = true; amtError = undefined; try { amt = await invoke<AmtReport>('transcribe_track', { trackId: track.trackId }); midiWindowStart = 0; } catch (error) { amtError = String(error); } finally { transcribing = false; } }
  async function exportMidi(): Promise<void> {
    if (!track || !amt) return;
    try { const bytes = await invoke<number[]>('export_amt_midi', { trackId: track.trackId }); const link = document.createElement('a'); const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'audio/midi' })); link.href = url; link.download = `${track.originalName.replace(/\.[^.]+$/, '')}-transcription.mid`; link.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000); }
    catch (error) { amtError = String(error); }
  }

  function windowNotes(notes: MidiNote[]): MidiNote[] { const selected = notes.filter((note) => note.end > midiWindowStart && note.start < midiWindowStart + MIDI_WINDOW_SECONDS); const stride = Math.ceil(selected.length / MAX_PIANO_ROLL_NOTES); return stride <= 1 ? selected : selected.filter((_, index) => index % stride === 0); }
  function midiStyle(note: MidiNote): string { const left = ((note.start - midiWindowStart) / MIDI_WINDOW_SECONDS) * 100; const width = Math.max(0.6, ((note.end - note.start) / MIDI_WINDOW_SECONDS) * 100); const bottom = ((Math.max(24, Math.min(96, note.midi)) - 24) / 72) * 100; return `left:${left}%;width:${width}%;bottom:${bottom}%`; }
  function visibleChords(chords: HarmonyReport['chords']): HarmonyReport['chords'] { const stride = Math.ceil(chords.length / MAX_CHORDS); return stride <= 1 ? chords : chords.filter((_, index) => index % stride === 0); }
  function visibleBeats(beats: number[]): number[] { const stride = Math.ceil(beats.length / MAX_BEATS); return stride <= 1 ? beats : beats.filter((_, index) => index % stride === 0); }
  function stemStatus(stem: StemState, stems: StemState[]): string { const hasSolo = stems.some((item) => item.solo); if (stem.muted || (hasSolo && !stem.solo)) return 'silent'; return stem.solo ? 'soloed' : 'active'; }
  function formatTime(value: number): string { const seconds = Math.max(0, Math.floor(value)); return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`; }
  function stepState(index: number): string { const progress = separationStatus?.progress ?? 0; const threshold = separationSteps[index].threshold; const next = separationSteps[index + 1]?.threshold ?? 1; if (progress >= next) return 'done'; return progress >= threshold ? 'active' : ''; }
  function stopMidiPreview(): void { if (midiTimer) window.clearTimeout(midiTimer); for (const oscillator of oscillators) { try { oscillator.stop(); } catch { /* already stopped */ } } oscillators = []; if (midiContext) void midiContext.close(); midiContext = undefined; midiPreviewPlaying = false; }
  function playMidiPreview(): void {
    if (!amt || !track) return;
    stopMidiPreview(); const context = new AudioContext(); midiContext = context; void context.resume(); const end = Math.min(track.durationSeconds, midiWindowStart + MIDI_PREVIEW_SECONDS); const now = context.currentTime + 0.05;
    const candidates = amt.notes.filter((item) => item.end > midiWindowStart && item.start < end);
    const voiceStride = Math.ceil(candidates.length / MAX_PREVIEW_VOICES);
    const voices = voiceStride <= 1 ? candidates : candidates.filter((_, index) => index % voiceStride === 0);
    for (const note of voices) { const oscillator = context.createOscillator(); const gain = context.createGain(); const start = now + Math.max(0, note.start - midiWindowStart); const stop = now + Math.max(0.03, Math.min(end, note.end) - midiWindowStart); const amplitude = 0.025 + note.velocity / 127 * 0.055; oscillator.type = 'triangle'; oscillator.frequency.setValueAtTime(440 * 2 ** ((note.midi - 69) / 12), start); gain.gain.setValueAtTime(0, start); gain.gain.linearRampToValueAtTime(amplitude, start + 0.01); gain.gain.setValueAtTime(amplitude, Math.max(start + 0.01, stop - 0.03)); gain.gain.linearRampToValueAtTime(0, stop); oscillator.connect(gain).connect(context.destination); oscillator.start(start); oscillator.stop(stop + 0.02); oscillators.push(oscillator); }
    midiPreviewPlaying = true; midiTimer = window.setTimeout(stopMidiPreview, (end - midiWindowStart) * 1000 + 150);
  }
  function drawWaveform(): void { const waveform = audio?.waveform; if (!waveformCanvas || !waveform || waveform.max.length === 0) return; const context = waveformCanvas.getContext('2d'); if (!context) return; const { width, height } = waveformCanvas; const middle = height / 2; context.clearRect(0, 0, width, height); context.strokeStyle = '#71e6bd'; context.beginPath(); for (let i = 0; i < waveform.max.length; i += 1) { const x = i / Math.max(1, waveform.max.length - 1) * width; context.moveTo(x, middle - waveform.max[i] * middle); context.lineTo(x, middle - waveform.min[i] * middle); } context.stroke(); }
</script>

<main>
  <header class="app-header"><div class="brand"><b>≋</b><div><p class="eyebrow">LOCAL STEM WORKBENCH</p><h1>Local Music Analyzer</h1></div></div><button class="diagnostic" type="button" disabled={checking} onclick={() => runDoctor(true)}><span class:good={report?.analysisWorker.ok} class:bad={report !== undefined && !report.analysisWorker.ok}>●</span>{checking ? 'Checking engine' : report?.analysisWorker.ok ? 'Engine ready' : 'Check engine'}</button></header>
  {#if report !== undefined && !report.analysisWorker.ok}<p class="error compact">{report.analysisWorker.detail}</p>{/if}

  {#if track !== undefined}
    <section class="track-strip"><div class="track-art">♫</div><div class="track-meta"><p class="eyebrow">CURRENT SOURCE</p><h2>{track.originalName}</h2><p>{formatTime(track.durationSeconds)} · {track.sampleRate / 1000} kHz · {track.channels === 1 ? 'Mono' : 'Stereo'}</p></div><button class="secondary" type="button" disabled={importing} onclick={importTrack}>{importing ? 'Importing…' : 'Replace track'}</button></section>
  {/if}
  {#if track !== undefined || activeTab === 'library'}
    <nav class="tabs" aria-label="Workbench sections">{#each tabs as tab}<button type="button" role="tab" aria-selected={activeTab === tab.id} disabled={track === undefined && tab.id !== 'library'} class:active={activeTab === tab.id} onclick={() => selectTab(tab.id)}><span>{tab.icon}</span>{tab.label}</button>{/each}</nav>
  {/if}

  {#if activeTab === 'library'}
    <section class="panel library">
      <div class="heading"><div><p class="eyebrow">LIBRARY</p><h2>{libraryEntries.length} tracks indexed</h2></div><em>LOCAL INDEX</em></div>
      <div class="actions">
        <button class="primary" type="button" disabled={importing} onclick={importTrack}>{importing ? 'Importing…' : 'Import new track'}</button>
        <button class="ghost" type="button" disabled={libraryBusy} onclick={rebuildLibrary}>{libraryBusy ? 'Rebuilding…' : 'Rebuild index'}</button>
      </div>
      <div class="library-toolbar">
        <input type="search" placeholder="Search name, key or tag…" bind:value={librarySearchText} oninput={scheduleLibrarySearch} />
        <select bind:value={librarySort} onchange={runLibrarySearch}>
          <option value="recent">Recent</option>
          <option value="name">Name</option>
        </select>
      </div>
      {#if libraryTags.length > 0}
        <div class="actions">{#each libraryTags as tagEntry (tagEntry.tag)}<button class="chip" class:active={librarySelectedTags.includes(tagEntry.tag)} type="button" onclick={() => toggleTagFilter(tagEntry.tag)}>{tagEntry.tag} <b>{tagEntry.trackCount}</b></button>{/each}</div>
      {/if}
      {#if libraryEntries.length === 0}
        <div class="empty"><span>▤</span><h3>No tracks indexed yet</h3><p>Import a track to start building your library.</p><button class="primary" type="button" disabled={importing} onclick={importTrack}>Import audio</button></div>
      {:else}
        <div class="library-list">
          {#each libraryEntries as entry (entry.trackId)}
            <article>
              <div class="heading"><strong>{entry.originalName}</strong><small>{formatTime(entry.durationSeconds)}{entry.bpm !== null ? ` · ${entry.bpm.toFixed(1)} BPM` : ''}{entry.keyLabel !== null ? ` · ${entry.keyLabel}` : ''}</small></div>
              <p class="detail">{entry.stemModels.length > 0 ? `Stems: ${entry.stemModels.join(', ')}` : 'No stems yet'}{entry.analyzed.length > 0 ? ` · Analyzed: ${entry.analyzed.join(', ')}` : ''}</p>
              <small>Opened {entry.openCount}× · {entry.lastOpenedAt ?? 'never'}</small>
              <div class="actions">
                {#each entry.tags as tag}<span class="chip">{tag} <button type="button" aria-label={`Remove tag ${tag}`} onclick={() => removeTag(entry, tag)}>×</button></span>{/each}
                <input type="text" placeholder="Add tag" value={libraryTagDraft[entry.trackId] ?? ''} oninput={(event) => { libraryTagDraft = { ...libraryTagDraft, [entry.trackId]: event.currentTarget.value }; }} onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void addTag(entry); } }} />
              </div>
              <button class="primary" type="button" onclick={() => openFromLibrary(entry)}>Open</button>
            </article>
          {/each}
        </div>
      {/if}
      {#if libraryError}<p class="error">{libraryError}</p>{/if}
    </section>
  {:else if track === undefined}
    <section class="welcome"><p class="eyebrow">START A SESSION</p><h2>Bring a song into the workbench.</h2><p>Analyze rhythm and harmony, generate local stems, create a MIDI sketch, and audition results without uploading audio.</p><button class="primary large" type="button" disabled={importing} onclick={importTrack}>{importing ? 'Importing audio…' : 'Open audio file'}</button><button class="ghost" type="button" onclick={() => selectTab('library')}>Browse library</button><small>MP3 · WAV · FLAC · M4A · AAC · OGG · OPUS</small>{#if importError}<p class="error">{importError}</p>{/if}</section>
  {:else}
    {#if activeTab === 'workspace'}
      <section class="panel"><div class="heading"><div><p class="eyebrow">SOURCE TRANSPORT</p><h2>{audio?.stemMix ? 'Stem mix loaded' : 'Original track'}</h2></div><em>{audio?.status ?? 'loading'}</em></div>{#if audio && audio.status !== 'empty'}<canvas bind:this={waveformCanvas} width="900" height="150" aria-label="Waveform overview"></canvas><div class="readout"><strong>{formatTime(audio.currentPositionSeconds)}</strong><span>/ {formatTime(audio.durationSeconds)}</span></div><input type="range" min="0" max={audio.durationSeconds} step="0.01" value={seeking ? seekValue : audio.currentPositionSeconds} oninput={(event) => { seeking = true; seekValue = Number(event.currentTarget.value); }} onchange={(event) => { seeking = false; void audioCommand('seek_audio', { seconds: Number(event.currentTarget.value) }); }} /><div class="actions"><button class="primary" type="button" onclick={() => audioCommand('play_audio')}>▶ Play</button><button class="secondary" type="button" onclick={() => audioCommand('pause_audio')}>Pause</button><button class="secondary" type="button" onclick={() => audioCommand('stop_audio')}>Stop</button><button class="ghost" type="button" onclick={() => audioCommand('reopen_audio_device')}>Reconnect output</button></div><label class="slider-label">Master output<input type="range" min="0" max="2" step="0.01" value={audio.volume} oninput={(event) => scheduleVolume('set_master_volume', { volume: Number(event.currentTarget.value) })} /></label>{/if}{#if audioError}<p class="error">{audioError}</p>{/if}</section>
      <section class="quick-grid"><button type="button" onclick={() => selectTab('separation')}><span>✦</span><strong>Generate stems</strong><small>Demucs locally</small></button><button type="button" onclick={() => selectTab('transcription')}><span>♫</span><strong>Make MIDI sketch</strong><small>Basic Pitch</small></button><button type="button" onclick={() => selectTab('analysis')}><span>⌁</span><strong>Analyze song</strong><small>BPM, key, chords</small></button></section>
    {:else if activeTab === 'separation'}
      <section class="panel"><div class="heading"><div><p class="eyebrow">STEM SEPARATION</p><h2>Split the source into playable parts.</h2></div><em>LOCAL ONLY</em></div>{#if separating || separationStatus}<div class="progress" role="status"><div class="heading"><div><p class="eyebrow">{separationStatus?.modelId ?? separating}</p><h3>{separationStatus?.cancelRequested ? 'Cancellation requested' : separationStatus?.stage ?? 'Starting separation'}</h3></div><strong>{formatTime(separationStatus?.elapsedSeconds ?? 0)}</strong></div><div class="progress-bar"><span class:working={!separationStatus?.cancelRequested} style={`width:${Math.max(5, (separationStatus?.progress ?? 0.05) * 100)}%`}></span></div><p>Demucs is processing locally. CPU jobs can take several minutes; the bar advances at verified worker checkpoints.</p><ol>{#each separationSteps as step, index}<li class={stepState(index)}><i></i>{step.label}</li>{/each}</ol><button class="danger" type="button" disabled={separationStatus?.cancelRequested === true} onclick={cancelSeparation}>{separationStatus?.cancelRequested ? 'Cancelling after model step…' : 'Cancel safely'}</button></div>{:else}<div class="model-grid">{#each models as model}<article class:unavailable={!model.installed}><div><span>✦</span><i class:ready={model.installed}></i></div><h3>{model.label}</h3><p>{model.stems.join(' · ')}</p><small>{model.detail}</small><button class:primary={model.installed} class="secondary" type="button" disabled={!model.installed} onclick={() => separate(model.id)}>{model.installed ? 'Separate track' : 'Bundle required'}</button></article>{/each}</div><details><summary>Need to install Demucs locally?</summary><p>Models are never downloaded while the desktop app is running.</p><code>powershell -ExecutionPolicy Bypass -File .\scripts\install-demucs.ps1 -Model demucs-4</code></details>{/if}{#if separationResult}<div class="success"><b>✓</b><div><strong>{separationResult.cacheHit ? 'Using cached stems' : 'Stems are ready'}</strong><p>{separationResult.stems.join(', ')} · {separationResult.progressEvents} worker checkpoints</p></div><button class="primary" type="button" onclick={() => loadStemMix(separationResult?.modelId ?? '')}>Open mixer</button></div>{/if}{#if separationError}<p class="error">{separationError}</p>{/if}</section>
    {:else if activeTab === 'transcription'}
      <section class="panel"><div class="heading"><div><p class="eyebrow">POLYPHONIC TRANSCRIPTION</p><h2>Turn the source into a MIDI sketch.</h2></div><em>{amt?.cacheHit ? 'cached' : amt?.engine ?? 'not generated'}</em></div>{#if !amt}<div class="empty"><span>♫</span><h3>No MIDI sketch yet</h3><p>Basic Pitch generates local note data and a MIDI artifact from the original track.</p><button class="primary" type="button" disabled={transcribing} onclick={transcribe}>{transcribing ? 'Transcribing locally…' : 'Transcribe original to MIDI'}</button></div>{:else}<div class="midi-toolbar"><div><strong>{amt.notes.length}</strong><span>notes detected</span></div><div class="actions"><button class="primary" type="button" disabled={midiPreviewPlaying} onclick={playMidiPreview}>▶ Preview {MIDI_PREVIEW_SECONDS}s</button><button class="secondary" type="button" disabled={!midiPreviewPlaying} onclick={stopMidiPreview}>Stop preview</button><button class="secondary" type="button" onclick={exportMidi}>Export .mid</button></div></div><div class="roll-title"><span>PIANO ROLL · SYNTH PREVIEW</span><strong>{formatTime(midiWindowStart)} — {formatTime(Math.min(track.durationSeconds, midiWindowStart + MIDI_WINDOW_SECONDS))}</strong></div><div class="piano-roll" aria-label={`${windowNotes(amt.notes).length} MIDI notes visible`}>{#each windowNotes(amt.notes) as note}<span title={`MIDI ${note.midi} · velocity ${note.velocity}`} style={midiStyle(note)}></span>{/each}</div><label class="slider-label">Navigate MIDI<input type="range" min="0" max={Math.max(0, track.durationSeconds - MIDI_WINDOW_SECONDS)} step="0.25" bind:value={midiWindowStart} /></label><p class="detail">The preview is a local synthesized interpretation of the detected notes. Export MIDI to continue editing in a DAW.</p>{/if}{#if amtError}<p class="error">{amtError}</p>{/if}</section>
    {:else if activeTab === 'analysis'}
      <section class="analysis-grid"><article class="panel"><p class="eyebrow">RHYTHM</p><h2>{rhythm ? `${rhythm.bpm.toFixed(1)} BPM` : 'Tempo & beats'}</h2><p>{rhythm ? `${Math.round(rhythm.stabilityScore * 100)}% beat stability · ${rhythm.beatTimes.length} beats` : 'Find tempo and a beat grid from the normalized source.'}</p><button class="primary" type="button" disabled={analyzingRhythm} onclick={analyzeRhythm}>{analyzingRhythm ? 'Analyzing…' : rhythm ? 'Reanalyze rhythm' : 'Analyze rhythm'}</button>{#if rhythm}<div class="beats" aria-label={`${rhythm.beatTimes.length} beats detected`}>{#each visibleBeats(rhythm.beatTimes) as beat}<i style={`left:${beat / Math.max(track.durationSeconds, 0.01) * 100}%`}></i>{/each}</div>{/if}{#if rhythmError}<p class="error">{rhythmError}</p>{/if}</article><article class="panel"><p class="eyebrow">HARMONY</p><h2>{harmony ? harmony.key.label : 'Key & chords'}</h2><p>{harmony ? `${harmony.chords.length} chord segments · ${harmony.algorithm}` : 'Estimate key and triad changes, aligned to the beat grid when available.'}</p><button class="primary" type="button" disabled={analyzingHarmony} onclick={analyzeHarmony}>{analyzingHarmony ? 'Analyzing…' : harmony ? 'Reanalyze harmony' : 'Analyze harmony'}</button>{#if harmony}<div class="chords">{#each visibleChords(harmony.chords) as chord}<span style={`left:${chord.start / Math.max(track.durationSeconds, 0.01) * 100}%;width:${(chord.end - chord.start) / Math.max(track.durationSeconds, 0.01) * 100}%`}>{chord.label}</span>{/each}</div>{/if}{#if harmonyError}<p class="error">{harmonyError}</p>{/if}</article><article class="panel pitch"><p class="eyebrow">STEM PITCH</p><h2>Bass & vocal notes</h2><p>Use separated bass or vocal stems for pYIN note segmentation.</p><div class="actions">{#each ['bass', 'vocals'] as stem}<button class="secondary" type="button" disabled={analyzingPitch !== undefined} onclick={() => analyzePitch(stem)}>{analyzingPitch === stem ? `Analyzing ${stem}…` : `Analyze ${stem}`}</button>{/each}</div>{#each Object.entries(pitch) as [stem, result]}{#if result}<span class="chip"><b>{stem}</b> {result.notes.length} notes · {result.engine}</span>{/if}{/each}{#if pitchError}<p class="error">{pitchError}</p>{/if}</article></section>
    {:else}
      <section class="panel"><div class="heading"><div><p class="eyebrow">STEM MIXER</p><h2>{audio?.stemMix ? 'Balance your separated stems.' : 'Load generated stems to start mixing.'}</h2></div><em>{audio?.stemMix ? `${audio.stems.length} channels` : 'waiting for stems'}</em></div>{#if audio?.stemMix}<canvas bind:this={waveformCanvas} width="900" height="120"></canvas><div class="actions"><button class="primary" type="button" onclick={() => audioCommand('play_audio')}>▶ Play mix</button><button class="secondary" type="button" onclick={() => audioCommand('pause_audio')}>Pause</button><button class="secondary" type="button" onclick={() => audioCommand('stop_audio')}>Stop</button></div><div class="mixer-list">{#each audio.stems as stem}<div><section><strong>{stem.stem}</strong><small>{stemStatus(stem, audio.stems)}</small></section><button class:active={stem.muted} aria-pressed={stem.muted} type="button" onclick={() => audioCommand('set_stem_muted', { stem: stem.stem, muted: !stem.muted })}>Mute</button><button class:active={stem.solo} aria-pressed={stem.solo} type="button" onclick={() => audioCommand('set_stem_solo', { stem: stem.stem, solo: !stem.solo })}>Solo</button><input aria-label={`${stem.stem} volume`} type="range" min="0" max="2" step="0.01" value={stem.volume} oninput={(event) => scheduleVolume('set_stem_volume', { stem: stem.stem, volume: Number(event.currentTarget.value) })} /></div>{/each}</div>{:else}<div class="empty"><span>≋</span><h3>Your mix will appear here</h3><p>Generate a stem set in the Stems tab, then choose “Open mixer”.</p><button class="primary" type="button" onclick={() => selectTab('separation')}>Go to stems</button></div>{/if}{#if audioError}<p class="error">{audioError}</p>{/if}</section>
    {/if}
  {/if}
</main>
