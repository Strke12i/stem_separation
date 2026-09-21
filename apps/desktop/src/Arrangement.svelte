<script lang="ts">
  // DAW-style arrangement for a stem mix: one lane per stem on a shared, zoomable
  // timeline with the beat/bar grid, each lane's waveform, the notes and chords found
  // in it, and what is sounding under the playhead. Clicking or dragging anywhere on
  // the timeline moves the playhead. All maths lives in arrangement.ts.
  import { onMount, tick } from 'svelte';
  import {
    activeAt, buildGrid, clamp, clampZoom, fitZoom, followScroll, formatBarBeat, formatClock, gridPosition,
    indexIntervals, labelStride, laneCapabilities, noteRow, notesFromAmt, notesFromPitch, overlapping,
    pitchRange, projectPosition, stepSeconds, stepTarget, timeTicks, waveformColumns, xToTime, zoomAt,
    MAX_PIXELS_PER_SECOND, MIN_PIXELS_PER_SECOND,
    type Interval, type TimedChord, type Waveform
  } from './arrangement';

  type StemState = { stem: string; volume: number; muted: boolean; solo: boolean };
  type AudioLike = { status: string; currentPositionSeconds: number; durationSeconds: number; stems: StemState[] };
  type PitchNote = { start: number; end: number; midi: number; note: string; confidence: number };
  type AmtNote = { start: number; end: number; midi: number; velocity: number };
  type ChordLike = Interval & { label: string };
  type AnalyzeKind = 'rhythm' | 'mix-chords' | 'notes' | 'chords';

  let {
    durationSeconds, audio, waveforms, rhythm, mixHarmony, pitch, stemAmt, stemHarmony, busy, errors, batch,
    onCommand, onVolume, onAnalyze, onAnalyzeAll
  }: {
    durationSeconds: number;
    audio: AudioLike;
    waveforms: { stem: string; waveform: Waveform }[];
    rhythm: { bpm: number; beatTimes: number[] } | undefined;
    mixHarmony: { chords: ChordLike[] } | undefined;
    pitch: Record<string, { notes: PitchNote[] } | undefined>;
    stemAmt: Record<string, { notes: AmtNote[] } | undefined>;
    stemHarmony: Record<string, { chords: ChordLike[] } | undefined>;
    busy: Record<string, boolean | undefined>;
    errors: Record<string, string | undefined>;
    batch: { done: number; total: number } | undefined;
    onCommand: (command: string, args?: Record<string, unknown>) => Promise<void>;
    onVolume: (command: string, args: Record<string, unknown>) => void;
    onAnalyze: (kind: AnalyzeKind, stem?: string) => void;
    onAnalyzeAll: () => void;
  } = $props();

  const RULER_HEIGHT = 34;
  const MIX_CHORD_HEIGHT = 30;
  const LANE_HEIGHT = 112;
  const CHORD_STRIP_HEIGHT = 20;
  const COLORS: Record<string, string> = { vocals: '#f59ac2', drums: '#f5c15b', bass: '#5cb8ff', other: '#7ae8bf', guitar: '#ff9b6b', piano: '#b39dff' };
  const colorOf = (stem: string): string => COLORS[stem] ?? '#9db2a9';

  let pixelsPerSecond = $state(48);
  let scrollLeft = $state(0);
  let viewportWidth = $state(800);
  let beatsPerBar = $state(4);
  let downbeatOffset = $state(0);
  let follow = $state(true);
  let selectedStem = $state<string | undefined>(undefined);
  let playhead = $state(0);
  let hoverTime = $state<number | undefined>(undefined);
  // Plain (non-reactive) on purpose: it must not re-run the effects that read it.
  const drag = { active: false };
  let lastSeekAt = 0;
  let lastSentSeconds = -1;
  let polledBase = 0;
  let polledAt = 0;
  let frame = 0;
  let scroller: HTMLDivElement | undefined;
  let baseCanvas: HTMLCanvasElement | undefined;
  let overlayCanvas: HTMLCanvasElement | undefined;

  const playing = $derived(audio.status === 'playing');
  const duration = $derived(Math.max(durationSeconds, audio.durationSeconds, 0.001));
  const stemKey = $derived(audio.stems.map((stem) => stem.stem).join('|'));
  const dimKey = $derived(audio.stems.map((stem) => (stem.muted ? 'm' : stem.solo ? 's' : '-')).join(''));
  const grid = $derived(rhythm ? buildGrid(rhythm.beatTimes, beatsPerBar, downbeatOffset) : []);
  const totalWidth = $derived(Math.ceil(duration * pixelsPerSecond));
  const totalHeight = $derived(RULER_HEIGHT + MIX_CHORD_HEIGHT + stemKey.split('|').filter(Boolean).length * LANE_HEIGHT);
  const mixChords = $derived(indexIntervals<TimedChord>(mixHarmony?.chords ?? []));

  // Rebuilt only when the reports or the stem set change, never on a position poll.
  const lanes = $derived(
    stemKey.split('|').filter(Boolean).map((stem) => {
      const capabilities = laneCapabilities(stem);
      const notes = capabilities.notes === 'pitch' ? notesFromPitch(pitch[stem]?.notes ?? [])
        : capabilities.notes === 'amt' ? notesFromAmt(stemAmt[stem]?.notes ?? []) : [];
      const chords = capabilities.chords ? (stemHarmony[stem]?.chords ?? []) : [];
      return {
        stem,
        capabilities,
        noteCount: notes.length,
        chordCount: chords.length,
        hasNotes: capabilities.notes === 'pitch' ? pitch[stem] !== undefined : capabilities.notes === 'amt' ? stemAmt[stem] !== undefined : false,
        hasChords: capabilities.chords && stemHarmony[stem] !== undefined,
        noteIndex: indexIntervals(notes),
        chordIndex: indexIntervals<TimedChord>(chords),
        range: pitchRange(notes),
        waveform: waveforms.find((entry) => entry.stem === stem)?.waveform
      };
    })
  );

  const position = $derived(formatBarBeat(gridPosition(grid, playhead)));
  const now = $derived({
    lanes: lanes.map((lane) => ({
      notes: [...new Set(activeAt(lane.noteIndex, playhead).map((note) => note.label))].join(' · '),
      chord: activeAt(lane.chordIndex, playhead)[0]?.label ?? ''
    })),
    mixChord: activeAt(mixChords, playhead)[0]?.label ?? ''
  });
  const anyBusy = $derived(Object.values(busy).some(Boolean));

  function command(name: string, args: Record<string, unknown> = {}): void { void onCommand(name, args); }
  function seek(seconds: number): void { playhead = clamp(seconds, 0, duration); lastSentSeconds = playhead; command('seek_audio', { seconds: playhead }); }
  function togglePlay(): void {
    if (playing) command('pause_audio');
    else if (audio.status === 'ended') { command('seek_audio', { seconds: 0 }); command('play_audio'); }
    else command('play_audio');
  }
  function step(direction: -1 | 1, unit: 'bar' | 'beat'): void {
    const target = stepTarget(grid, playhead, direction, unit);
    if (target !== null) seek(target);
    else if (grid.length === 0) seek(stepSeconds(playhead, direction, unit === 'bar' ? 5 : 1, duration));
    else seek(direction > 0 ? duration : 0);
  }
  function toggleMute(stem: StemState): void { command('set_stem_muted', { stem: stem.stem, muted: !stem.muted }); }
  function toggleSolo(stem: StemState): void { command('set_stem_solo', { stem: stem.stem, solo: !stem.solo }); }

  // ---- Playhead ----------------------------------------------------------------
  // The engine is polled a few times a second; between polls the playhead is
  // extrapolated so it moves smoothly, and every poll re-anchors it.
  $effect(() => {
    polledBase = audio.currentPositionSeconds;
    polledAt = performance.now();
    if (!drag.active) playhead = polledBase;
  });

  $effect(() => {
    if (!playing) return;
    const tickFrame = (time: number): void => {
      if (!drag.active) playhead = projectPosition(polledBase, polledAt, time, true, duration);
      if (follow && !drag.active && scroller) {
        const target = followScroll(playhead * pixelsPerSecond, scroller.scrollLeft, viewportWidth);
        if (target !== null) scroller.scrollLeft = target;
      }
      frame = requestAnimationFrame(tickFrame);
    };
    frame = requestAnimationFrame(tickFrame);
    return () => cancelAnimationFrame(frame);
  });

  // ---- Mouse: click or drag anywhere on the timeline to move the playhead -------
  function timeAt(event: PointerEvent): number {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    return xToTime(event.clientX - box.left + scrollLeft, pixelsPerSecond, duration);
  }
  function onPointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).focus();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag.active = true;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const row = Math.floor((event.clientY - box.top - RULER_HEIGHT - MIX_CHORD_HEIGHT) / LANE_HEIGHT);
    if (row >= 0 && row < lanes.length) selectedStem = lanes[row].stem;
    seek(timeAt(event));
    lastSeekAt = performance.now();
  }
  function onPointerMove(event: PointerEvent): void {
    const time = timeAt(event);
    hoverTime = time;
    if (!drag.active) return;
    playhead = time;
    const nowMs = performance.now();
    if (nowMs - lastSeekAt > 120) { lastSeekAt = nowMs; lastSentSeconds = time; command('seek_audio', { seconds: time }); }
  }
  function onPointerUp(event: PointerEvent): void {
    if (!drag.active) return;
    drag.active = false;
    // The press already sent this position for a plain click; only send a real move.
    const time = timeAt(event);
    if (Math.abs(time - lastSentSeconds) > 0.02) seek(time);
  }
  function onWheel(event: WheelEvent): void {
    if (!event.ctrlKey && !event.metaKey) return;
    event.preventDefault();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    void applyZoom(event.deltaY < 0 ? 1.2 : 1 / 1.2, event.clientX - box.left);
  }
  async function applyZoom(factor: number, anchorX = viewportWidth / 2): Promise<void> {
    const next = zoomAt({ pixelsPerSecond, scrollLeft }, factor, anchorX);
    pixelsPerSecond = next.pixelsPerSecond;
    await tick();
    if (scroller) scroller.scrollLeft = next.scrollLeft;
  }
  async function fit(): Promise<void> {
    pixelsPerSecond = fitZoom(duration, viewportWidth);
    await tick();
    if (scroller) scroller.scrollLeft = 0;
  }

  function onKeyDown(event: KeyboardEvent): void {
    const target = event.target as HTMLElement;
    if (target.closest('input, select, textarea, button')) return;
    const unit = event.shiftKey ? 'bar' : 'beat';
    const handled = ((): boolean => {
      switch (event.key) {
        case ' ': togglePlay(); return true;
        case 'ArrowLeft': step(-1, unit); return true;
        case 'ArrowRight': step(1, unit); return true;
        case 'Home': seek(0); return true;
        case 'End': seek(duration); return true;
        case '+': case '=': void applyZoom(1.25); return true;
        case '-': void applyZoom(0.8); return true;
        case 'f': case 'F': follow = !follow; return true;
        case 'm': case 'M': { const stem = audio.stems.find((item) => item.stem === selectedStem); if (stem) toggleMute(stem); return stem !== undefined; }
        case 's': case 'S': { const stem = audio.stems.find((item) => item.stem === selectedStem); if (stem) toggleSolo(stem); return stem !== undefined; }
        default: return false;
      }
    })();
    if (handled) event.preventDefault();
  }

  // ---- Drawing ------------------------------------------------------------------
  function prepare(canvas: HTMLCanvasElement | undefined): CanvasRenderingContext2D | undefined {
    if (!canvas) return undefined;
    const ratio = window.devicePixelRatio || 1;
    const width = Math.round(viewportWidth * ratio);
    const height = Math.round(totalHeight * ratio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const context = canvas.getContext('2d') ?? undefined;
    if (!context) return undefined;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, viewportWidth, totalHeight);
    return context;
  }

  function drawBase(): void {
    const context = prepare(baseCanvas);
    if (!context) return;
    const width = viewportWidth;
    const from = scrollLeft / pixelsPerSecond;
    const to = (scrollLeft + width) / pixelsPerSecond;
    const x = (time: number): number => time * pixelsPerSecond - scrollLeft;
    const lanesTop = RULER_HEIGHT + MIX_CHORD_HEIGHT;
    context.textBaseline = 'middle';

    // Ruler
    context.fillStyle = '#0b1c17';
    context.fillRect(0, 0, width, RULER_HEIGHT);
    context.font = '600 10px Inter, system-ui, sans-serif';
    if (grid.length > 0) {
      const barSeconds = rhythm ? (60 / Math.max(rhythm.bpm, 1)) * beatsPerBar : 2;
      const stride = labelStride(barSeconds * pixelsPerSecond);
      for (const line of grid) {
        if (line.time < from - 1 || line.time > to + 1) continue;
        const lineX = Math.round(x(line.time)) + 0.5;
        context.strokeStyle = line.isBar ? '#4d8a72' : '#28483c';
        context.beginPath();
        context.moveTo(lineX, line.isBar ? 4 : RULER_HEIGHT - 9);
        context.lineTo(lineX, RULER_HEIGHT);
        context.stroke();
        if (line.isBar && (line.bar - 1) % stride === 0) { context.fillStyle = '#9fd9c0'; context.fillText(String(line.bar), lineX + 4, 11); }
      }
    } else {
      context.fillStyle = '#9db2a9';
      for (const tick of timeTicks(duration, pixelsPerSecond)) {
        if (tick < from - 5 || tick > to + 5) continue;
        const tickX = Math.round(x(tick)) + 0.5;
        context.strokeStyle = '#4d8a72';
        context.beginPath(); context.moveTo(tickX, RULER_HEIGHT - 10); context.lineTo(tickX, RULER_HEIGHT); context.stroke();
        context.fillText(formatClock(tick).replace(/\.0$/, ''), tickX + 4, 12);
      }
    }

    // Whole-mix chords
    context.fillStyle = '#0c1f19';
    context.fillRect(0, RULER_HEIGHT, width, MIX_CHORD_HEIGHT);
    context.font = '700 11px Inter, system-ui, sans-serif';
    for (const chord of overlapping(mixChords, from, to)) drawChord(context, chord, x(chord.start), x(chord.end), RULER_HEIGHT + 4, MIX_CHORD_HEIGHT - 8, '#7ae8bf');

    // Lanes
    const dim = dimKey;
    const someSolo = dim.includes('s');
    lanes.forEach((lane, index) => {
      const top = lanesTop + index * LANE_HEIGHT;
      const silent = dim[index] === 'm' || (someSolo && dim[index] !== 's');
      const color = colorOf(lane.stem);
      context.globalAlpha = silent ? 0.35 : 1;
      context.fillStyle = index % 2 === 0 ? '#0a1a15' : '#0c1f19';
      context.fillRect(0, top, width, LANE_HEIGHT);
      const stripHeight = lane.capabilities.chords ? CHORD_STRIP_HEIGHT : 0;
      const bodyTop = top + stripHeight;
      const bodyHeight = LANE_HEIGHT - stripHeight;

      if (lane.waveform) {
        const columns = waveformColumns(lane.waveform, duration, scrollLeft / pixelsPerSecond, pixelsPerSecond, Math.ceil(width));
        const middle = bodyTop + bodyHeight / 2;
        const scale = bodyHeight / 2 - 3;
        context.fillStyle = color + '3a';
        for (let column = 0; column < columns.max.length; column += 1) {
          const highY = middle - columns.max[column] * scale;
          const lowY = middle - columns.min[column] * scale;
          if (lowY - highY > 0.2) context.fillRect(column, highY, 1, lowY - highY);
        }
      }
      drawGridLines(context, from, to, top, LANE_HEIGHT);

      for (const chord of overlapping(lane.chordIndex, from, to)) drawChord(context, chord, x(chord.start), x(chord.end), top + 2, stripHeight - 4, color);
      for (const note of overlapping(lane.noteIndex, from, to)) {
        const row = noteRow(note.midi, lane.range, bodyHeight - 6);
        const left = x(note.start);
        const noteWidth = Math.max(2, (note.end - note.start) * pixelsPerSecond);
        context.fillStyle = color + Math.round(96 + 159 * note.strength).toString(16).padStart(2, '0');
        context.fillRect(left, bodyTop + 3 + row.y, noteWidth, Math.max(2, row.height - 1));
        if (noteWidth > 26 && row.height >= 9) {
          context.fillStyle = '#04100c';
          context.font = '700 9px Inter, system-ui, sans-serif';
          context.fillText(note.label, left + 3, bodyTop + 3 + row.y + row.height / 2);
        }
      }
      context.globalAlpha = 1;

      let hint = '';
      if (lane.capabilities.notes === null) hint = 'Percussion — no pitch to detect';
      else if (!lane.hasNotes) hint = 'No notes yet — use “Detect notes”';
      else if (lane.noteCount === 0) hint = 'No notes detected in this stem';
      if (hint) {
        context.fillStyle = '#7f9a90';
        context.font = '600 11px Inter, system-ui, sans-serif';
        context.fillText(hint, 12, bodyTop + bodyHeight / 2);
      }
      context.strokeStyle = lane.stem === selectedStem ? '#77b99e' : '#1c3a30';
      context.lineWidth = lane.stem === selectedStem ? 1.5 : 1;
      context.strokeRect(0.5, top + 0.5, width - 1, LANE_HEIGHT - 1);
      context.lineWidth = 1;
    });
  }

  function drawGridLines(context: CanvasRenderingContext2D, from: number, to: number, top: number, height: number): void {
    for (const line of grid) {
      if (line.time < from - 1 || line.time > to + 1) continue;
      const lineX = Math.round(line.time * pixelsPerSecond - scrollLeft) + 0.5;
      context.strokeStyle = line.isBar ? 'rgba(122,232,191,0.26)' : 'rgba(122,232,191,0.09)';
      context.beginPath(); context.moveTo(lineX, top); context.lineTo(lineX, top + height); context.stroke();
    }
  }

  function drawChord(context: CanvasRenderingContext2D, chord: TimedChord, left: number, right: number, top: number, height: number, color: string): void {
    if (height <= 0) return;
    const chordWidth = Math.max(2, right - left - 1);
    context.fillStyle = color + '33';
    context.fillRect(left, top, chordWidth, height);
    context.fillStyle = color;
    context.fillRect(left, top, 2, height);
    if (chord.label !== 'N' && chordWidth > 22) {
      context.font = '700 11px Inter, system-ui, sans-serif';
      context.fillStyle = '#e8fff6';
      context.fillText(chord.label, left + 6, top + height / 2);
    }
  }

  // The overlay is cheap and redrawn every frame the playhead moves: what is sounding
  // is outlined so the note or chord under the playhead is easy to spot.
  function drawOverlay(): void {
    const context = prepare(overlayCanvas);
    if (!context) return;
    const x = (time: number): number => time * pixelsPerSecond - scrollLeft;
    const lanesTop = RULER_HEIGHT + MIX_CHORD_HEIGHT;
    context.lineWidth = 2;
    context.strokeStyle = '#ffffff';
    for (const chord of activeAt(mixChords, playhead)) {
      context.strokeRect(x(chord.start) + 0.5, RULER_HEIGHT + 4, Math.max(2, x(chord.end) - x(chord.start) - 1), MIX_CHORD_HEIGHT - 8);
    }
    lanes.forEach((lane, index) => {
      const top = lanesTop + index * LANE_HEIGHT;
      const stripHeight = lane.capabilities.chords ? CHORD_STRIP_HEIGHT : 0;
      for (const chord of activeAt(lane.chordIndex, playhead)) {
        context.strokeRect(x(chord.start) + 0.5, top + 2, Math.max(2, x(chord.end) - x(chord.start) - 1), stripHeight - 4);
      }
      for (const note of activeAt(lane.noteIndex, playhead)) {
        const row = noteRow(note.midi, lane.range, LANE_HEIGHT - stripHeight - 6);
        context.strokeRect(x(note.start) + 0.5, top + stripHeight + 3 + row.y, Math.max(2, (note.end - note.start) * pixelsPerSecond), Math.max(2, row.height - 1));
      }
    });
    context.lineWidth = 1;
  }

  $effect(() => { drawBase(); });
  $effect(() => { drawOverlay(); });

  onMount(() => {
    if (!scroller) return;
    const observer = new ResizeObserver(() => { if (scroller) viewportWidth = Math.max(200, scroller.clientWidth); });
    observer.observe(scroller);
    viewportWidth = Math.max(200, scroller.clientWidth);
    return () => observer.disconnect();
  });

  const playheadX = $derived(playhead * pixelsPerSecond - scrollLeft);
  const hoverX = $derived(hoverTime === undefined ? undefined : hoverTime * pixelsPerSecond - scrollLeft);
  const zoomValue = $derived(Math.log(pixelsPerSecond));
  const laneName = (stem: string): string => stem.charAt(0).toUpperCase() + stem.slice(1);
