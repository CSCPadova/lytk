"""Dataset base classes, the folder and records datasets, splits, caching and
ML adapters."""

from __future__ import annotations

import functools
import hashlib
import json
import os
import random
import tempfile
import warnings
from pathlib import Path
from typing import Any, Callable, Hashable, Iterable, Iterator, Sequence

import numpy as np

from lytk import _core

# Recognised input extensions → format. Keep in step with `_EXT_FORMAT` in
# `lytk.cli`; `test_datasets.py` asserts they agree.
_LY_EXTS = {".ly", ".ily"}
_XML_EXTS = {".xml", ".musicxml", ".mxl"}
_MIDI_EXTS = {".mid", ".midi"}
_ABC_EXTS = {".abc"}
_KERN_EXTS = {".krn", ".kern"}
SUPPORTED_EXTENSIONS = _LY_EXTS | _XML_EXTS | _MIDI_EXTS | _ABC_EXTS | _KERN_EXTS
_FORMAT_OF_EXT = {
    **dict.fromkeys(_LY_EXTS, "lilypond"),
    **dict.fromkeys(_XML_EXTS, "musicxml"),
    **dict.fromkeys(_MIDI_EXTS, "midi"),
    **dict.fromkeys(_ABC_EXTS, "abc"),
    **dict.fromkeys(_KERN_EXTS, "humdrum"),
}
# Formats a record's text can hold (MIDI is binary).
TEXT_FORMATS = ("lilypond", "musicxml", "abc", "humdrum")

_ON_ERROR = ("raise", "skip", "warn")
_MOVEMENTS = ("first", "all")
# What reading an item can raise for a bad item: lytk's errors (ParseError is
# also a ValueError), I/O, and a value no reader accepts.
_READ_ERRORS = (_core.LytkError, OSError, ValueError)


def _check_choice(name: str, value: str, choices: Sequence[str]) -> str:
    if value not in choices:
        raise ValueError(f"{name} must be one of {list(choices)}, not {value!r}")
    return value


def _read(
    source: str | Path,
    fmt: str,
    *,
    from_text: bool,
    movements: str,
    language: str | None,
    include_paths: Sequence[str] | None,
    strict: bool,
    quantize: int | None,
) -> list:
    """The documents of a file (``from_text=False``) or of text: its first
    movement, or all of them. The LilyPond options apply to LilyPond only,
    ``quantize`` to MIDI only."""
    every = movements == "all"
    src = str(source)
    if fmt == "lilypond":
        opts: dict[str, Any] = {"language": language, "strict": strict}
        if include_paths is not None:
            opts["include_paths"] = [str(p) for p in include_paths]
        if from_text:
            if every:
                scores = _core.from_lilypond_movements_string(src, **opts)
                return [s.to_music_document() for s in scores]
            return [_core.from_lilypond_music_string(src, **opts)]
        if every:
            return _core.from_lilypond_music_movements(src, **opts)
        return [_core.from_lilypond_music(src, **opts)]
    if fmt == "abc":
        if every:
            tunes = _core.from_abc_tunes_string(src) if from_text else _core.from_abc_tunes(src)
            return [t.to_music_document() for t in tunes]
        score = _core.from_abc_string(src) if from_text else _core.from_abc(src)
    elif fmt == "musicxml":
        score = _core.from_musicxml_string(src) if from_text else _core.from_musicxml(src)
    elif fmt == "humdrum":
        score = _core.from_humdrum_string(src) if from_text else _core.from_humdrum(src)
    elif fmt == "midi" and not from_text:
        score = _core.from_midi(src, quantize=quantize)
    else:
        raise ValueError(f"unsupported format: {fmt!r}")
    return [score.to_music_document()]


def load_document(
    path: str | Path,
    *,
    language: str | None = None,
    include_paths: Sequence[str] | None = None,
    strict: bool = False,
    quantize: int | None = None,
):
    """Load any supported music file as a :class:`MusicDocument` (its first
    movement).

    LilyPond is parsed directly into the Layer-1 Music tree; every other format
    is parsed into a Score and lifted. ``language``, ``include_paths`` and
    ``strict`` are passed to the LilyPond reader, ``quantize`` to the MIDI one.
    """
    path = Path(path)
    ext = path.suffix.lower()
    if ext not in _FORMAT_OF_EXT:
        raise ValueError(
            f"unsupported file extension: {ext!r} ({path}); "
            f"supported: {sorted(SUPPORTED_EXTENSIONS)}"
        )
    (doc,) = _read(
        path,
        _FORMAT_OF_EXT[ext],
        from_text=False,
        movements="first",
        language=language,
        include_paths=include_paths,
        strict=strict,
        quantize=quantize,
    )
    return doc


