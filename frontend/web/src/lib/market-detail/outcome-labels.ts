/**
 * Short outcome labels for multi-outcome event groups.
 *
 * Polymarket-mirrored events model each outcome as a separate binary market
 * whose `name` is the full question ("Will Crude Oil (CL) hit (HIGH) $105 by
 * end of June?"). Across siblings only a fragment differs — this strips the
 * shared question text so UI shows "(HIGH) $105" instead of the whole
 * sentence. Consumed by the chart legend, the rail outcome picker, and the
 * Bet CTA so all three agree.
 */

/**
 * Strip the question text shared by every sibling outcome so callers see only
 * what differs. The longest common prefix/suffix is trimmed back to a word
 * boundary so tokens like "(HIGH)" or "$105" stay whole. Falls back to the
 * full label whenever stripping would leave nothing (lone binary market, no
 * shared text, etc.). Output is index-aligned with the input.
 */
export function deriveShortLabels(labels: string[]): string[] {
  if (labels.length < 2) return labels;

  let prefix = labels[0]!;
  let suffix = labels[0]!;
  for (const l of labels) {
    let p = 0;
    while (p < prefix.length && p < l.length && prefix[p] === l[p]) p++;
    prefix = prefix.slice(0, p);
    let s = 0;
    while (
      s < suffix.length &&
      s < l.length &&
      suffix[suffix.length - 1 - s] === l[l.length - 1 - s]
    ) {
      s++;
    }
    suffix = suffix.slice(suffix.length - s);
  }

  // Snap to whitespace so a token is never cut mid-word.
  const lastSpace = prefix.lastIndexOf(" ");
  const cutHead = lastSpace >= 0 ? lastSpace + 1 : 0;
  const firstSpace = suffix.indexOf(" ");
  const cutTail = firstSpace >= 0 ? suffix.length - firstSpace : 0;

  return labels.map((l) => {
    const short = l.slice(cutHead, l.length - cutTail).trim();
    return short.length > 0 ? short : l;
  });
}
