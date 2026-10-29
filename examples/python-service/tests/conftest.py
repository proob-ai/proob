"""Shared Hypothesis strategies for the idem tests."""

from hypothesis import strategies as st

from idem.store import Response

# A few keys and fingerprints, so generated sequences reuse them often.
keys = st.sampled_from(["k1", "k2", "k3"])
fingerprints = st.sampled_from(["fp-a", "fp-b"])
responses = st.builds(Response, st.sampled_from([200, 201, 409]), st.text(max_size=8))
# JSON-object bodies: string keys, scalar or nested-object values.
bodies = st.recursive(
    st.dictionaries(st.text(min_size=1, max_size=4), st.integers() | st.text(max_size=4), max_size=4),
    lambda inner: st.dictionaries(st.text(min_size=1, max_size=4), inner, max_size=3),
    max_leaves=6,
)
