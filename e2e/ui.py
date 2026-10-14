"""Steps through Ariekei's UI shared by the scenarios."""


def open_inbox(s):
    """Shows the Inbox if it is collapsed: on a narrow screen (under 1200 px, e.g. a CI
    runner's) it steps aside while the review side panel is open. Ctrl+B, as a user would."""
    if s.find(".inbox.collapsed"):
        s.js("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'b', ctrlKey: true, bubbles: true }))")
        s.wait(".inbox:not(.collapsed)", timeout=10)


def select_row(s, number):
    open_inbox(s)
    s.wait(f".inbox .row[data-number='{number}']", timeout=60)
    s.js("document.querySelector(`.inbox .row[data-number='${arguments[0]}']`).click()", number)
