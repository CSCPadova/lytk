"""Tests for the generation-evaluation metrics (lytk.metrics)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

pytest.importorskip("scipy")  # JS + FMD both need scipy

import lytk
from lytk import metrics


def test_js_descriptor_similarity_math():
    # Identical descriptor distributions → JS divergence 0 → score 100.
    agg = {m: {"mean": 0.5, "std": 0.1, "n": 10} for m in metrics.JS_METRICS}
    assert metrics.js_descriptor_similarity(agg, agg) == pytest.approx(100.0, abs=1e-3)
    # A shifted distribution scores strictly lower.
    shifted = {m: {"mean": 0.9, "std": 0.1, "n": 10} for m in metrics.JS_METRICS}
    assert metrics.js_descriptor_similarity(agg, shifted) < 100.0


def test_js_similarity_end_to_end():
    # Descriptor extraction wired through lytk.compute_metrics; identical corpora
    # score 100 (or None if a descriptor has zero variance across the sample).
    paths = sorted(Path("tests/fixtures/xml").glob("*.xml"))[:6]
    docs = [lytk.from_musicxml(str(p)).to_music_document() for p in paths]
    score = metrics.js_similarity(docs, docs)
    assert score is None or score == pytest.approx(100.0, abs=1e-3)


def test_fmd_identical_sets_zero():
    rng = np.random.default_rng(0)
    x = rng.standard_normal((20, 8))
    assert metrics.frechet_music_distance(x, x) == pytest.approx(0.0, abs=1e-6)


def test_fmd_separated_sets_positive():
    rng = np.random.default_rng(0)
    x = rng.standard_normal((20, 8))
    y = rng.standard_normal((20, 8)) + 5.0
    # Two means 5 apart over 8 dims → ||Δμ||² ≈ 200.
    assert metrics.frechet_music_distance(x, y) > 100.0


def test_fmd_needs_two_docs():
    with pytest.raises(ValueError):
        metrics.frechet_music_distance(np.zeros((1, 4)), np.zeros((3, 4)))
