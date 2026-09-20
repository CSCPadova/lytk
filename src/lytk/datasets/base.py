"""Dataset base classes, the folder dataset, splits, caching and ML adapters."""

from __future__ import annotations

import functools
import hashlib
import random
from pathlib import Path
from typing import Any, Callable, Iterator, Sequence

import numpy as np

from lytk import _core

# Recognised input extensions → loader producing a MusicDocument. Keep in step
# with `_SUPPORTED_EXTS` in `lytk.cli`; `test_datasets.py` asserts they agree.
_LY_EXTS = {".ly", ".ily"}
_XML_EXTS = {".xml", ".musicxml", ".mxl"}
_MIDI_EXTS = {".mid", ".midi"}
_ABC_EXTS = {".abc"}
_KERN_EXTS = {".krn", ".kern"}
SUPPORTED_EXTENSIONS = _LY_EXTS | _XML_EXTS | _MIDI_EXTS | _ABC_EXTS | _KERN_EXTS


def load_document(path: str | Path):
    """Load any supported music file as a :class:`MusicDocument`.

    LilyPond is parsed directly into the Layer-1 Music tree; every other format
    is parsed into a Score and lifted.
    """
    path = Path(path)
    ext = path.suffix.lower()
    if ext in _LY_EXTS:
        return _core.from_lilypond_music(str(path))
    if ext in _XML_EXTS:
        return _core.from_musicxml(str(path)).to_music_document()
    if ext in _MIDI_EXTS:
        return _core.from_midi(str(path)).to_music_document()
    if ext in _ABC_EXTS:
        return _core.from_abc(str(path)).to_music_document()
    if ext in _KERN_EXTS:
        return _core.from_humdrum(str(path)).to_music_document()
    raise ValueError(
        f"unsupported file extension: {ext!r} ({path}); "
        f"supported: {sorted(SUPPORTED_EXTENSIONS)}"
    )


