"""Dataset utilities for symbolic-music ML pipelines (Epic F).

Provides a lazy :class:`Dataset` base, a :class:`FolderDataset` that loads a
directory of music files, representation converters (note-array / event /
piano-roll), train/val/test splits, on-disk caching, and torch/TensorFlow
adapters — datasets and padded data loaders — imported lazily so neither
framework is needed unless used.
"""

from __future__ import annotations

from lytk.datasets.base import (
    SUPPORTED_EXTENSIONS,
    Dataset,
    FolderDataset,
    Subset,
    load_document,
    pad_collate,
)

__all__ = [
    "SUPPORTED_EXTENSIONS",
    "Dataset",
    "FolderDataset",
    "Subset",
    "load_document",
    "pad_collate",
]
