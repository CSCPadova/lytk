"""Dataset utilities for symbolic-music ML pipelines (Epic F).

Provides a lazy :class:`Dataset` base, a :class:`FolderDataset` that loads a
directory of music files, a :class:`RecordsDataset` over records holding
music as text (JSON Lines), representation converters (note-array / event /
piano-roll), train/val/test splits (by ratio, by group, or the records' own),
on-disk caching, and torch/TensorFlow adapters — datasets and padded data
loaders — imported lazily so neither framework is needed unless used.
"""

from __future__ import annotations

from lytk.datasets.base import (
    SUPPORTED_EXTENSIONS,
    TEXT_FORMATS,
    Dataset,
    FolderDataset,
    RecordsDataset,
    Subset,
    load_document,
    pad_collate,
)

__all__ = [
    "SUPPORTED_EXTENSIONS",
    "TEXT_FORMATS",
    "Dataset",
    "FolderDataset",
    "RecordsDataset",
    "Subset",
    "load_document",
    "pad_collate",
]
