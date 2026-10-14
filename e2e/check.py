"""Assertions for the e2e scenarios: a failed check stops the scenario with what was seen."""


def check(what, ok, seen=None):
    """Fails with `what` (and what was seen) unless ok."""
    if not ok:
        raise AssertionError(f"{what}" + (f" - saw: {seen!r}" if seen is not None else ""))


def equal(what, seen, expected):
    check(f"{what}: expected {expected!r}", seen == expected, seen)


def contains(what, seen, part):
    check(f"{what}: expected it to contain {part!r}", seen is not None and part in seen, seen)


def eventually(what, get, ok, timeout=20):
    """Polls get() until ok(value) holds; fails with the last value after `timeout` seconds."""
    import time

    deadline, value = time.monotonic() + timeout, None
    while True:
        value = get()
        if ok(value):
            return value
        if time.monotonic() > deadline:
            raise AssertionError(f"{what} - after {timeout}s saw: {value!r}")
        time.sleep(0.25)
