/**
 * Where the devnet notice records that it has been dismissed.
 *
 * Its own module, not an export of the (client) component: `layout.tsx` is a
 * server component, and importing a value across a "use client" boundary hands
 * it a client reference — which interpolated into the pre-paint script as
 * garbage, and quietly stopped the dismissal from surviving a reload.
 */
export const DEVNET_DISMISSED_KEY = "sybil-devnet-notice-dismissed";