# Representation converters, and their defaults: the cache key normalizes
# arguments with them, so `to_note_arrays()` and
# `iter_representation("note_array")` share cache entries.
_REPRESENTATIONS: dict[str, Callable[..., np.ndarray]] = {
    "note_array": _core.to_note_array,
    "event_sequence": _core.to_event_sequence,
    "piano_roll": _core.to_piano_roll,
}
_DEFAULTS: dict[str, dict[str, Any]] = {
    "note_array": {"resolution": 480},
    "event_sequence": {
        "resolution": 480,
        "max_time_shift": 100,
        "velocity_bins": 32,
        "encode_velocity": True,
    },
    "piano_roll": {"resolution": 480, "encode_velocity": True},
}


def _require_representation(representation: str) -> Callable[..., np.ndarray]:
    """Return the converter for ``representation`` or raise ``ValueError``."""
    if representation not in _REPRESENTATIONS:
        raise ValueError(
            f"unknown representation {representation!r}; "
            f"choose from {sorted(_REPRESENTATIONS)}"
        )
    return _REPRESENTATIONS[representation]


def pad_collate(batch: Sequence[Any], pad_value: float = 0.0):
    """Collate variable-length representation tensors into one padded batch.

    Every representation is ragged along its first axis — note arrays are
    ``(n_notes, 4)``, event sequences ``(n_events,)``, piano rolls
    ``(n_frames, 128)`` — and the number of notes/events/frames differs per
    score, so ``torch``'s default collate (which stacks) raises
    ``RuntimeError: stack expects each tensor to be equal size``. This pads the
    first axis to the longest item in the batch.

    Returns ``(padded, lengths)``, or ``(padded, lengths, ids)`` when the items
    are ``(tensor, id)`` pairs (a dataset made with ``return_ids=True``). The
    lengths are returned rather than left to be inferred from ``pad_value``,
    because the padding value is not reserved: ``0`` is a legitimate event
    code, pitch and velocity, so trailing zeros are genuinely ambiguous. Use
    them to build a mask or to ``pack_padded_sequence``.

    Suitable as a ``collate_fn`` for any ``torch.utils.data.DataLoader``:

    ```python
    from functools import partial
    DataLoader(ds, batch_size=8, collate_fn=partial(pad_collate, pad_value=-1))
    ```
    """
    try:
        import torch
        from torch.nn.utils.rnn import pad_sequence
    except ImportError as exc:  # pragma: no cover - optional dep
        raise ImportError("PyTorch is required for pad_collate()") from exc

    with_ids = bool(batch) and isinstance(batch[0], tuple)
    items = [item[0] for item in batch] if with_ids else batch
    tensors = [torch.as_tensor(np.asarray(item)) for item in items]
    lengths = torch.tensor([t.shape[0] for t in tensors], dtype=torch.long)
    padded = pad_sequence(tensors, batch_first=True, padding_value=pad_value)
    if with_ids:
        return padded, lengths, [item[1] for item in batch]
    return padded, lengths


