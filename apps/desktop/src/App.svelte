<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type ComponentStatus = {
    ok: boolean;
    detail: string;
  };

  type DoctorReport = {
    desktopCore: ComponentStatus;
    analysisWorker: ComponentStatus;
    protocolVersion: number;
  };

  type IngestedTrack = {
    trackId: string;
    originalName: string;
    durationSeconds: number;
    sampleRate: number;
    channels: number;
    workspacePath: string;
  };

  let report: DoctorReport | undefined;
  let track: IngestedTrack | undefined;
  let importError: string | undefined;
  let checking = true;
  let importing = false;

  async function runDoctor(restart = false): Promise<void> {
    checking = true;
    try {
      report = await invoke<DoctorReport>(restart ? 'restart_worker' : 'doctor');
    } catch (error) {
      report = {
        desktopCore: { ok: true, detail: 'Rust host is running' },
        analysisWorker: { ok: false, detail: String(error) },
        protocolVersion: 1
      };
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    void runDoctor();
  });

  async function pickAndImport(): Promise<void> {
    importing = true;
    importError = undefined;
    try {
      const imported = await invoke<IngestedTrack | null>('pick_and_import');
      if (imported !== null) {
        track = imported;
      }
    } catch (error) {
      importError = String(error);
    } finally {
      importing = false;
    }
  }
</script>

<main>
  <h1>Local Music Analyzer</h1>
  <p class="subtitle">Phase 1 — local audio ingestion</p>

  {#if checking && report === undefined}
    <p role="status">Checking desktop services…</p>
  {:else if report !== undefined}
    <section aria-label="Desktop diagnostics">
      <p><span class:good={report.desktopCore.ok} class:bad={!report.desktopCore.ok}>●</span> Desktop core: {report.desktopCore.ok ? 'OK' : 'ERROR'}</p>
      <p><span class:good={report.analysisWorker.ok} class:bad={!report.analysisWorker.ok}>●</span> Analysis worker: {report.analysisWorker.ok ? 'OK' : 'ERROR'}</p>
      <p>Protocol: v{report.protocolVersion}</p>
      <p class="detail">{report.analysisWorker.detail}</p>
    </section>
  {/if}

  <button type="button" disabled={checking} onclick={() => runDoctor(true)}>
    {checking ? 'Checking…' : 'Restart worker'}
  </button>

  <section class="import" aria-label="Audio import">
    <h2>Import audio</h2>
    <button type="button" disabled={importing} onclick={pickAndImport}>
      {importing ? 'Importing…' : 'Open audio file'}
    </button>
    {#if track !== undefined}
      <p class="detail">
        Imported <strong>{track.originalName}</strong> — {track.durationSeconds.toFixed(2)} s,
        {track.sampleRate} Hz, {track.channels} channels.
      </p>
    {:else if importError !== undefined}
      <p class="error" role="alert">{importError}</p>
    {/if}
  </section>
</main>
