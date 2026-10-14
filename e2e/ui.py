"""Steps through Ariekei's UI shared by the scenarios."""


def open_inbox(s):
    """Shows the Inbox if it is collapsed: on a narrow screen (under 1200 px, e.g. a CI
    runner's) it steps aside while the review side panel is open. Ctrl+B, as a user would."""
    s.wait(".inbox", timeout=60)
    if s.find(".inbox.collapsed"):
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
        s.wait(".inbox:not(.collapsed)", timeout=10)


def select_row(s, number):
    wait_rows(s)
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)


def wait_rows(s, timeout=60):
    """Waits for Inbox rows, opening the Inbox whenever it collapses meanwhile (right after
    a launch it collapses only once the restored review tab has mounted)."""
    import time

    deadline = time.monotonic() + timeout
    while not s.find(".inbox .row"):
        if time.monotonic() > deadline:
            raise TimeoutError("no Inbox rows" + s._page())
        if s.find(".inbox.collapsed"):
            open_inbox(s)
        time.sleep(0.25)


def side_by_side(s):
    """Shows the diff side by side: below a 720 px diff column (a small screen) it starts
    unified. The choice is the user's toggle (u), remembered for the item."""
    s.wait("[data-opt='unified']", timeout=30)
    if s.js("return document.querySelector(\"[data-opt='unified']\").getAttribute('aria-pressed') === 'true'"):
        s.js("document.querySelector(\"[data-opt='unified']\").click()")
        s.wait_js("return document.querySelector(\"[data-opt='unified']\").getAttribute('aria-pressed') === 'false'", timeout=10)
