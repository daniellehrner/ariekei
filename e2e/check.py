"""Assertions for the e2e scenarios: a failed check stops the scenario with what was seen."""


def check(what, ok, seen=None):
    """Fails with `what` (and what was seen) unless ok."""
    if not ok:
        raise AssertionError(f"{what}" + (f" — saw: {seen!r}" if seen is not None else ""))


def equal(what, seen, expected):
    check(f"{what}: expected {expected!r}", seen == expected, seen)


def contains(what, seen, part):
    check(f"{what}: expected it to contain {part!r}", seen is not None and part in seen, seen)
