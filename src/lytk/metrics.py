"""Generation-evaluation metrics: JS-similarity and Fréchet Music Distance.

Ported from the LilyBench reference implementation. Both are *corpus-level*
metrics comparing a model's outputs against a reference set. Heavy deps
(``scipy`` for JS, plus ``torch``/``transformers`` for FMD) are imported lazily,
so this module imports fine without them — install via ``pip install lytk[eval]``.

JS-similarity uses lytk's own :func:`lytk.compute_metrics` for the three MusPy
descriptors, so it needs no extra MIDI/muspy round-trip.
"""

from __future__ import annotations

import math
from pathlib import Path
from typing import Iterable, Mapping, Sequence

import numpy as np

from lytk import _core

# ---------------------------------------------------------------------------
# JS-similarity over the three MusPy descriptors
# ---------------------------------------------------------------------------

# The descriptors the paper fits Gaussians to; all three come straight out of
# lytk's compute_metrics, so no muspy dependency is needed.
JS_METRICS: tuple[str, ...] = (
    "polyphony_rate",
    "groove_consistency",
    "scale_consistency",
)


def compute_descriptors(
    doc, resolution: int = 480
) -> dict[str, float | None]:
    """The JS descriptors for one ``MusicDocument`` (``None`` where undefined)."""
    m = _core.compute_metrics(doc, resolution)
    out: dict[str, float | None] = {}
    for k in JS_METRICS:
        v = m.get(k)
        out[k] = None if v is None or (isinstance(v, float) and math.isnan(v)) else float(v)
    return out


def aggregate_descriptor_stats(
    per_file: Mapping[str, Mapping[str, float | None]],
) -> dict[str, dict]:
    """Compute ``{metric: {mean, std, n}}`` over per-file descriptor values."""
    if not per_file:
        return {}
    out: dict[str, dict] = {}
    keys = list(next(iter(per_file.values())).keys())
    for metric in keys:
        values = [
            v[metric]
            for v in per_file.values()
            if v.get(metric) is not None
            and isinstance(v[metric], (int, float))
            and not isinstance(v[metric], bool)
        ]
        n = len(values)
        if n == 0:
            out[metric] = {"mean": None, "std": None, "n": 0}
        elif n == 1:
            out[metric] = {"mean": float(values[0]), "std": None, "n": 1}
        else:
            mean = sum(values) / n
            var = sum((x - mean) ** 2 for x in values) / (n - 1)
            out[metric] = {"mean": float(mean), "std": math.sqrt(var), "n": n}
    return out


def _js_divergence_gaussian(
    mu1: float, sigma1: float, mu2: float, sigma2: float, n_points: int = 2000
) -> float:
    from scipy import stats as scipy_stats

    lo = min(mu1 - 5 * sigma1, mu2 - 5 * sigma2)
    hi = max(mu1 + 5 * sigma1, mu2 + 5 * sigma2)
    x = np.linspace(lo, hi, n_points)
    dx = x[1] - x[0]
    p = scipy_stats.norm.pdf(x, mu1, sigma1)
    q = scipy_stats.norm.pdf(x, mu2, sigma2)
    m = 0.5 * (p + q)

    def _kl(a, b):
        mask = (a > 0) & (b > 0)
        return float(np.sum(a[mask] * np.log(a[mask] / b[mask])) * dx)

    return 0.5 * _kl(p, m) + 0.5 * _kl(q, m)


def js_descriptor_similarity(
    model_agg: Mapping[str, Mapping[str, float | None]],
    ref_agg: Mapping[str, Mapping[str, float | None]],
    metrics: tuple[str, ...] = JS_METRICS,
) -> float | None:
    """``100 * exp(-2 * mean_JS)`` over the descriptor distributions.

    Returns ``None`` if any descriptor lacks a usable mean/std (e.g. < 2 files,
    or zero variance).
    """
    js_vals: list[float] = []
    for metric in metrics:
        m_stats = model_agg.get(metric, {})
        r_stats = ref_agg.get(metric, {})
        mu1, s1 = m_stats.get("mean"), m_stats.get("std")
        mu2, s2 = r_stats.get("mean"), r_stats.get("std")
        if any(v is None for v in (mu1, s1, mu2, s2)):
            return None
        if s1 <= 0 or s2 <= 0:
            return None
        js_vals.append(_js_divergence_gaussian(mu1, s1, mu2, s2))
    return 100.0 * math.exp(-2.0 * (sum(js_vals) / len(js_vals)))


