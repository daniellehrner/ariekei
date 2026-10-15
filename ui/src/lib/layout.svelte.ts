// Layout facts shared between views: the review's side panel is open (then the Inbox steps
// aside on narrow windows, so the diff keeps its room).

export const layout = $state({ reviewPanel: false });

// Whether the keyboard shortcuts overlay is open (? anywhere, or the review's keyboard button).
export const shortcuts = $state({ open: false });