class Dataset:
    """Abstract base: an indexable collection of :class:`MusicDocument` objects.

    Subclasses implement :meth:`__len__` and :meth:`__getitem__`. Everything
    else (representation conversion, metrics, splits, torch/tf adapters) is
    built on top of those two.

    ``on_error`` says what iterating does with an item that cannot be read:
    ``"raise"`` (the default) raises, ``"skip"`` leaves it out and records it
    in :attr:`errors`, ``"warn"`` does the same and warns. Iteration covers
    ``for doc in ds``, :meth:`iter_representation`, :meth:`to_representation`,
    :meth:`metrics` and the TensorFlow adapters; indexing (``ds[i]``, the
    torch adapters) always raises for such an item.
    """

    on_error: str = "raise"

    def __len__(self) -> int:  # pragma: no cover - abstract
        raise NotImplementedError

    def __getitem__(self, index: int):  # pragma: no cover - abstract
        raise NotImplementedError

    @property
    def ids(self) -> list[str]:
        """An identifier per item, in order (here the index as a string)."""
        return [str(i) for i in range(len(self))]

    @property
    def errors(self) -> dict[str, str]:
        """The items iteration skipped: id → ``"ErrorType: message"``."""
        return self.__dict__.setdefault("_errors", {})

    def _failed(self, index: int, error: Exception) -> None:
        """Handle an item that cannot be read, as ``on_error`` says."""
        if self.on_error == "raise":
            raise error
        item_id = self.ids[index]
        message = f"{type(error).__name__}: {error}"
        self.errors[item_id] = message
        if self.on_error == "warn":
            warnings.warn(f"lytk: skipped {item_id}: {message}", RuntimeWarning, stacklevel=4)

    def _each(self, index: int, fn: Callable[[int], Any]) -> Iterator[Any]:
        """``fn(index)``, or nothing when the item cannot be read."""
        try:
            yield fn(index)
        except _READ_ERRORS as error:
            self._failed(index, error)

    def __iter__(self) -> Iterator:
        for i in range(len(self)):
            yield from self._each(i, self.__getitem__)

    # -- representation conversion -------------------------------------------

    def iter_representation(
        self, representation: str, **kwargs: Any
    ) -> Iterator[np.ndarray]:
        """Lazily convert each item to ``representation``, one array at a time.

        ``representation`` is one of ``"note_array"``, ``"event_sequence"``,
        ``"piano_roll"``; ``**kwargs`` are forwarded to the converter. Unlike
        :meth:`to_representation`, this never materialises the whole dataset in
        memory, so it scales to large folders. When a ``cache_dir`` is set the
        per-item cache is consulted as each item is produced.
        """
        for _, arr in self._iter_converted(representation, kwargs):
            yield arr

    def _iter_converted(
        self, representation: str, kwargs: dict[str, Any]
    ) -> Iterator[tuple[int, np.ndarray]]:
        """(index, array) for every item that can be read."""
        _require_representation(representation)  # validate the name up front
        for i in range(len(self)):
            for arr in self._each(i, lambda j: self._convert_item(j, representation, kwargs)):
                yield i, arr

    def to_representation(self, representation: str, **kwargs: Any) -> list[np.ndarray]:
        """Eagerly convert every item to ``representation`` (one array per item
        that can be read).

        This materialises the whole dataset; use :meth:`iter_representation`
        for a memory-bounded stream over large corpora.
        """
        return list(self.iter_representation(representation, **kwargs))

    def _cache_dir(self) -> Path | None:
        """Where converted items are cached; ``None`` for no cache."""
        return None

    def _cache_source(self, index: int) -> bytes:  # pragma: no cover - abstract
        """What item ``index`` is read from, and how, for the cache key."""
        raise NotImplementedError

    def _convert_item(self, index, representation, kwargs) -> np.ndarray:
        convert = _REPRESENTATIONS[representation]
        cache_dir = self._cache_dir()
        if cache_dir is None:
            return convert(self[index], **kwargs)
        # The key: the item's source (content and reading options), lytk's
        # version and the converter's arguments with their defaults filled in.
        params = json.dumps(
            {
                "lytk": _core.__version__,
                "representation": representation,
                "kwargs": {**_DEFAULTS.get(representation, {}), **kwargs},
            },
            sort_keys=True,
        )
        digest = hashlib.sha256(params.encode("utf-8") + b"\0" + self._cache_source(index))
        cache_file = cache_dir / f"{digest.hexdigest()}.npy"
        if cache_file.exists():
            return np.load(cache_file, allow_pickle=False)
        arr = np.asarray(convert(self[index], **kwargs))
        # Written to a temporary file and renamed: a reader never sees half a
        # file, and two writers of one entry write the same bytes.
        fd, tmp = tempfile.mkstemp(dir=cache_dir, suffix=".npy.tmp")
        try:
            with os.fdopen(fd, "wb") as out:
                np.save(out, arr, allow_pickle=False)
            os.replace(tmp, cache_file)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise
        return arr

    def to_note_arrays(self, resolution: int = 480) -> list[np.ndarray]:
        return self.to_representation("note_array", resolution=resolution)

    def to_event_sequences(self, **kwargs: Any) -> list[np.ndarray]:
        return self.to_representation("event_sequence", **kwargs)

    def to_pianorolls(self, resolution: int = 480, encode_velocity: bool = True) -> list[np.ndarray]:
        return self.to_representation(
            "piano_roll", resolution=resolution, encode_velocity=encode_velocity
        )

    def metrics(
        self, resolution: int = 480, measure_resolution: int | None = None
    ) -> list[dict[str, Any]]:
        """Compute objective metrics for every item that can be read."""
        return [
            _core.compute_metrics(doc, resolution, measure_resolution)
            for doc in self
        ]

    # -- splitting -----------------------------------------------------------

    def _group_keys(self, groups: Any) -> list[Hashable]:
        """The group of each item: ``groups`` is one key per item, or a
        function of the item's id."""
        if callable(groups):
            return [groups(item_id) for item_id in self.ids]
        if isinstance(groups, str):
            raise TypeError("groups is a field name only for a RecordsDataset")
        keys = list(groups)
        if len(keys) != len(self):
            raise ValueError(f"groups has {len(keys)} keys for {len(self)} items")
        return keys

    def split(
        self,
        ratios: Sequence[float] = (0.8, 0.1, 0.1),
        seed: int = 0,
        *,
        groups: Any = None,
    ) -> tuple["Subset", ...]:
        """Deterministically partition into subsets by the given ratios.

        Returns one :class:`Subset` per ratio (e.g. train/val/test). With
        ``groups`` (one key per item, or a function of the item's id), items of
        a group land in the same subset: the groups are shuffled and each goes
        to the subset furthest below its share of the items.
        """
        if not ratios or any(r < 0 for r in ratios):
            raise ValueError("ratios must be non-empty and non-negative")
        n = len(self)
        total = float(sum(ratios))
        if groups is None:
            indices = list(range(n))
            random.Random(seed).shuffle(indices)
            subsets: list[Subset] = []
            start = 0
            for r in ratios[:-1]:
                end = start + int(round(n * r / total))
                subsets.append(Subset(self, indices[start:end]))
                start = end
            # The last subset takes the remainder so every item is used.
            subsets.append(Subset(self, indices[start:]))
            return tuple(subsets)
        members: dict[Hashable, list[int]] = {}
        for i, key in enumerate(self._group_keys(groups)):
            members.setdefault(key, []).append(i)
        order = list(members)
        random.Random(seed).shuffle(order)
        targets = [n * r / total for r in ratios]
        parts: list[list[int]] = [[] for _ in ratios]
        for key in order:
            # The subset furthest below its share (the first on a tie).
            best = max(range(len(parts)), key=lambda k: (targets[k] - len(parts[k]), -k))
            parts[best].extend(members[key])
        return tuple(Subset(self, part) for part in parts)

    # -- ML framework adapters (lazy imports) --------------------------------

    def to_pytorch_dataset(
        self, representation: str = "note_array", *, return_ids: bool = False, **kwargs: Any
    ):
        """Return a ``torch.utils.data.Dataset`` yielding tensors of the
        chosen representation, or ``(tensor, id)`` pairs with ``return_ids``.
        Requires PyTorch.

        Conversion is lazy: each item is converted (and cached, if a
        ``cache_dir`` is set) only when it is indexed, so constructing the
        dataset does not materialise the whole corpus in memory.
        """
        try:
            import torch
            from torch.utils.data import Dataset as TorchDataset
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError("PyTorch is required for to_pytorch_dataset()") from exc

        _require_representation(representation)  # validate the name up front
        outer = self
        ids = self.ids if return_ids else None

        class _LytkTorchDataset(TorchDataset):
            def __len__(self) -> int:
                return len(outer)

            def __getitem__(self, i: int):
                arr = outer._convert_item(i, representation, kwargs)
                tensor = torch.as_tensor(np.asarray(arr))
                return (tensor, ids[i]) if ids is not None else tensor

        return _LytkTorchDataset()

    def to_tensorflow_dataset(
        self, representation: str = "note_array", *, return_ids: bool = False, **kwargs: Any
    ):
        """Return a ``tf.data.Dataset`` of the chosen representation, or of
        ``(tensor, id)`` pairs with ``return_ids``. Requires TensorFlow.

        The dataset streams items through a generator, converting (and
        caching, if a ``cache_dir`` is set) one item at a time rather than
        building the whole corpus up front. Items that cannot be read are
        skipped or raise as ``on_error`` says.
        """
        try:
            import tensorflow as tf
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError("TensorFlow is required for to_tensorflow_dataset()") from exc

        _require_representation(representation)  # validate the name up front
        outer = self
        ids = self.ids if return_ids else None

        def _gen():
            for i, arr in outer._iter_converted(representation, kwargs):
                yield (np.asarray(arr), ids[i]) if ids is not None else np.asarray(arr)

        # One probe item derives the tensor spec: one item, not the whole
        # dataset (and it warms the cache).
        probe = next(self._iter_converted(representation, kwargs), None)
        if probe is not None:
            arr = np.asarray(probe[1])
            spec = tf.TensorSpec(shape=[None] * arr.ndim, dtype=arr.dtype)
        else:  # pragma: no cover - empty dataset
            spec = tf.TensorSpec(shape=[None], dtype=tf.int32)
        if ids is not None:
            spec = (spec, tf.TensorSpec(shape=(), dtype=tf.string))
        return tf.data.Dataset.from_generator(_gen, output_signature=spec)

    # -- ML framework data loaders (batching) --------------------------------

    def to_pytorch_dataloader(
        self,
        representation: str = "note_array",
        *,
        batch_size: int = 1,
        shuffle: bool = False,
        pad_value: float = 0.0,
        return_ids: bool = False,
        representation_kwargs: dict[str, Any] | None = None,
        **loader_kwargs: Any,
    ):
        """Return a ready-to-train ``torch.utils.data.DataLoader``.

        Like :meth:`to_pytorch_dataset` but batched, with :func:`pad_collate`
        wired in so that ``batch_size > 1`` works on ragged scores. Each batch
        is a ``(padded, lengths)`` tuple, ``(padded, lengths, ids)`` with
        ``return_ids``.

        Unlike the other methods on this class, ``**kwargs`` here go to the
        ``DataLoader`` (``num_workers``, ``pin_memory``, ``drop_last``, …);
        arguments for the representation converter go in
        ``representation_kwargs``. Pass your own ``collate_fn`` to override the
        padding behaviour.

        ```python
        train, val, test = FolderDataset("corpus/").split()
        loader = train.to_pytorch_dataloader(
            "event_sequence", batch_size=32, shuffle=True, num_workers=4
        )
        for events, lengths in loader:
            ...
        ```
        """
        try:
            from torch.utils.data import DataLoader
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError(
                "PyTorch is required for to_pytorch_dataloader()"
            ) from exc

        dataset = self.to_pytorch_dataset(
            representation, return_ids=return_ids, **(representation_kwargs or {})
        )
        loader_kwargs.setdefault(
            "collate_fn", functools.partial(pad_collate, pad_value=pad_value)
        )
        return DataLoader(
            dataset, batch_size=batch_size, shuffle=shuffle, **loader_kwargs
        )

    def to_tensorflow_dataloader(
        self,
        representation: str = "note_array",
        *,
        batch_size: int = 1,
        shuffle: bool = False,
        pad_value: float = 0.0,
        return_ids: bool = False,
        representation_kwargs: dict[str, Any] | None = None,
    ):
        """Return a batched ``tf.data.Dataset`` of ``(padded, lengths)`` tuples,
        ``(padded, lengths, ids)`` with ``return_ids``.

        The TensorFlow counterpart of :meth:`to_pytorch_dataloader`: pads each
        batch along the ragged first axis with ``padded_batch`` and carries the
        true lengths alongside, for the same reason as :func:`pad_collate`
        (``0`` is a valid value, so padding is not self-identifying).

        ```python
        loader = train.to_tensorflow_dataloader("piano_roll", batch_size=16)
        for rolls, lengths in loader:
            ...
        ```
        """
        try:
            import tensorflow as tf
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError(
                "TensorFlow is required for to_tensorflow_dataloader()"
            ) from exc

        base = self.to_tensorflow_dataset(
            representation, return_ids=return_ids, **(representation_kwargs or {})
        )
        if return_ids:
            dtype = base.element_spec[0].dtype
            with_lengths = base.map(lambda item, item_id: (item, tf.shape(item)[0], item_id))
        else:
            dtype = base.element_spec.dtype
            with_lengths = base.map(lambda item: (item, tf.shape(item)[0]))
        if shuffle:
            # Bounded by the dataset size: items are converted lazily, so the
            # buffer holds arrays, not documents.
            with_lengths = with_lengths.shuffle(buffer_size=max(len(self), 1))
        # Cast through numpy: every representation is an integer dtype
        # (int64 events, int32 note arrays, uint8 rolls) and TensorFlow refuses
        # to build an int constant from the float default, unlike torch.
        pad_scalar = np.asarray(pad_value).astype(dtype.as_numpy_dtype)
        padding: tuple = (
            tf.constant(pad_scalar, dtype=dtype),
            tf.constant(0, dtype=tf.int32),
        )
        if return_ids:
            padding += (tf.constant("", dtype=tf.string),)
        return with_lengths.padded_batch(batch_size, padding_values=padding)