# Representation converters: name → (function, default kwargs).
_REPRESENTATIONS: dict[str, Callable[..., np.ndarray]] = {
    "note_array": _core.to_note_array,
    "event_sequence": _core.to_event_sequence,
    "piano_roll": _core.to_piano_roll,
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

    Returns ``(padded, lengths)``. The lengths are returned rather than left to
    be inferred from ``pad_value``, because the padding value is not reserved:
    ``0`` is a legitimate event code, pitch and velocity, so trailing zeros are
    genuinely ambiguous. Use them to build a mask or to
    ``pack_padded_sequence``.

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

    tensors = [torch.as_tensor(np.asarray(item)) for item in batch]
    lengths = torch.tensor([t.shape[0] for t in tensors], dtype=torch.long)
    padded = pad_sequence(tensors, batch_first=True, padding_value=pad_value)
    return padded, lengths


class Dataset:
    """Abstract base: an indexable collection of :class:`MusicDocument` objects.

    Subclasses implement :meth:`__len__` and :meth:`__getitem__`. Everything
    else (representation conversion, metrics, splits, torch/tf adapters) is
    built on top of those two.
    """

    def __len__(self) -> int:  # pragma: no cover - abstract
        raise NotImplementedError

    def __getitem__(self, index: int):  # pragma: no cover - abstract
        raise NotImplementedError

    def __iter__(self) -> Iterator:
        for i in range(len(self)):
            yield self[i]

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
        _require_representation(representation)  # validate the name up front
        for i in range(len(self)):
            yield self._convert_item(i, representation, kwargs)

    def to_representation(self, representation: str, **kwargs: Any) -> list[np.ndarray]:
        """Eagerly convert every item to ``representation`` (one array per item).

        This materialises the whole dataset; use :meth:`iter_representation`
        for a memory-bounded stream over large corpora.
        """
        return list(self.iter_representation(representation, **kwargs))

    def _convert_item(self, index, representation, kwargs) -> np.ndarray:
        return _REPRESENTATIONS[representation](self[index], **kwargs)

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
        """Compute objective metrics for every item."""
        return [
            _core.compute_metrics(self[i], resolution, measure_resolution)
            for i in range(len(self))
        ]

    # -- splitting -----------------------------------------------------------

    def split(
        self,
        ratios: Sequence[float] = (0.8, 0.1, 0.1),
        seed: int = 0,
    ) -> tuple["Subset", ...]:
        """Deterministically partition into subsets by the given ratios.

        Returns one :class:`Subset` per ratio (e.g. train/val/test).
        """
        if not ratios or any(r < 0 for r in ratios):
            raise ValueError("ratios must be non-empty and non-negative")
        n = len(self)
        indices = list(range(n))
        random.Random(seed).shuffle(indices)
        total = float(sum(ratios))
        subsets: list[Subset] = []
        start = 0
        for r in ratios[:-1]:
            end = start + int(round(n * r / total))
            subsets.append(Subset(self, indices[start:end]))
            start = end
        # The last subset takes the remainder so every item is used.
        subsets.append(Subset(self, indices[start:]))
        return tuple(subsets)

    # -- ML framework adapters (lazy imports) --------------------------------

    def to_pytorch_dataset(self, representation: str = "note_array", **kwargs: Any):
        """Return a ``torch.utils.data.Dataset`` yielding tensors of the
        chosen representation. Requires PyTorch.

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

        class _LytkTorchDataset(TorchDataset):
            def __len__(self) -> int:
                return len(outer)

            def __getitem__(self, i: int):
                arr = outer._convert_item(i, representation, kwargs)
                return torch.as_tensor(np.asarray(arr))

        return _LytkTorchDataset()

    def to_tensorflow_dataset(self, representation: str = "note_array", **kwargs: Any):
        """Return a ``tf.data.Dataset`` of the chosen representation.
        Requires TensorFlow.

        The dataset streams items through a generator, converting (and
        caching, if a ``cache_dir`` is set) one item at a time rather than
        building the whole corpus up front.
        """
        try:
            import tensorflow as tf
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError("TensorFlow is required for to_tensorflow_dataset()") from exc

        _require_representation(representation)  # validate the name up front
        outer = self
        n = len(outer)

        def _gen():
            for i in range(n):
                yield np.asarray(outer._convert_item(i, representation, kwargs))

        if n:
            # Convert a single probe item to derive the tensor spec; this is
            # one item, not the whole dataset (and it warms the cache).
            probe = np.asarray(outer._convert_item(0, representation, kwargs))
            spec = tf.TensorSpec(shape=[None] * probe.ndim, dtype=probe.dtype)
        else:  # pragma: no cover - empty dataset
            spec = tf.TensorSpec(shape=[None], dtype=tf.int32)
        return tf.data.Dataset.from_generator(_gen, output_signature=spec)

    # -- ML framework data loaders (batching) --------------------------------

    def to_pytorch_dataloader(
        self,
        representation: str = "note_array",
        *,
        batch_size: int = 1,
        shuffle: bool = False,
        pad_value: float = 0.0,
        representation_kwargs: dict[str, Any] | None = None,
        **loader_kwargs: Any,
    ):
        """Return a ready-to-train ``torch.utils.data.DataLoader``.

        Like :meth:`to_pytorch_dataset` but batched, with :func:`pad_collate`
        wired in so that ``batch_size > 1`` works on ragged scores. Each batch
        is a ``(padded, lengths)`` tuple.

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
            representation, **(representation_kwargs or {})
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
        representation_kwargs: dict[str, Any] | None = None,
    ):
        """Return a batched ``tf.data.Dataset`` of ``(padded, lengths)`` tuples.

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
            representation, **(representation_kwargs or {})
        )
        with_lengths = base.map(lambda item: (item, tf.shape(item)[0]))
        if shuffle:
            # Bounded by the dataset size: items are converted lazily, so the
            # buffer holds arrays, not documents.
            with_lengths = with_lengths.shuffle(buffer_size=max(len(self), 1))
        # Cast through numpy: every representation is an integer dtype
        # (int64 events, int32 note arrays, uint8 rolls) and TensorFlow refuses
        # to build an int constant from the float default, unlike torch.
        dtype = base.element_spec.dtype
        pad_scalar = np.asarray(pad_value).astype(dtype.as_numpy_dtype)
        return with_lengths.padded_batch(
            batch_size,
            padding_values=(
                tf.constant(pad_scalar, dtype=dtype),
                tf.constant(0, dtype=tf.int32),
            ),
        )


class Subset(Dataset):
    """A view of a parent dataset restricted to a list of indices."""

    def __init__(self, dataset: Dataset, indices: Sequence[int]):
        self.dataset = dataset
        self.indices = list(indices)

    def __len__(self) -> int:
        return len(self.indices)

    def __getitem__(self, index: int):
        return self.dataset[self.indices[index]]


class FolderDataset(Dataset):
    """A dataset over the supported music files found in a directory.

    Documents are loaded lazily on access. When ``cache_dir`` is given,
    converted representations are cached to ``.npy`` files keyed by source
    file and conversion parameters.
    """

    def __init__(
        self,
        root: str | Path,
        recursive: bool = True,
        extensions: Sequence[str] | None = None,
        cache_dir: str | Path | None = None,
    ):
        self.root = Path(root)
        if not self.root.is_dir():
            raise NotADirectoryError(f"not a directory: {self.root}")
        exts = {e.lower() for e in (extensions or SUPPORTED_EXTENSIONS)}
        globber = self.root.rglob if recursive else self.root.glob
        self.paths: list[Path] = sorted(
            p for p in globber("*") if p.is_file() and p.suffix.lower() in exts
        )
        self.cache_dir = Path(cache_dir) if cache_dir is not None else None
        if self.cache_dir is not None:
            self.cache_dir.mkdir(parents=True, exist_ok=True)

    def __len__(self) -> int:
        return len(self.paths)

    def __getitem__(self, index: int):
        return load_document(self.paths[index])

    @property
    def filenames(self) -> list[str]:
        return [p.name for p in self.paths]

    def _path_hash(self, index: int) -> str:
        """Stable short hash identifying the source file.

        Derived from the path relative to ``root`` (falling back to the
        absolute path), so two files that share a stem but live in different
        subfolders — or differ only by extension, e.g. ``train/foo.ly`` and
        ``valid/foo.xml`` — get distinct cache keys instead of colliding.
        """
        path = self.paths[index]
        try:
            rel = path.relative_to(self.root)
        except ValueError:  # pragma: no cover - path outside root
            rel = path
        return hashlib.sha1(rel.as_posix().encode("utf-8")).hexdigest()[:16]

    def _convert_item(self, index, representation, kwargs) -> np.ndarray:
        # On-disk cache of converted representations (EFT2).
        convert = _REPRESENTATIONS[representation]
        if self.cache_dir is None:
            return convert(self[index], **kwargs)
        key = "_".join(f"{k}-{v}" for k, v in sorted(kwargs.items()))
        stem = self.paths[index].stem
        path_hash = self._path_hash(index)
        cache_file = self.cache_dir / f"{stem}__{representation}__{key}__{path_hash}.npy"
        if cache_file.exists():
            return np.load(cache_file, allow_pickle=False)
        arr = convert(self[index], **kwargs)
        np.save(cache_file, np.asarray(arr), allow_pickle=False)
        return arr
