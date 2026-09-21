import { describe, expect, it } from 'vitest';
import {
  activeAt, buildGrid, clampZoom, fitZoom, followScroll, formatBarBeat, formatClock, gridPosition, indexIntervals,
  labelStride, laneCapabilities, midiName, noteRow, notesFromAmt, notesFromPitch, overlapping, pitchRange,
  projectPosition, stepSeconds, stepTarget, timeTicks, waveformColumns, xToTime, zoomAt
} from './arrangement';

const beats = (count: number, interval = 0.5, first = 0.25): number[] => Array.from({ length: count }, (_, index) => first + index * interval);

describe('grid', () => {
  it('groups beats into bars of the chosen size', () => {
    const grid = buildGrid(beats(9), 4, 0);

    expect(grid.map((line) => [line.bar, line.beat])).toEqual([[1, 1], [1, 2], [1, 3], [1, 4], [2, 1], [2, 2], [2, 3], [2, 4], [3, 1]]);
    expect(grid.filter((line) => line.isBar).map((line) => line.time)).toEqual([0.25, 2.25, 4.25]);
  });

  it('treats beats before the downbeat as a pre-roll bar 0', () => {
    const grid = buildGrid(beats(6), 4, 2);

    expect(grid.map((line) => [line.bar, line.beat])).toEqual([[0, 3], [0, 4], [1, 1], [1, 2], [1, 3], [1, 4]]);
    expect(grid[0].isBar).toBe(false);
    expect(grid[2].isBar).toBe(true);
  });

  it('ignores bad beat times and normalizes the offset', () => {
    const grid = buildGrid([1, Number.NaN, -1, 0.5], 3, 5);

    expect(grid.map((line) => line.time)).toEqual([0.5, 1]);
    expect(buildGrid(beats(4), 4, 4).map((line) => line.bar)).toEqual([1, 1, 1, 1]);
    expect(buildGrid(beats(2), 0, 0).map((line) => line.isBar)).toEqual([true, true]);
  });

  it('reports the bar and beat under the playhead', () => {
    const grid = buildGrid(beats(8), 4, 0);

    expect(gridPosition(grid, 0.1)).toBeNull();
    expect(gridPosition(grid, 0.25)).toEqual({ bar: 1, beat: 1 });
    expect(gridPosition(grid, 1.9)).toEqual({ bar: 1, beat: 4 });
    expect(gridPosition(grid, 99)).toEqual({ bar: 2, beat: 4 });
    expect(gridPosition([], 5)).toBeNull();
    expect(formatBarBeat({ bar: 2, beat: 3 })).toBe('Bar 2 · 3');
    expect(formatBarBeat({ bar: 0, beat: 4 })).toBe('Pre · 4');
    expect(formatBarBeat(null)).toBe('—');
  });
});

describe('stepping', () => {
  const grid = buildGrid(beats(12), 4, 0); // bars at 0.25, 2.25, 4.25

  it('jumps to the next bar and null past the last one', () => {
    expect(stepTarget(grid, 0, 1, 'bar')).toBe(0.25);
    expect(stepTarget(grid, 0.25, 1, 'bar')).toBe(2.25);
    expect(stepTarget(grid, 3, 1, 'bar')).toBe(4.25);
    expect(stepTarget(grid, 4.25, 1, 'bar')).toBeNull();
  });

  it('goes back to the previous bar, not the one just passed', () => {
    expect(stepTarget(grid, 2.4, -1, 'bar')).toBe(0.25);
    expect(stepTarget(grid, 3.5, -1, 'bar')).toBe(2.25);
    expect(stepTarget(grid, 0.5, -1, 'bar')).toBeNull();
  });

  it('steps by beat and falls back to seconds without a grid', () => {
    expect(stepTarget(grid, 1.0, 1, 'beat')).toBe(1.25);
    expect(stepTarget(grid, 1.1, -1, 'beat')).toBe(0.75);
    expect(stepTarget(grid, 1.0, -1, 'beat')).toBe(0.25); // just after a beat: go to the one before
    expect(stepTarget([], 1, 1, 'bar')).toBeNull();
    expect(stepSeconds(3, -1, 5, 100)).toBe(0);
    expect(stepSeconds(98, 1, 5, 100)).toBe(100);
  });
});

