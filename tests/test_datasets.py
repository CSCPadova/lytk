"""Tests for the dataset utilities (Epic F, EFT1/EFT2)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

import lytk
from lytk.datasets import (
    SUPPORTED_EXTENSIONS,
    Dataset,
    FolderDataset,
    RecordsDataset,
    load_document,
)

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


# ---------------------------------------------------------------------------
# Format coverage
# ---------------------------------------------------------------------------

ABC_DIR = Path("tests/fixtures/abc")


class TestFormatCoverage:
    def test_loads_abc(self):
        doc = load_document(sorted(ABC_DIR.glob("*.abc"))[0])
        assert __import__("lytk").to_note_array(doc).shape[1] == 4

    def test_loads_humdrum(self, tmp_path):
        import lytk

        krn = tmp_path / "gen.krn"
        lytk.to_humdrum(lytk.from_lilypond_string(r"\relative c' { c4 d e f }"), str(krn))
        assert lytk.to_note_array(load_document(krn)).shape == (4, 4)

    def test_folder_dataset_discovers_abc_and_kern(self, tmp_path):
        """A corpus of ABC/**kern files must not come back empty."""
        import lytk

        for src in sorted(ABC_DIR.glob("*.abc")):
            (tmp_path / src.name).write_text(src.read_text())
        lytk.to_humdrum(
            lytk.from_lilypond_string(r"\relative c' { c4 d e f }"),
            str(tmp_path / "gen.krn"),
        )
        ds = FolderDataset(tmp_path)
        assert len(ds) == len(list(ABC_DIR.glob("*.abc"))) + 1
        assert any(n.endswith(".krn") for n in ds.filenames)
        assert any(n.endswith(".abc") for n in ds.filenames)

    def test_extensions_match_the_cli(self):
        """`lytk.cli` and the dataset loader must accept the same formats.

        They keep separate lists (the CLI produces Scores, the loader produces
        MusicDocuments); this catches one gaining a format without the other,
        which is how ABC and **kern came to be silently undiscoverable.
        """
        from lytk.cli import _EXT_FORMAT

        assert SUPPORTED_EXTENSIONS == set(_EXT_FORMAT)


# ---------------------------------------------------------------------------
# Data loaders (padded batching)
# ---------------------------------------------------------------------------


class TestPytorchDataLoader:
    def test_ragged_items_batch(self, tmp_path):
        """The whole point: default torch collate cannot stack ragged scores."""
        torch = pytest.importorskip("torch")
        from torch.utils.data import DataLoader

        ds = FolderDataset(_small_ly_folder(tmp_path, 4))
        plain = ds.to_pytorch_dataset("event_sequence")
        lengths = [plain[i].shape[0] for i in range(len(plain))]
        assert len(set(lengths)) > 1, "fixtures must differ in length to be a test"
        with pytest.raises(RuntimeError):
            next(iter(DataLoader(plain, batch_size=len(plain))))

        padded, lens = next(
            iter(ds.to_pytorch_dataloader("event_sequence", batch_size=len(plain)))
        )
        assert lens.tolist() == lengths
        assert padded.shape == (len(plain), max(lengths))

    def test_content_and_padding_preserved(self, tmp_path):
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        plain = ds.to_pytorch_dataset("note_array", resolution=24)
        padded, lens = next(
            iter(
                ds.to_pytorch_dataloader(
                    "note_array", batch_size=3, representation_kwargs={"resolution": 24}
                )
            )
        )
        for i in range(3):
            assert (padded[i, : lens[i]] == plain[i]).all()
            assert (padded[i, lens[i] :] == 0).all()

    def test_pad_value_is_honoured(self, tmp_path):
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        padded, lens = next(
            iter(ds.to_pytorch_dataloader("event_sequence", batch_size=3, pad_value=-1))
        )
        assert (padded[lens.argmin(), lens.min() :] == -1).all()

    def test_pad_collate_is_reusable(self, tmp_path):
        """`pad_collate` must work with a hand-rolled DataLoader too."""
        pytest.importorskip("torch")
        from functools import partial

        from torch.utils.data import DataLoader

        from lytk.datasets import pad_collate

        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        loader = DataLoader(
            ds.to_pytorch_dataset("event_sequence"),
            batch_size=3,
            collate_fn=partial(pad_collate, pad_value=0),
        )
        padded, lens = next(iter(loader))
        assert padded.shape[0] == 3 and len(lens) == 3

    def test_rejects_unknown_representation(self, tmp_path):
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 1))
        with pytest.raises(ValueError):
            ds.to_pytorch_dataloader("nope")


class TestTensorflowDataLoader:
    def test_ragged_items_batch(self, tmp_path):
        pytest.importorskip("tensorflow")
        ds = FolderDataset(_small_ly_folder(tmp_path, 4))
        padded, lens = next(iter(ds.to_tensorflow_dataloader("event_sequence", batch_size=4)))
        expected = [a.shape[0] for a in ds.to_representation("event_sequence")]
        assert lens.numpy().tolist() == expected
        assert tuple(padded.shape) == (4, max(expected))

    def test_integer_pad_value_on_integer_dtypes(self, tmp_path):
        """The float default must not fail on TF's stricter dtype rules."""
        pytest.importorskip("tensorflow")
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        for rep, kwargs in (
            ("event_sequence", {}),
            ("note_array", {"resolution": 24}),
            ("piano_roll", {"resolution": 24}),
        ):
            padded, _ = next(
                iter(
                    ds.to_tensorflow_dataloader(
                        rep, batch_size=2, representation_kwargs=kwargs
                    )
                )
            )
            assert padded.dtype.is_integer

    def test_agrees_with_pytorch(self, tmp_path):
        pytest.importorskip("tensorflow")
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        tp, tl = next(iter(ds.to_pytorch_dataloader("event_sequence", batch_size=3)))
        fp, fl = next(iter(ds.to_tensorflow_dataloader("event_sequence", batch_size=3)))
        assert fl.numpy().tolist() == tl.tolist()
        assert np.array_equal(fp.numpy(), tp.numpy())


# ---------------------------------------------------------------------------
# Epic L: errors, movements, cache keys, ids, records, groups
# ---------------------------------------------------------------------------


def _write(folder: Path, files: dict[str, str | bytes]) -> Path:
    folder.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        path = folder / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content)
    return folder


def _pitches(doc) -> list[int]:
    return [int(p) for p in lytk.to_note_array(doc, 24)[:, 2]]


class TestOnError:
    FILES = {"a.ly": "{ c'4 d' }\n", "b.ly": b"{ c'4 \xe9 }\n", "c.ly": "{ e'1 }\n"}

    def test_raise_is_the_default(self, tmp_path):
        ds = FolderDataset(_write(tmp_path / "f", self.FILES))
        with pytest.raises(lytk.ParseError):
            list(ds)

    def test_skip_records_the_error(self, tmp_path):
        ds = FolderDataset(_write(tmp_path / "f", self.FILES), on_error="skip")
        assert [_pitches(d) for d in ds] == [[60, 62], [64]]
        assert list(ds.errors) == ["b.ly"] and ds.errors["b.ly"].startswith("ParseError")
        assert len(ds.to_note_arrays(resolution=24)) == 2
        assert len(ds.metrics()) == 2
        with pytest.raises(lytk.ParseError):
            ds[1]  # indexing asks for that item
        # A subset records into its parent's errors.
        ds.errors.clear()
        (whole,) = ds.split((1.0,))
        assert len(list(whole)) == 2 and list(ds.errors) == ["b.ly"]

    def test_warn(self, tmp_path):
        ds = FolderDataset(_write(tmp_path / "f", self.FILES), on_error="warn")
        with pytest.warns(RuntimeWarning, match="b.ly"):
            assert len(list(ds)) == 2

    def test_strict_errors_are_skipped_too(self, tmp_path):
        folder = _write(tmp_path / "f", {"a.ly": "{ c'4 }\n", "b.ly": "{ c'4 d'4\n"})
        ds = FolderDataset(folder, strict=True, on_error="skip")
        assert len(list(ds)) == 1 and list(ds.errors) == ["b.ly"]

    def test_bad_choices(self, tmp_path):
        with pytest.raises(ValueError):
            FolderDataset(tmp_path, on_error="ignore")
        with pytest.raises(ValueError):
            FolderDataset(tmp_path, movements="some")


class TestMovementsAndReadingOptions:
    def test_each_movement_is_an_item(self, tmp_path):
        folder = _write(
            tmp_path / "f",
            {
                "b.ly": "\\score { { c'1 } }\n\\score { { d'1 } }\n",
                "a.abc": "X:1\nT:A\nK:C\nC4|\n\nX:2\nT:B\nK:C\nD4|\n",
            },
        )
        ds = FolderDataset(folder, movements="all")
        assert ds.ids == ["a.abc#1", "a.abc#2", "b.ly#1", "b.ly#2"]
        assert [_pitches(d) for d in ds] == [[60], [62], [60], [62]]
        assert len(FolderDataset(folder)) == 2

    def test_lilypond_options_pass_through(self, tmp_path):
        lib = _write(tmp_path / "lib", {"notes.ily": "{ e'4 }\n"})
        folder = _write(tmp_path / "f", {"a.ly": '\\include "notes.ily"\n{ cs\'4 }\n'})
        plain = FolderDataset(folder, movements="all")
        assert [_pitches(d) for d in plain] == [[]]  # `cs` is no Dutch note name
        ds = FolderDataset(folder, movements="all", include_paths=[lib], language="english")
        assert [_pitches(d) for d in ds] == [[64], [61]]

    def test_quantize_reaches_the_midi_reader(self):
        ds = FolderDataset(MIDI_DIR, recursive=False, quantize=16)
        assert len(ds) > 0 and hasattr(ds[0], "to_score")


class TestCacheKeys:
    def test_defaults_share_entries_and_writes_are_atomic(self, tmp_path):
        cache = tmp_path / "cache"
        ds = FolderDataset(_small_ly_folder(tmp_path, 2), cache_dir=cache)
        ds.to_note_arrays()
        list(ds.iter_representation("note_array"))
        assert len(list(cache.glob("*.npy"))) == 2
        assert not list(cache.glob("*.tmp"))

    def test_changed_content_is_not_served_stale(self, tmp_path):
        folder = _write(tmp_path / "f", {"a.ly": "{ c'4 }\n"})
        ds = FolderDataset(folder, cache_dir=tmp_path / "cache")
        first = ds.to_note_arrays(resolution=24)[0]
        (folder / "a.ly").write_text("{ g'4 a'4 }\n")
        second = ds.to_note_arrays(resolution=24)[0]
        assert first.shape[0] == 1 and second.shape[0] == 2

    def test_subsets_use_the_cache(self, tmp_path):
        cache = tmp_path / "cache"
        ds = FolderDataset(_small_ly_folder(tmp_path, 3), cache_dir=cache)
        train, rest = ds.split((2, 1), seed=0)
        train.to_note_arrays(resolution=24)
        assert len(list(cache.glob("*.npy"))) == 2


class TestIds:
    def test_folder_ids_are_relative_paths(self, tmp_path):
        folder = _write(tmp_path / "f", {"x/a.ly": "{ c'1 }\n", "y/a.ly": "{ d'1 }\n"})
        ds = FolderDataset(folder)
        assert ds.ids == ["x/a.ly", "y/a.ly"]
        first, second = ds.split((1, 1), seed=3)
        assert sorted(first.ids + second.ids) == ds.ids

    def test_groups_stay_together(self, tmp_path):
        files = {f"{g}/{i}.ly": "{ c'1 }\n" for g in "abcdefgh" for i in range(3)}
        ds = FolderDataset(_write(tmp_path / "f", files))
        parts = ds.split((0.5, 0.25, 0.25), seed=1, groups=lambda item_id: item_id.split("/")[0])
        seen = [{i.split("/")[0] for i in part.ids} for part in parts]
        assert sum(len(s) for s in seen) == 8  # no folder in two parts
        assert [len(p) for p in parts] == [12, 6, 6]
        # Deterministic.
        again = ds.split((0.5, 0.25, 0.25), seed=1, groups=lambda item_id: item_id.split("/")[0])
        assert [p.ids for p in parts] == [p.ids for p in again]


class TestRecordsDataset:
    RECORDS = [
        {"id": "r1", "text": "{ c'4 }", "split": "train", "cluster": "k1"},
        {"id": "r2", "text": "{ d'4 }", "split": "train", "cluster": "k1"},
        {"id": "r3", "text": "{ e'4 }", "split": "test", "cluster": "k2"},
        {"id": "r4", "text": "\\score { { f'4 } }\n\\score { { g'4 } }", "split": "valid", "cluster": "k3"},
    ]

    def test_from_jsonl_keeps_ids_and_records(self, tmp_path):
        import json

        path = tmp_path / "scores.jsonl"
        path.write_text("".join(json.dumps(r) + "\n" for r in self.RECORDS) + "\n")
        ds = RecordsDataset.from_jsonl(path)
        assert ds.ids == ["r1", "r2", "r3", "r4"]
        assert ds.record(2)["cluster"] == "k2"
        assert [_pitches(d) for d in ds] == [[60], [62], [64], [65]]
        every = RecordsDataset.from_records(self.RECORDS, movements="all")
        assert every.ids[-2:] == ["r4#1", "r4#2"]
        assert every.record(4)["id"] == "r4"
        assert _pitches(every[4]) == [67]

    def test_own_splits_are_kept(self):
        ds = RecordsDataset.from_records(self.RECORDS, split_field="split")
        parts = ds.split()
        assert {k: v.ids for k, v in parts.items()} == {
            "train": ["r1", "r2"],
            "test": ["r3"],
            "valid": ["r4"],
        }
        assert list(ds.split(field="cluster")) == ["k1", "k2", "k3"]
        with pytest.raises(ValueError):
            ds.split((0.5, 0.5), field="split")
        # Ratios are a deliberate re-split.
        a, b = ds.split((0.5, 0.5), seed=0, groups="cluster")
        assert {r for p in (a, b) for r in p.ids} == {"r1", "r2", "r3", "r4"}
        assert {"r1", "r2"} <= set(a.ids) or {"r1", "r2"} <= set(b.ids)

    def test_other_text_formats(self):
        abc = {"id": "t", "text": "X:1\nK:C\nC4|\n\nX:2\nK:C\nE4|\n"}
        ds = RecordsDataset.from_records([abc], format="abc", movements="all")
        assert [_pitches(d) for d in ds] == [[60], [64]]
        with pytest.raises(ValueError):
            RecordsDataset.from_records([abc], format="midi")

    def test_bad_records(self):
        with pytest.raises(ValueError, match="duplicate"):
            RecordsDataset.from_records([{"id": 1, "text": "{ c'1 }"}, {"id": 1, "text": "{ d'1 }"}])
        with pytest.raises(ValueError, match="text"):
            RecordsDataset.from_records([{"id": 1}])

    def test_errors_and_cache(self, tmp_path):
        records = [{"id": "ok", "text": "{ c'4 }"}, {"id": "bad", "text": "{ c'4"}]
        ds = RecordsDataset.from_records(records, strict=True, on_error="skip", cache_dir=tmp_path / "c")
        assert len(ds.to_note_arrays()) == 1 and list(ds.errors) == ["bad"]
        assert len(list((tmp_path / "c").glob("*.npy"))) == 1


class TestReturnIds:
    def test_pytorch(self, tmp_path):
        pytest.importorskip("torch")
        ds = FolderDataset(_small_ly_folder(tmp_path, 3))
        tensor, item_id = ds.to_pytorch_dataset("note_array", return_ids=True, resolution=24)[0]
        assert item_id == ds.ids[0] and tensor.ndim == 2
        loader = ds.to_pytorch_dataloader("note_array", batch_size=3, return_ids=True)
        padded, lengths, ids = next(iter(loader))
        assert ids == ds.ids and padded.shape[0] == 3 and lengths.shape == (3,)

    def test_tensorflow(self, tmp_path):
        pytest.importorskip("tensorflow")
        ds = FolderDataset(_small_ly_folder(tmp_path, 2))
        items = list(ds.to_tensorflow_dataset("note_array", return_ids=True, resolution=24))
        assert [i.numpy().decode() for _, i in items] == ds.ids
        padded, lengths, ids = next(iter(ds.to_tensorflow_dataloader("note_array", batch_size=2, return_ids=True)))
        assert [i.decode() for i in ids.numpy()] == ds.ids and padded.shape[0] == 2