</script>

<div class="arr" role="group" aria-label="Arrangement">
  <div class="arr-transport">
    <div class="arr-buttons" role="group" aria-label="Transport">
      <button type="button" title="Go to start (Home)" aria-label="Go to start" onclick={() => seek(0)}>⏮</button>
      <button type="button" title="Previous bar (Shift+Left)" aria-label="Previous bar" onclick={() => step(-1, 'bar')}>⏪</button>
      <button type="button" title="Previous beat (Left)" aria-label="Previous beat" onclick={() => step(-1, 'beat')}>◀</button>
      <button type="button" class="arr-play" title="Play / pause (Space)" aria-label={playing ? 'Pause' : 'Play'} aria-pressed={playing} onclick={togglePlay}>{playing ? '⏸' : '▶'}</button>
      <button type="button" title="Stop (back to start)" aria-label="Stop" onclick={() => command('stop_audio')}>⏹</button>
      <button type="button" title="Next beat (Right)" aria-label="Next beat" onclick={() => step(1, 'beat')}>▶</button>
      <button type="button" title="Next bar (Shift+Right)" aria-label="Next bar" onclick={() => step(1, 'bar')}>⏩</button>
    </div>
    <div class="arr-readout" aria-live="off">
      <strong>{formatClock(playhead)}</strong><span> / {formatClock(duration)}</span>
      <em>{position}</em>
      {#if rhythm}<span class="arr-bpm">{rhythm.bpm.toFixed(1)} BPM</span>{/if}
    </div>
    <div class="arr-controls">
      <label title="Downbeats are not detected: choose how beats group into bars">Beats/bar
        <select bind:value={beatsPerBar} onchange={() => { downbeatOffset = Math.min(downbeatOffset, beatsPerBar - 1); }}>
          {#each [2, 3, 4, 5, 6, 7, 8] as count}<option value={count}>{count}</option>{/each}
        </select>
      </label>
      <label title="Which detected beat starts the first bar">Bar 1 on beat
        <select bind:value={downbeatOffset}>
          {#each Array.from({ length: beatsPerBar }, (_, index) => index) as offset}<option value={offset}>{offset + 1}</option>{/each}
        </select>
      </label>
      <span class="arr-zoom">
        <button type="button" title="Zoom out (-)" aria-label="Zoom out" onclick={() => applyZoom(0.8)}>−</button>
        <input type="range" aria-label="Zoom" min={Math.log(MIN_PIXELS_PER_SECOND)} max={Math.log(MAX_PIXELS_PER_SECOND)} step="0.01" value={zoomValue}
          oninput={(event) => { pixelsPerSecond = clampZoom(Math.exp(Number(event.currentTarget.value))); }} />
        <button type="button" title="Zoom in (+)" aria-label="Zoom in" onclick={() => applyZoom(1.25)}>+</button>
        <button type="button" title="Fit the whole song" onclick={fit}>Fit</button>
      </span>
      <label class="arr-follow" title="Keep the playhead in view (F)"><input type="checkbox" bind:checked={follow} /> Follow</label>
      <button type="button" class="arr-analyze" disabled={anyBusy || batch !== undefined} onclick={onAnalyzeAll}>
        {batch ? `Analyzing ${batch.done}/${batch.total}…` : 'Analyze all lanes'}
      </button>
    </div>
  </div>

  {#if !rhythm}
    <p class="arr-banner">No beat grid yet — the ruler shows seconds. <button type="button" disabled={busy['rhythm']} onclick={() => onAnalyze('rhythm')}>{busy['rhythm'] ? 'Analyzing…' : 'Analyze rhythm'}</button>{#if errors['rhythm']}<span class="error"> {errors['rhythm']}</span>{/if}</p>
  {/if}

  <div class="arr-body">
    <div class="arr-heads" style:width="13rem">
      <div class="arr-corner" style:height="{RULER_HEIGHT}px"><small>bar</small></div>
      <div class="arr-head arr-mixhead" style:height="{MIX_CHORD_HEIGHT}px">
        <span>Mix chords</span>
        {#if mixHarmony}<b>{now.mixChord || '—'}</b>
        {:else}<button type="button" disabled={busy['mix-chords']} onclick={() => onAnalyze('mix-chords')}>{busy['mix-chords'] ? '…' : 'Detect'}</button>{/if}
      </div>
      {#each audio.stems as stem, index (stem.stem)}
        {@const lane = lanes[index]}
        <div class="arr-head" class:selected={selectedStem === stem.stem} style:height="{LANE_HEIGHT}px" style:--lane-color={colorOf(stem.stem)}>
          <div class="arr-head-top">
            <button type="button" class="arr-name" onclick={() => { selectedStem = stem.stem; }} aria-pressed={selectedStem === stem.stem}><i></i>{laneName(stem.stem)}</button>
            <button type="button" class:active={stem.muted} aria-pressed={stem.muted} title="Mute (M)" onclick={() => toggleMute(stem)}>M</button>
            <button type="button" class:active={stem.solo} aria-pressed={stem.solo} title="Solo (S)" onclick={() => toggleSolo(stem)}>S</button>
          </div>
          <input type="range" aria-label={`${stem.stem} volume`} min="0" max="2" step="0.01" value={stem.volume}
            oninput={(event) => onVolume('set_stem_volume', { stem: stem.stem, volume: Number(event.currentTarget.value) })} />
          <div class="arr-now" title="Sounding now">
            {#if lane?.capabilities.notes}<span>♪ {now.lanes[index]?.notes || '—'}</span>{/if}
            {#if lane?.capabilities.chords}<span>♫ {now.lanes[index]?.chord || '—'}</span>{/if}
          </div>
          <div class="arr-actions">
            {#if lane?.capabilities.notes}
              {#if lane.hasNotes}<small>{lane.noteCount} notes</small>
              {:else}<button type="button" disabled={busy[`${stem.stem}:notes`] || batch !== undefined} onclick={() => onAnalyze('notes', stem.stem)}>{busy[`${stem.stem}:notes`] ? 'Detecting…' : 'Detect notes'}</button>{/if}
            {/if}
            {#if lane?.capabilities.chords}
              {#if lane.hasChords}<small>{lane.chordCount} chords</small>
              {:else}<button type="button" disabled={busy[`${stem.stem}:chords`] || batch !== undefined} onclick={() => onAnalyze('chords', stem.stem)}>{busy[`${stem.stem}:chords`] ? 'Detecting…' : 'Detect chords'}</button>{/if}
            {/if}
          </div>
          {#if errors[`${stem.stem}:notes`] || errors[`${stem.stem}:chords`]}
            <small class="error" title={errors[`${stem.stem}:notes`] ?? errors[`${stem.stem}:chords`]}>{errors[`${stem.stem}:notes`] ?? errors[`${stem.stem}:chords`]}</small>
          {/if}
        </div>
      {/each}
    </div>

    <div class="arr-scroll" bind:this={scroller} onscroll={(event) => { scrollLeft = event.currentTarget.scrollLeft; }} onwheel={onWheel}>
      <div class="arr-spacer" style:width="{totalWidth}px" style:height="{totalHeight}px">
        <div class="arr-sticky" style:width="{viewportWidth}px" style:height="{totalHeight}px" role="slider" tabindex="0"
          aria-label="Song position" aria-valuemin={0} aria-valuemax={Math.round(duration)} aria-valuenow={Math.round(playhead)} aria-valuetext={`${formatClock(playhead)}, ${position}`}
          onpointerdown={onPointerDown} onpointermove={onPointerMove} onpointerup={onPointerUp} onpointercancel={onPointerUp} onpointerleave={() => { hoverTime = undefined; }} onkeydown={onKeyDown}>
          <canvas class="arr-canvas" bind:this={baseCanvas} style:width="{viewportWidth}px" style:height="{totalHeight}px"></canvas>
          <canvas class="arr-canvas arr-overlay" bind:this={overlayCanvas} style:width="{viewportWidth}px" style:height="{totalHeight}px"></canvas>
          {#if hoverX !== undefined}<div class="arr-hover" style:transform="translateX({hoverX}px)"><span>{formatClock(hoverTime ?? 0)}</span></div>{/if}
          {#if playheadX >= -2 && playheadX <= viewportWidth + 2}<div class="arr-playhead" style:transform="translateX({playheadX}px)"></div>{/if}
        </div>
      </div>
    </div>
  </div>
</div>
