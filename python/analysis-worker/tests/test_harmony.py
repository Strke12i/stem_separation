from __future__ import annotations

from typing import Any

import numpy as np
import pytest

from music_analyzer_worker.harmony import HarmonyError, infer_harmony


def triad_chroma(root: int, intervals: tuple[int, int, int]) -> np.ndarray[Any, Any]:
    chroma = np.zeros((12, 12), dtype=float)
    chroma[[(root + interval) % 12 for interval in intervals], :] = 1.0
    return chroma


def test_detects_a_synthetic_c_major_triad() -> None:
    result = infer_harmony(triad_chroma(0, (0, 4, 7)), [index * 0.1 for index in range(12)], [])

    assert result["key"]["label"] == "C major"
    assert result["chords"][0]["label"] == "C"


def test_detects_a_synthetic_a_minor_triad() -> None:
    result = infer_harmony(
        triad_chroma(9, (0, 3, 7)),
        [index * 0.1 for index in range(12)],
        [0.0, 0.5, 1.0],
    )

    assert result["key"]["label"] == "A minor"
    assert result["chords"][0]["label"] == "Am"


def test_silent_chroma_is_refused_instead_of_guessing_a_key() -> None:
    with pytest.raises(HarmonyError) as raised:
        infer_harmony(np.zeros((12, 12)), [index * 0.1 for index in range(12)], [])

    assert raised.value.code == "SILENT_AUDIO"
