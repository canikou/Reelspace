/** The section the reader is in: the first one, in page order, that intersects the observed band. */
export function pickActiveSection(order: string[], visible: ReadonlySet<string>, fallback: string): string {
  return order.find((id) => visible.has(id)) ?? fallback;
}