class Subset(Dataset):
    """A view of a parent dataset restricted to a list of indices. Its items,
    ids, cache and errors are the parent's."""

    def __init__(self, dataset: Dataset, indices: Sequence[int]):
        self.dataset = dataset
        self.indices = list(indices)

    def __len__(self) -> int:
        return len(self.indices)

    def __getitem__(self, index: int):
        return self.dataset[self.indices[index]]

    @property
    def ids(self) -> list[str]:
        parent = self.dataset.ids
        return [parent[i] for i in self.indices]

    @property
    def on_error(self) -> str:  # type: ignore[override]
        return self.dataset.on_error

    @property
    def errors(self) -> dict[str, str]:
        return self.dataset.errors

    def _failed(self, index: int, error: Exception) -> None:
        self.dataset._failed(self.indices[index], error)

    def _convert_item(self, index, representation, kwargs) -> np.ndarray:
        return self.dataset._convert_item(self.indices[index], representation, kwargs)


class _SourceDataset(Dataset):
    """What the folder and records datasets share: reading options, the
    movements they make items of, and the cache."""

    def _setup(
        self,
        *,
        cache_dir: str | Path | None,
        on_error: str,
        movements: str,
        language: str | None,
        include_paths: Sequence[str | Path] | None,
        strict: bool,
        quantize: int | None,
    ) -> None:
        self.on_error = _check_choice("on_error", on_error, _ON_ERROR)
        self.movements = _check_choice("movements", movements, _MOVEMENTS)
        self._read_opts = {
            "language": language,
            "include_paths": None if include_paths is None else [str(p) for p in include_paths],
            "strict": strict,
            "quantize": quantize,
        }
        self.cache_dir = Path(cache_dir) if cache_dir is not None else None
        if self.cache_dir is not None:
            self.cache_dir.mkdir(parents=True, exist_ok=True)
        # With movements="all": (source index, movement index) per item.
        self._items: list[tuple[int, int]] | None = None
        self._last: tuple[int, list] | None = None

    # A source is a file or a record; subclasses say how many there are, what
    # identifies one, and how to read it.
    def _n_sources(self) -> int:  # pragma: no cover - abstract
        raise NotImplementedError

    def _source_id(self, source: int) -> str:  # pragma: no cover - abstract
        raise NotImplementedError

    def _source_bytes(self, source: int) -> bytes:  # pragma: no cover - abstract
        raise NotImplementedError

    def _read_source(self, source: int, movements: str) -> list:  # pragma: no cover
        raise NotImplementedError

    def _documents(self, source: int) -> list:
        """Every movement of a source (the last source read is kept)."""
        if self._last is None or self._last[0] != source:
            self._last = (source, self._read_source(source, "all"))
        return self._last[1]

    def _item_list(self) -> list[tuple[int, int]]:
        """With movements="all", every (source, movement), reading each source
        once; a source that cannot be read makes no item (or raises, as
        ``on_error`` says)."""
        if self._items is None:
            items: list[tuple[int, int]] = []
            for s in range(self._n_sources()):
                try:
                    count = len(self._documents(s))
                except _READ_ERRORS as error:
                    if self.on_error == "raise":
                        raise
                    message = f"{type(error).__name__}: {error}"
                    self.errors[self._source_id(s)] = message
                    if self.on_error == "warn":
                        warnings.warn(
                            f"lytk: skipped {self._source_id(s)}: {message}",
                            RuntimeWarning,
                            stacklevel=3,
                        )
                    continue
                items.extend((s, k) for k in range(count))
            self._items = items
        return self._items

    def __len__(self) -> int:
        if self.movements == "first":
            return self._n_sources()
        return len(self._item_list())

    def __getitem__(self, index: int):
        if self.movements == "first":
            (doc,) = self._read_source(range(self._n_sources())[index], "first")
            return doc
        source, k = self._item_list()[index]
        return self._documents(source)[k]

    @property
    def ids(self) -> list[str]:
        """The source's id per item; with ``movements="all"``, followed by
        ``#`` and the movement's number, from 1."""
        if self.movements == "first":
            return [self._source_id(s) for s in range(self._n_sources())]
        return [f"{self._source_id(s)}#{k + 1}" for s, k in self._item_list()]

    def _cache_dir(self) -> Path | None:
        return self.cache_dir

    def _cache_source(self, index: int) -> bytes:
        if self.movements == "first":
            source, movement = range(self._n_sources())[index], 0
        else:
            source, movement = self._item_list()[index]
        options = json.dumps(
            {**self._read_opts, "movements": self.movements, "movement": movement},
            sort_keys=True,
        )
        return options.encode("utf-8") + b"\0" + self._source_bytes(source)


