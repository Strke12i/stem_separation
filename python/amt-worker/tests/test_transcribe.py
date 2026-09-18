from __future__ import annotations

from music_analyzer_amt.transcribe import event_note


def test_converts_a_basic_pitch_event_to_a_bounded_midi_note() -> None:
    assert event_note((0.1, 0.5, 60, 1.2, None)) == {
        "start": 0.1,
        "end": 0.5,
        "midi": 60,
        "velocity": 127,
    }
