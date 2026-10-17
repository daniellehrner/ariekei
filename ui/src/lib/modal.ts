// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

/** Shows a <dialog> as a modal in the top layer: above everything else, with the focus kept in
 *  it, and given back to where it was once the dialog goes. */
export function modal(node: HTMLDialogElement) {
  const opener = document.activeElement as HTMLElement | null;
  node.showModal();
  return {
    destroy() {
      opener?.focus?.();
    },
  };
}
