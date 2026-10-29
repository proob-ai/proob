"""Example-based tests for the store: the tests the team had before proob."""

import pytest

from idem import Conflict, IdempotencyStore, InProgress, Proceed, Replay, Response, fingerprint


def test_a_new_key_proceeds_and_then_replays():
    store = IdempotencyStore(ttl_seconds=60)
    assert store.begin("order-1", "fp", now=0) == Proceed()
    store.complete("order-1", Response(201, "created"))
    assert store.begin("order-1", "fp", now=1) == Replay(Response(201, "created"))


def test_a_key_in_flight_reports_in_progress():
    store = IdempotencyStore(ttl_seconds=60)
    store.begin("order-1", "fp", now=0)
    assert store.begin("order-1", "fp", now=1) == InProgress()


def test_a_different_request_under_the_same_key_is_a_conflict():
    store = IdempotencyStore(ttl_seconds=60)
    store.begin("order-1", "fp-a", now=0)
    assert store.begin("order-1", "fp-b", now=1) == Conflict()


def test_an_expired_key_is_new_again():
    store = IdempotencyStore(ttl_seconds=10)
    store.begin("order-1", "fp", now=0)
    store.complete("order-1", Response(200, "ok"))
    assert store.begin("order-1", "fp", now=10) == Proceed()


def test_completing_an_unknown_key_is_an_error():
    store = IdempotencyStore(ttl_seconds=10)
    with pytest.raises(KeyError):
        store.complete("never-begun", Response(200, "ok"))


def test_fingerprint_ignores_method_case():
    assert fingerprint("post", "/orders", {"sku": 1}) == fingerprint("POST", "/orders", {"sku": 1})
