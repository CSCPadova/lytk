"""Dataset utilities for symbolic-music ML pipelines (Epic F).

Provides a lazy :class:`Dataset` base, a :class:`FolderDataset` that loads a
directory of music files, representation converters (note-array / event /
piano-roll), train/val/test splits, on-disk caching, and lazy torch/tf
adapters.
"""

from __future__ import annotations

from lytk.datasets.base import Dataset, FolderDataset, load_document

__all__ = ["Dataset", "FolderDataset", "load_document"]
