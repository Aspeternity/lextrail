/**
 * The popup's natural height in CSS pixels (= logical pixels): what it would take with
 * nothing scrolled. The window may be shorter or taller right now, so the layout is
 * unconstrained for an instant (`popup--measuring`) and measured synchronously; nothing
 * is painted in between.
 */
export function naturalHeight(root: HTMLElement): number {
  root.classList.add('popup--measuring');
  try {
    return Math.ceil(root.getBoundingClientRect().height);
  } finally {
    root.classList.remove('popup--measuring');
  }
}
