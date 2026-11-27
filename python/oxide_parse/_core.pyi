"""Type stubs for the native extension module (`crates/py/src/lib.rs`).

Keep names aligned with `crates/py/src/lib.rs`: `tests/test_cli_and_release.py` verifies stub parity against the
compiled module when present.
"""
from typing import Any, Dict, Iterable, List, Optional, Tuple, Union

Source = Union[str, bytes, bytearray]

__version__: str
"""Version string of the compiled Rust extension crate (`oxide_parse._core`)."""


class OxideParseError(Exception):
    """Compilation, extraction, and session errors raised by the native extension."""


def engines() -> List[Tuple[str, str, bool, str]]:
    """Return `(name, kind, xpath_supported, description)` tuples for compiled-in engines.

    Examples:
        >>> from oxide_parse import _core
        >>> any(name == "json" for name, _, _, _ in _core.engines())
        True
    """


def distill(html: Source, keep: int = 2, max_text: int = 80, max_chars: int = 20000) -> str:
    """Shrink HTML into a compact skeleton for LLM prompts or debugging (`lite` feature required).

    Args:
        html: HTML `str`, UTF-8 `bytes`, or `bytearray`.
        keep: Similar sibling count retained before folding (`<!-- +N similar ... -->`).
        max_text: Character limit per text node.
        max_chars: Hard character limit on returned HTML.

    Returns:
        Distilled HTML skeleton string.
    """


class Session:
    """In-progress extraction holding pending `fetch` requests (`follow`/`paginate`) and selector `repair` requests."""

    def pending(self) -> List[Tuple[int, Dict[str, Any]]]:
        """Return open fetch requests as `(id, params)` tuples for `fetch(**params)`."""

    def requests_json(self) -> str:
        """Serialize all open requests (`fetch` and `repair`) as a JSON array string."""

    def overrides(self) -> List[Tuple[str, str]]:
        """Return selectors supplied by repairs so far as `(key, css)` tuples."""

    def done(self) -> bool:
        """Return `True` when no fetch requests remain pending (repairs are optional)."""

    def errors(self) -> List[Tuple[str, str]]:
        """Return records quarantined by `map(..., on_error=...)` as `(path, message)` tuples."""

    def misses_json(self) -> str:
        """Return unresolved `adaptive` selector misses serialized as JSON."""

    def resume(self, items: List[Tuple[int, Any]]) -> None:
        """Feed `(id, document)`, `(id, (document, meta_json))` fetches, `(id, {key: css})` repairs, or `(id, None)` declines."""

    def result(self) -> Any:
        """Consume and return the final extracted Python value once `done()` is `True`."""


class Plan:
    """Thread-safe compiled extraction plan executed in Rust with the GIL released."""

    def __init__(
        self,
        ir_json: str,
        backend: str = "html",
        adaptive_store: Optional[str] = None,
        adaptive_state: Optional[str] = None,
    ) -> None:
        """Compile a JSON IR string for `backend` (`auto`, `html`, `stream`, `lite`, `scraper`, `xml`, `json`)."""

    @property
    def backend(self) -> str:
        """Resolved execution engine name."""

    def explain(self) -> str:
        """Return the multi-line execution plan (selectors, peephole rewrites, streaming capture table)."""

    def ir_json(self) -> str:
        """Serialize the plan's IR back to JSON."""

    def start(self, source: Source, meta_json: Optional[str] = None) -> Session:
        """Start a stateful `Session` for `follow`, `paginate`, or manual `adaptive` repair."""

    def extract(self, source: Source, meta_json: Optional[str] = None) -> Any:
        """Run one-shot extraction over `source` (raises if pending requests are emitted)."""

    def run_native(self, source: Source) -> int:
        """Run extraction without creating Python objects; returns extracted value count for benchmarking."""

    def extract_many(self, sources: Iterable[Source]) -> List[Any]:
        """Extract all documents in parallel using Rayon with the GIL released."""

    def extract_batch(self, sources: Iterable[Source], threads: Optional[int] = None, collect_errors: bool = False) -> Tuple[List[Any], List[Tuple[int, str]]]:
        """Extract documents on a Rayon pool; with `collect_errors=True`, failed pages yield `(index, message)`."""

    def export_adaptive_store(self) -> str:
        """Serialize all learned `adaptive` selector fingerprints as JSON."""

    def import_adaptive_store(self, state: str) -> int:
        """Merge serialized fingerprints from `export_adaptive_store()`; returns count loaded."""

    def forget_adaptive_keys(self, keys: List[str]) -> None:
        """Delete stored baseline and `#healed` fingerprints for `keys`."""
