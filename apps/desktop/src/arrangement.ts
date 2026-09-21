// Pure logic behind the mixer's arrangement view: the beat/bar grid, time <-> pixel
// mapping, what is sounding at a given instant, and waveform reduction per pixel
// column. Nothing here touches the DOM or Tauri, so it is unit-tested directly.

export type Waveform = { sampleWindows: number; min: number[]; max: number[] };
export type Interval = { start: number; end: number };
export type TimedNote = Interval & { midi: number; label: string; strength: number };
export type TimedChord = Interval & { label: string };

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

export function midiName(midi: number): string {
  const rounded = Math.round(midi);
  return `${NOTE_NAMES[((rounded % 12) + 12) % 12]}${Math.floor(rounded / 12) - 1}`;
}

export const clamp = (value: number, low: number, high: number): number => Math.min(high, Math.max(low, value));

// ---- Beat / bar grid ----------------------------------------------------------

export type GridLine = { time: number; bar: number; beat: number; isBar: boolean };

/**
 * Turns detected beat times into bars. Downbeats are not detected, so the grouping is
 * the user's choice: `beatsPerBar` beats per bar and `downbeatOffset` beats before the
 * first downbeat (those form a pre-roll "bar 0").
 */
export function buildGrid(beatTimes: number[], beatsPerBar: number, downbeatOffset: number): GridLine[] {
  const perBar = Math.max(1, Math.floor(beatsPerBar));
  const offset = ((Math.floor(downbeatOffset) % perBar) + perBar) % perBar;
  const times = beatTimes.filter((time) => Number.isFinite(time) && time >= 0).sort((a, b) => a - b);
  return times.map((time, index) => {
    const shifted = index - offset;
    if (shifted < 0) return { time, bar: 0, beat: perBar - offset + index + 1, isBar: false };
    const beat = (shifted % perBar) + 1;
    return { time, bar: Math.floor(shifted / perBar) + 1, beat, isBar: beat === 1 };
  });
}

function lastAtOrBefore<T extends { time: number }>(lines: T[], time: number): number {
  let low = 0;
  let high = lines.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (lines[middle].time <= time) { found = middle; low = middle + 1; } else high = middle - 1;
  }
  return found;
}

/** Bar and beat under `time`, or null before the first detected beat. */
export function gridPosition(grid: GridLine[], time: number): { bar: number; beat: number } | null {
  const index = lastAtOrBefore(grid, time);
  return index < 0 ? null : { bar: grid[index].bar, beat: grid[index].beat };
}

export function formatBarBeat(position: { bar: number; beat: number } | null): string {
  if (position === null) return '—';
  return position.bar === 0 ? `Pre · ${position.beat}` : `Bar ${position.bar} · ${position.beat}`;
}

/**
 * Where "next/previous" lands. Backward skips a small margin so pressing it just after a
 * line goes to the line before, like a DAW. Returns null when there is nothing in that
 * direction (the caller goes to the end / the start).
 */
export function stepTarget(grid: GridLine[], time: number, direction: -1 | 1, unit: 'bar' | 'beat'): number | null {
  const lines = unit === 'bar' ? grid.filter((line) => line.isBar) : grid;
  if (direction > 0) return lines.find((line) => line.time > time + 0.05)?.time ?? null;
  const margin = unit === 'bar' ? 0.6 : 0.25;
  for (let index = lines.length - 1; index >= 0; index -= 1) if (lines[index].time < time - margin) return lines[index].time;
  return null;
}

/** Without a grid the skip buttons move by a fixed number of seconds. */
export function stepSeconds(time: number, direction: -1 | 1, seconds: number, duration: number): number {
  return clamp(time + direction * seconds, 0, duration);
}

/** Every Nth bar label so labels stay readable: N is 1, 2, 4, 8 or 16. */
export function labelStride(pixelsPerBar: number, minGapPx = 44): number {
  const needed = minGapPx / Math.max(pixelsPerBar, 0.001);
  for (const stride of [1, 2, 4, 8, 16]) if (stride >= needed) return stride;
  return 32;
}

/** Ruler ticks (seconds) for when no beat grid exists. */
export function timeTicks(duration: number, pixelsPerSecond: number, minGapPx = 64): number[] {
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
  const step = steps.find((candidate) => candidate * pixelsPerSecond >= minGapPx) ?? 600;
  const ticks: number[] = [];
  for (let time = 0; time <= duration; time += step) ticks.push(time);
  return ticks;
}

// ---- Time <-> pixels, zoom, follow ----------------------------------------------

export const MIN_PIXELS_PER_SECOND = 6;
export const MAX_PIXELS_PER_SECOND = 480;

export const clampZoom = (pixelsPerSecond: number): number => clamp(pixelsPerSecond, MIN_PIXELS_PER_SECOND, MAX_PIXELS_PER_SECOND);
export const fitZoom = (duration: number, viewportWidth: number): number => clampZoom(viewportWidth / Math.max(duration, 1));
export const timeToX = (time: number, pixelsPerSecond: number): number => time * pixelsPerSecond;
export const xToTime = (x: number, pixelsPerSecond: number, duration: number): number => clamp(x / pixelsPerSecond, 0, duration);

/** Zoom by `factor` keeping the time under `anchorX` (viewport pixels) where it is. */
export function zoomAt(view: { pixelsPerSecond: number; scrollLeft: number }, factor: number, anchorX: number): { pixelsPerSecond: number; scrollLeft: number } {
  const anchorTime = (view.scrollLeft + anchorX) / view.pixelsPerSecond;
  const pixelsPerSecond = clampZoom(view.pixelsPerSecond * factor);
  return { pixelsPerSecond, scrollLeft: Math.max(0, anchorTime * pixelsPerSecond - anchorX) };
}

