/**
 * Return a new list with the item at `from` moved to position `to`. If either
 * index is out of range or they're equal, the original list is returned.
 */
export function reorder<T>(list: T[], from: number, to: number): T[] {
  if (
    from < 0 ||
    to < 0 ||
    from >= list.length ||
    to >= list.length ||
    from === to
  ) {
    return list;
  }
  const out = list.slice();
  const [moved] = out.splice(from, 1);
  out.splice(to, 0, moved);
  return out;
}
