"""Tests for the dataset utilities (Epic F, EFT1/EFT2)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from lytk.datasets import Dataset, FolderDataset, load_document

LY_DIR = Path("tests/fixtures/ly")
MXL_DIR = Path("tests/fixtures/mxl")
MIDI_DIR = Path("tests/fixtures/midi")


def _small_ly_folder(tmp_path: Path, n: int = 5) -> Path:
    """Copy a handful of .ly fixtures into a temp folder."""
    srcs = sorted(LY_DIR.glob("*.ly"))[:n]
    dst = tmp_path / "ly"
    dst.mkdir()
    for s in srcs:
        (dst / s.name).write_text(s.read_text())
    return dst


# ---------------------------------------------------------------------------
# load_document
# ---------------------------------------------------------------------------


class TestLoadDocument:
    def test_load_lilypond(self):
        ly = sorted(LY_DIR.glob("*.ly"))[0]
        doc = load_document(ly)
        # A MusicDocument exposes to_score(); use it as a smoke check.
        assert hasattr(doc, "to_score")

    def test_load_mxl(self):
        mxl = sorted(MXL_DIR.glob("*.mxl"))[0]
        doc = load_document(mxl)
        arr = __import__("lytk").to_note_array(doc, 24)
        assert arr.ndim == 2 and arr.shape[1] == 4

    def test_load_midi(self):
        midi = sorted(MIDI_DIR.glob("*.mid*"))[0]
        doc = load_document(midi)
        assert hasattr(doc, "to_score")

    def test_unsupported_extension(self, tmp_path):
        f = tmp_path / "x.txt"
        f.write_text("nope")
        with pytest.raises(ValueError):
            load_document(f)


# ---------------------------------------------------------------------------
# FolderDataset
# ---------------------------------------------------------------------------


class TestFolderDataset:
    def test_discovers_files(self, tmp_path):
        folder = _small_ly_folder(tmp_path, 5)
        ds = FolderDataset(folder)
        assert len(ds) == 5
        assert all(name.endswith(".ly") for name in ds.filenames)

    def test_getitem_and_iter(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        docs = list(ds)
        assert len(docs) == 3
        assert all(hasattr(d, "to_score") for d in docs)

    def test_to_note_arrays(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        arrays = ds.to_note_arrays(resolution=24)
        assert len(arrays) == 3
        assert all(a.ndim == 2 and a.shape[1] == 4 for a in arrays)

    def test_to_pianorolls(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        rolls = ds.to_pianorolls(resolution=4)
        assert all(r.shape[1] == 128 for r in rolls)

    def test_metrics(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        ms = ds.metrics(resolution=480)
        assert len(ms) == 2
        assert "scale_consistency" in ms[0]

    def test_not_a_directory(self, tmp_path):
        f = tmp_path / "file.ly"
        f.write_text("{ c'4 }")
        with pytest.raises(NotADirectoryError):
            FolderDataset(f)


# ---------------------------------------------------------------------------
# Splits (EFT2)
# ---------------------------------------------------------------------------


class TestSplit:
    def test_partitions_cover_all_items_disjointly(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 10))
        train, val, test = ds.split((0.8, 0.1, 0.1), seed=1)
        assert len(train) + len(val) + len(test) == 10
        # Disjoint coverage of the parent indices.
        all_idx = sorted(train.indices + val.indices + test.indices)
        assert all_idx == list(range(10))

    def test_deterministic(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 10))
        a = ds.split(seed=42)
        b = ds.split(seed=42)
        assert [s.indices for s in a] == [s.indices for s in b]

    def test_subset_is_a_dataset(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 6))
        (train,) = ds.split((1.0,))
        assert isinstance(train, Dataset)
        assert len(train.to_note_arrays(24)) == 6


# ---------------------------------------------------------------------------
# Caching (EFT2)
# ---------------------------------------------------------------------------


class TestCaching:
    def test_caches_to_disk(self, tmp_path):
        folder = _small_ly_folder(tmp_path, 2)
        cache = tmp_path / "cache"
        ds = FolderDataset(folder, cache_dir=cache)
        first = ds.to_note_arrays(resolution=24)
        cached = list(cache.glob("*.npy"))
        assert len(cached) == 2
        # Second call loads from cache and matches.
        second = ds.to_note_arrays(resolution=24)
        for a, b in zip(first, second):
            assert np.array_equal(a, b)

    def test_cache_keys_unique_across_subfolders(self, tmp_path):
        """Two files sharing a stem in different subfolders must not collide.

        Regression test: the cache key used to be derived from the stem only,
        so ``train/foo.ly`` and ``valid/foo.ly`` overwrote each other.
        """
        srcs = sorted(LY_DIR.glob("*.ly"))[:2]
        assert len(srcs) == 2 and srcs[0].read_text() != srcs[1].read_text()

        root = tmp_path / "corpus"
        (root / "train").mkdir(parents=True)
        (root / "valid").mkdir(parents=True)
        # Same file *name* (same stem) in two subfolders, different content.
        (root / "train" / "foo.ly").write_text(srcs[0].read_text())
        (root / "valid" / "foo.ly").write_text(srcs[1].read_text())

        cache = tmp_path / "cache"
        ds = FolderDataset(root, recursive=True, cache_dir=cache)
        assert len(ds) == 2

        arrays = ds.to_note_arrays(resolution=24)
        # Each source produced its own cache file (no clobbering).
        assert len(list(cache.glob("*.npy"))) == 2
        # Re-reading from cache returns each file's own representation, and the
        # two distinct sources do not collapse to a single shared array.
        cached = ds.to_note_arrays(resolution=24)
        for a, b in zip(arrays, cached):
            assert np.array_equal(a, b)
        assert not np.array_equal(arrays[0], arrays[1])


# ---------------------------------------------------------------------------
# Lazy conversion + framework adapters (Finding 3)
# ---------------------------------------------------------------------------


class TestLazyConversion:
    def test_iter_representation_is_lazy(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        it = ds.iter_representation("note_array", resolution=24)
        # A generator, not a materialised list.
        assert iter(it) is it
        arrays = list(it)
        assert len(arrays) == 3
        assert all(a.ndim == 2 and a.shape[1] == 4 for a in arrays)

    def test_iter_representation_rejects_unknown(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 1))
        with pytest.raises(ValueError):
            list(ds.iter_representation("nope"))

    def test_to_representation_matches_iter(self, tmp_path):
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        eager = ds.to_representation("note_array", resolution=24)
        lazy = list(ds.iter_representation("note_array", resolution=24))
        assert len(eager) == len(lazy)
        for a, b in zip(eager, lazy):
            assert np.array_equal(a, b)

    def test_pytorch_dataset_is_lazy(self, tmp_path):
        """The torch adapter must not materialise the whole dataset up front."""
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))

        calls: list[int] = []
        original = ds._convert_item

        def _spy(index, *a, **k):
            calls.append(index)
            return original(index, *a, **k)

        ds._convert_item = _spy  # type: ignore[method-assign]
        td = ds.to_pytorch_dataset("note_array", resolution=24)
        # Constructing the torch dataset converts nothing.
        assert calls == []
        assert len(td) == 3
        item0 = td[0]
        # Only the accessed item is converted.
        assert calls == [0]
        assert item0.ndim == 2

    def test_pytorch_dataset_rejects_unknown(self, tmp_path):
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 1))
        with pytest.raises(ValueError):
            ds.to_pytorch_dataset("nope")

    def test_tensorflow_dataset_streams(self, tmp_path):
        tf = pytest.importorskip("tensorflow")
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        tfds = ds.to_tensorflow_dataset("note_array", resolution=24)
        collected = [x.numpy() for x in tfds]
        assert len(collected) == 2
        assert all(x.shape[-1] == 4 for x in collected)