def js_similarity(model_docs: Iterable, ref_docs: Iterable) -> float | None:
    """Convenience: JS-similarity between two iterables of ``MusicDocument``."""
    model_agg = aggregate_descriptor_stats(
        {str(i): compute_descriptors(d) for i, d in enumerate(model_docs)}
    )
    ref_agg = aggregate_descriptor_stats(
        {str(i): compute_descriptors(d) for i, d in enumerate(ref_docs)}
    )
    return js_descriptor_similarity(model_agg, ref_agg)


# ---------------------------------------------------------------------------
# Fréchet Music Distance (LilyBERT embeddings)
# ---------------------------------------------------------------------------


def lilybert_embed(
    docs: Sequence[str],
    *,
    checkpoint: str | Path,
    device: str = "cpu",
    batch_size: int = 16,
    max_length: int = 512,
    embed_layer: int | None = 6,
) -> np.ndarray:
    """Embed raw LilyPond ``docs`` with a LilyBERT checkpoint (requires torch).

    ``embed_layer=6`` reproduces the paper; pass ``None`` for the final layer.
    """
    import torch
    from transformers import AutoModel, PreTrainedTokenizerFast

    tokenizer = PreTrainedTokenizerFast.from_pretrained(str(checkpoint))
    model = AutoModel.from_pretrained(str(checkpoint)).to(device).eval()
    want_hidden = embed_layer is not None
    chunks: list[np.ndarray] = []
    with torch.no_grad():
        for i in range(0, len(docs), batch_size):
            batch = list(docs[i : i + batch_size])
            enc = tokenizer(
                batch,
                padding=True,
                truncation=True,
                max_length=max_length,
                return_tensors="pt",
            ).to(device)
            out = model(**enc, output_hidden_states=want_hidden)
            cls = (
                out.hidden_states[embed_layer][:, 0, :]
                if want_hidden
                else out.last_hidden_state[:, 0, :]
            )
            chunks.append(cls.detach().float().cpu().numpy())
    return np.concatenate(chunks, axis=0) if chunks else np.empty((0, 0))


def frechet_music_distance(x: np.ndarray, y: np.ndarray, *, eps: float = 1e-6) -> float:
    """FMD between two embedding matrices (rows = documents).

    ``FMD = ||mu_x - mu_y||^2 + Tr(Sigma_x + Sigma_y - 2*sqrt(Sigma_x @ Sigma_y))``.
    """
    from scipy import linalg

    if x.shape[0] < 2 or y.shape[0] < 2:
        raise ValueError(f"need >=2 docs per set (got {x.shape[0]}, {y.shape[0]})")
    mu_x, mu_y = x.mean(axis=0), y.mean(axis=0)
    sigma_x = np.cov(x, rowvar=False)
    sigma_y = np.cov(y, rowvar=False)
    diff = mu_x - mu_y
    covmean, _ = linalg.sqrtm(sigma_x @ sigma_y, disp=False)
    if not np.isfinite(covmean).all():
        offset = np.eye(sigma_x.shape[0]) * eps
        covmean = linalg.sqrtm((sigma_x + offset) @ (sigma_y + offset))
    if np.iscomplexobj(covmean):
        covmean = covmean.real
    return float(
        diff @ diff + np.trace(sigma_x) + np.trace(sigma_y) - 2 * np.trace(covmean)
    )


def load_documents(paths: Iterable[str | Path], *, min_chars: int = 40) -> list[str]:
    """Read ``.ly`` paths into text, dropping documents shorter than ``min_chars``."""
    docs: list[str] = []
    for p in paths:
        try:
            txt = Path(p).read_text(encoding="utf-8", errors="ignore").strip()
        except OSError:
            continue
        if len(txt) >= min_chars:
            docs.append(txt)
    return docs
