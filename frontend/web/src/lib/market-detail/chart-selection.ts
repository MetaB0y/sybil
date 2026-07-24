/**
 * Which outcomes get a line on the market-detail chart.
 *
 * Three inputs, in increasing priority:
 *   - `defaultIds`   — favourite-first fallback when nothing is chosen.
 *   - `selectedIds`  — what the legend last committed (null = untouched).
 *   - `visitedIds`   — outcomes switched to this session, plus the active one.
 *
 * Visited outcomes are appended after the base rather than merged into it, so
 * adding a line never reshuffles the chart. When the two together exceed the
 * cap the base gives way first: a line you asked for outranks a default.
 */
export function chartLineSelection({
  selectedIds,
  visitedIds,
  activeId,
  availableIds,
  defaultIds,
  max,
}: {
  selectedIds: readonly number[] | null;
  visitedIds: readonly number[];
  activeId: number;
  availableIds: ReadonlySet<number>;
  defaultIds: readonly number[];
  max: number;
}): number[] {
  const valid = (selectedIds ?? []).filter((id) => availableIds.has(id));
  const base = valid.length > 0 ? valid : [...defaultIds];
  const extras = [...visitedIds, activeId].filter(
    (id, i, all) =>
      availableIds.has(id) && !base.includes(id) && all.indexOf(id) === i,
  );
  if (extras.length === 0) return base;
  const kept = extras.slice(-max);
  return [...base.slice(0, max - kept.length), ...kept];
}