class FolderDataset(_SourceDataset):
    """A dataset over the supported music files found in a directory.

    Documents are loaded lazily on access; an item's id is its path relative to
    ``root``. ``movements="all"`` makes each movement of a file (each
    ``\\score`` of a LilyPond file, each tune of an ABC file) an item.
    ``language``, ``include_paths`` and ``strict`` are passed to the LilyPond
    reader, ``quantize`` to the MIDI one. ``on_error`` is described in
    :class:`Dataset`. When ``cache_dir`` is given, converted representations
    are cached to ``.npy`` files keyed by the file's content, how it is read,
    the lytk version and the conversion parameters (the content of included
    files is not part of the key).
    """

    def __init__(
        self,
        root: str | Path,
        recursive: bool = True,
        extensions: Sequence[str] | None = None,
        cache_dir: str | Path | None = None,
        *,
        on_error: str = "raise",
        movements: str = "first",
        language: str | None = None,
        include_paths: Sequence[str | Path] | None = None,
        strict: bool = False,
        quantize: int | None = None,
    ):
        self.root = Path(root)
        if not self.root.is_dir():
            raise NotADirectoryError(f"not a directory: {self.root}")
        exts = {e.lower() for e in (extensions or SUPPORTED_EXTENSIONS)}
        globber = self.root.rglob if recursive else self.root.glob
        self.paths: list[Path] = sorted(
            p for p in globber("*") if p.is_file() and p.suffix.lower() in exts
        )
        self._rel = [p.relative_to(self.root).as_posix() for p in self.paths]
        self._setup(
            cache_dir=cache_dir,
            on_error=on_error,
            movements=movements,
            language=language,
            include_paths=include_paths,
            strict=strict,
            quantize=quantize,
        )

    @property
    def filenames(self) -> list[str]:
        return [p.name for p in self.paths]

    def _n_sources(self) -> int:
        return len(self.paths)

    def _source_id(self, source: int) -> str:
        return self._rel[source]

    def _source_bytes(self, source: int) -> bytes:
        return self.paths[source].read_bytes()

    def _read_source(self, source: int, movements: str) -> list:
        path = self.paths[source]
        fmt = _FORMAT_OF_EXT.get(path.suffix.lower())
        if fmt is None:
            raise ValueError(f"unsupported file extension: {path.suffix!r} ({path})")
        return _read(path, fmt, from_text=False, movements=movements, **self._read_opts)