/** New scroll position that brings the playhead back into view, or null if it already is. */
export function followScroll(playheadX: number, scrollLeft: number, viewportWidth: number): number | null {
  if (playheadX >= scrollLeft + viewportWidth * 0.1 && playheadX <= scrollLeft + viewportWidth * 0.9) return null;
  return Math.max(0, playheadX - viewportWidth * 0.25);
}

/** The engine is polled a few times a second; between polls the playhead is extrapolated. */
export function projectPosition(base: number, polledAtMs: number, nowMs: number, playing: boolean, duration: number): number {
  return playing ? Math.min(duration, base + Math.max(0, nowMs - polledAtMs) / 1000) : base;
}

// ---- What is sounding ------------------------------------------------------------

export type IntervalIndex<T extends Interval> = { items: T[]; longest: number };

export function indexIntervals<T extends Interval>(items: T[]): IntervalIndex<T> {
  const sorted = [...items].sort((a, b) => a.start - b.start);
  let longest = 0;
  for (const item of sorted) longest = Math.max(longest, item.end - item.start);
  return { items: sorted, longest };
}

function firstStartingAfter<T extends Interval>(items: T[], time: number): number {
  let low = 0;
  let high = items.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (items[middle].start > time) high = middle; else low = middle + 1;
  }
  return low;
}

/** Items overlapping the half-open window [from, to), in start order. */
export function overlapping<T extends Interval>(index: IntervalIndex<T>, from: number, to: number): T[] {
  const found: T[] = [];
  for (let position = firstStartingAfter(index.items, to) - 1; position >= 0; position -= 1) {
    const item = index.items[position];
    if (item.start < from - index.longest) break;
    if (item.end > from && item.start < to) found.push(item);
  }
  return found.reverse();
}

/** Items sounding at `time`. */
export function activeAt<T extends Interval>(index: IntervalIndex<T>, time: number): T[] {
  const found: T[] = [];
  for (let position = firstStartingAfter(index.items, time) - 1; position >= 0; position -= 1) {
    const item = index.items[position];
    if (item.start < time - index.longest) break;
    if (item.end > time) found.push(item);
  }
  return found.reverse();
}

// ---- Note lanes ------------------------------------------------------------------

export type PitchRange = { low: number; high: number };

/** The lane's own pitch window (at least `minSpan` semitones) so small ranges are not flat. */
export function pitchRange(notes: { midi: number }[], minSpan = 12): PitchRange {
  if (notes.length === 0) return { low: 48, high: 48 + minSpan - 1 };
  let low = Infinity;
  let high = -Infinity;
  for (const note of notes) { low = Math.min(low, note.midi); high = Math.max(high, note.midi); }
  low -= 1;
  high += 1;
  const missing = minSpan - (high - low + 1);
  if (missing > 0) { low -= Math.floor(missing / 2); high += Math.ceil(missing / 2); }
  return { low: Math.max(0, low), high: Math.min(127, high) };
}

export function noteRow(midi: number, range: PitchRange, height: number): { y: number; height: number } {
  const rows = range.high - range.low + 1;
  const rowHeight = height / rows;
  return { y: (range.high - clamp(midi, range.low, range.high)) * rowHeight, height: rowHeight };
}

type PitchNoteLike = { start: number; end: number; midi: number; note: string; confidence: number };
type AmtNoteLike = { start: number; end: number; midi: number; velocity: number };

export function notesFromPitch(notes: PitchNoteLike[]): TimedNote[] {
  return notes.map((note) => ({ start: note.start, end: note.end, midi: note.midi, label: note.note, strength: clamp(note.confidence, 0, 1) }));
}

export function notesFromAmt(notes: AmtNoteLike[]): TimedNote[] {
  return notes.map((note) => ({ start: note.start, end: note.end, midi: note.midi, label: midiName(note.midi), strength: clamp(note.velocity / 127, 0, 1) }));
}

/** What can be detected for a stem: monophonic pYIN, polyphonic Basic Pitch, chords, or nothing. */
export type LaneCapabilities = { notes: 'pitch' | 'amt' | null; chords: boolean };

export function laneCapabilities(stem: string): LaneCapabilities {
  if (stem === 'vocals' || stem === 'bass') return { notes: 'pitch', chords: false };
  if (stem === 'drums') return { notes: null, chords: false };
  return { notes: 'amt', chords: true };
}

// ---- Waveform ----------------------------------------------------------------------

/** Min/max per pixel column for the viewport starting at `startTime`. */
export function waveformColumns(waveform: Waveform, duration: number, startTime: number, pixelsPerSecond: number, widthPx: number): { min: Float32Array; max: Float32Array } {
  const min = new Float32Array(widthPx);
  const max = new Float32Array(widthPx);
  const windows = Math.min(waveform.min.length, waveform.max.length);
  if (windows === 0 || duration <= 0) return { min, max };
  for (let column = 0; column < widthPx; column += 1) {
    const from = startTime + column / pixelsPerSecond;
    const first = Math.floor((from / duration) * windows);
    const last = Math.max(first + 1, Math.ceil(((from + 1 / pixelsPerSecond) / duration) * windows));
    if (first < 0 || first >= windows) continue;
    let low = 0;
    let high = 0;
    for (let window = first; window < Math.min(last, windows); window += 1) {
      low = Math.min(low, waveform.min[window]);
      high = Math.max(high, waveform.max[window]);
    }
    min[column] = low;
    max[column] = high;
  }
  return { min, max };
}

// ---- Formatting ----------------------------------------------------------------------

export function formatClock(seconds: number): string {
  const safe = Math.max(0, seconds);
  const minutes = Math.floor(safe / 60);
  return `${minutes}:${(safe - minutes * 60).toFixed(1).padStart(4, '0')}`;
}