describe('ruler', () => {
  it('thins bar labels as the view zooms out', () => {
    expect(labelStride(200)).toBe(1);
    expect(labelStride(30)).toBe(2);
    expect(labelStride(12)).toBe(4);
    expect(labelStride(0.5)).toBe(32);
  });

  it('picks second ticks that are far enough apart', () => {
    expect(timeTicks(30, 100)).toHaveLength(31);
    expect(timeTicks(100, 10)).toEqual([0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
  });
});

describe('zoom, position and follow', () => {
  it('clamps zoom and fits a track to the viewport', () => {
    expect(clampZoom(1)).toBe(6);
    expect(clampZoom(9999)).toBe(480);
    expect(fitZoom(200, 1000)).toBe(6);
    expect(fitZoom(10, 1000)).toBe(100);
  });

  it('keeps the time under the cursor fixed while zooming', () => {
    const before = { pixelsPerSecond: 50, scrollLeft: 200 };
    const anchorX = 300;
    const anchorTime = (before.scrollLeft + anchorX) / before.pixelsPerSecond;

    const after = zoomAt(before, 2, anchorX);

    expect(after.pixelsPerSecond).toBe(100);
    expect((after.scrollLeft + anchorX) / after.pixelsPerSecond).toBeCloseTo(anchorTime);
    expect(zoomAt({ pixelsPerSecond: 50, scrollLeft: 0 }, 0.1, 10).scrollLeft).toBeGreaterThanOrEqual(0);
  });

  it('maps pixels to a clamped time', () => {
    expect(xToTime(500, 100, 60)).toBe(5);
    expect(xToTime(-10, 100, 60)).toBe(0);
    expect(xToTime(1e6, 100, 60)).toBe(60);
  });

  it('scrolls only when the playhead leaves the comfortable middle', () => {
    expect(followScroll(500, 0, 1000)).toBeNull();
    expect(followScroll(950, 0, 1000)).toBe(700);
    expect(followScroll(20, 500, 1000)).toBe(0);
  });

  it('extrapolates the playhead only while playing', () => {
    expect(projectPosition(10, 1000, 1500, true, 100)).toBeCloseTo(10.5);
    expect(projectPosition(10, 1000, 1500, false, 100)).toBe(10);
    expect(projectPosition(99.9, 1000, 3000, true, 100)).toBe(100);
    expect(projectPosition(10, 2000, 1000, true, 100)).toBe(10);
  });
});

describe('what is sounding', () => {
  const notes = [
    { start: 0, end: 4, midi: 40, label: 'E2', strength: 1 },
    { start: 1, end: 2, midi: 52, label: 'E3', strength: 1 },
    { start: 3, end: 3.5, midi: 55, label: 'G3', strength: 1 },
    { start: 6, end: 7, midi: 45, label: 'A2', strength: 1 }
  ];
  const index = indexIntervals([...notes].reverse());

  it('finds overlapping (polyphonic) notes at an instant, including a long earlier one', () => {
    expect(activeAt(index, 1.5).map((note) => note.label)).toEqual(['E2', 'E3']);
    expect(activeAt(index, 3.2).map((note) => note.label)).toEqual(['E2', 'G3']);
    expect(activeAt(index, 5).map((note) => note.label)).toEqual([]);
    expect(activeAt(index, 6).map((note) => note.label)).toEqual(['A2']);
  });

  it('treats the end of a note as exclusive', () => {
    expect(activeAt(index, 2).map((note) => note.label)).toEqual(['E2']);
  });

  it('lists what overlaps a viewport window', () => {
    expect(overlapping(index, 2.5, 3.2).map((note) => note.label)).toEqual(['E2', 'G3']);
    expect(overlapping(index, 4, 6).map((note) => note.label)).toEqual([]);
    expect(overlapping(indexIntervals([]), 0, 10)).toEqual([]);
  });
});

describe('note lanes', () => {
  it('names notes in scientific pitch notation', () => {
    expect(midiName(60)).toBe('C4');
    expect(midiName(45)).toBe('A2');
    expect(midiName(61)).toBe('C#4');
    expect(midiName(0)).toBe('C-1');
  });

  it('gives each lane a window of at least an octave around its notes', () => {
    expect(pitchRange([{ midi: 45 }, { midi: 47 }])).toEqual({ low: 41, high: 52 });
    expect(pitchRange([{ midi: 30 }, { midi: 70 }])).toEqual({ low: 29, high: 71 });
    expect(pitchRange([])).toEqual({ low: 48, high: 59 });
    expect(pitchRange([{ midi: 0 }]).low).toBe(0);
  });

  it('places higher notes nearer the top and clamps out-of-range notes', () => {
    const range = { low: 40, high: 51 };
    const high = noteRow(51, range, 120);
    const low = noteRow(40, range, 120);

    expect(high.y).toBe(0);
    expect(low.y).toBe(110);
    expect(high.height).toBe(10);
    expect(noteRow(90, range, 120).y).toBe(0);
  });

  it('normalizes pitch and AMT notes', () => {
    expect(notesFromPitch([{ start: 0, end: 1, midi: 45, note: 'A2', confidence: 0.9 }])).toEqual([{ start: 0, end: 1, midi: 45, label: 'A2', strength: 0.9 }]);
    const [amt] = notesFromAmt([{ start: 0, end: 1, midi: 60, velocity: 127 }]);
    expect(amt).toMatchObject({ label: 'C4', strength: 1 });
  });

  it('knows what each stem can be analyzed for', () => {
    expect(laneCapabilities('vocals')).toEqual({ notes: 'pitch', chords: false });
    expect(laneCapabilities('bass')).toEqual({ notes: 'pitch', chords: false });
    expect(laneCapabilities('drums')).toEqual({ notes: null, chords: false });
    for (const stem of ['other', 'guitar', 'piano']) expect(laneCapabilities(stem)).toEqual({ notes: 'amt', chords: true });
  });
});

describe('waveform columns', () => {
  const waveform = { sampleWindows: 1, min: [-0.1, -0.5, -0.2, -0.9], max: [0.1, 0.5, 0.2, 0.9] };

  it('reduces windows into one min/max per pixel column', () => {
    // 4 windows over 4 s at 1 px/s: one window per column.
    const columns = waveformColumns(waveform, 4, 0, 1, 4);

    expect(Array.from(columns.max)).toEqual([0.1, 0.5, 0.2, 0.9].map(Math.fround));
    expect(columns.min[3]).toBeCloseTo(-0.9);
  });

  it('merges several windows into a coarse column when zoomed out', () => {
    const columns = waveformColumns(waveform, 4, 0, 0.5, 2);

    expect(columns.max[0]).toBeCloseTo(0.5);
    expect(columns.max[1]).toBeCloseTo(0.9);
  });

  it('leaves columns beyond the track empty and survives empty input', () => {
    const columns = waveformColumns(waveform, 4, 3, 1, 4);

    expect(columns.max[0]).toBeCloseTo(0.9);
    expect(columns.max[2]).toBe(0);
    expect(Array.from(waveformColumns({ sampleWindows: 1, min: [], max: [] }, 4, 0, 1, 3).max)).toEqual([0, 0, 0]);
  });
});

describe('formatting', () => {
  it('formats a clock with tenths', () => {
    expect(formatClock(0)).toBe('0:00.0');
    expect(formatClock(72.34)).toBe('1:12.3');
    expect(formatClock(-5)).toBe('0:00.0');
  });
});