class RecordsDataset(_SourceDataset):
    """A dataset over records (dicts) holding music as text: each record's
    ``text_field`` in ``format`` (``"lilypond"``, ``"musicxml"``, ``"abc"`` or
    ``"humdrum"``), identified by its ``id_field``.

    Items keep their record (:meth:`record`), and their id is the record's
    (followed by ``#`` and the movement's number with ``movements="all"``).
    With ``split_field``, the records carry their own splits: :meth:`split`
    returns them instead of re-shuffling by ratio. The other options are as in
    :class:`FolderDataset`; the cache is keyed by the record's text.
    """

    def __init__(
        self,
        records: Iterable[dict[str, Any]],
        *,
        text_field: str = "text",
        id_field: str = "id",
        format: str = "lilypond",
        split_field: str | None = None,
        cache_dir: str | Path | None = None,
        on_error: str = "raise",
        movements: str = "first",
        language: str | None = None,
        include_paths: Sequence[str | Path] | None = None,
        strict: bool = False,
    ):
        self.format = _check_choice("format", format, TEXT_FORMATS)
        self.text_field, self.id_field, self.split_field = text_field, id_field, split_field
        self.records: list[dict[str, Any]] = list(records)
        seen: set[str] = set()
        for n, record in enumerate(self.records):
            for field in (text_field, id_field):
                if field not in record:
                    raise ValueError(f"record {n} has no {field!r} field")
            if not isinstance(record[text_field], str):
                raise ValueError(f"record {n}: {text_field!r} is not a string")
            record_id = str(record[id_field])
            if record_id in seen:
                raise ValueError(f"duplicate record id {record_id!r}")
            seen.add(record_id)
        self._setup(
            cache_dir=cache_dir,
            on_error=on_error,
            movements=movements,
            language=language,
            include_paths=include_paths,
            strict=strict,
            quantize=None,
        )

    @classmethod
    def from_records(cls, records: Iterable[dict[str, Any]], **kwargs: Any) -> "RecordsDataset":
        """A dataset over an iterable of records (keywords as the class)."""
        return cls(records, **kwargs)

    @classmethod
    def from_jsonl(cls, path: str | Path, **kwargs: Any) -> "RecordsDataset":
        """A dataset over a JSON Lines file, one record per line (keywords as
        the class)."""
        with open(path, encoding="utf-8") as lines:
            return cls((json.loads(line) for line in lines if line.strip()), **kwargs)

    def record(self, index: int) -> dict[str, Any]:
        """The record item ``index`` comes from."""
        if self.movements == "first":
            return self.records[index]
        return self.records[self._item_list()[index][0]]

    def _n_sources(self) -> int:
        return len(self.records)

    def _source_id(self, source: int) -> str:
        return str(self.records[source][self.id_field])

    def _source_bytes(self, source: int) -> bytes:
        return f"{self.format}\0{self.records[source][self.text_field]}".encode("utf-8")

    def _read_source(self, source: int, movements: str) -> list:
        text = self.records[source][self.text_field]
        return _read(text, self.format, from_text=True, movements=movements, **self._read_opts)

    def _group_keys(self, groups: Any) -> list[Hashable]:
        if isinstance(groups, str):
            return [self.record(i).get(groups) for i in range(len(self))]
        return super()._group_keys(groups)

    def split(  # type: ignore[override]
        self,
        ratios: Sequence[float] | None = None,
        seed: int = 0,
        *,
        groups: Any = None,
        field: str | None = None,
    ):
        """The records' own splits, or a ratio split.

        With ``field`` (or ``split_field`` given to the dataset, when no
        ``ratios`` are), returns ``{value: Subset}`` for each value of that
        field, in order of first appearance: a curated, decontaminated split
        stays as it is. Otherwise splits by ``ratios`` (default
        ``(0.8, 0.1, 0.1)``) as :meth:`Dataset.split` does; ``groups`` may name
        a field whose equal values stay together.
        """
        if field is None and ratios is None:
            field = self.split_field
        if field is not None:
            if ratios is not None:
                raise ValueError("pass ratios or field, not both")
            parts: dict[Any, list[int]] = {}
            for i in range(len(self)):
                parts.setdefault(self.record(i).get(field), []).append(i)
            return {value: Subset(self, indices) for value, indices in parts.items()}
        return super().split(ratios if ratios is not None else (0.8, 0.1, 0.1), seed, groups=groups)
