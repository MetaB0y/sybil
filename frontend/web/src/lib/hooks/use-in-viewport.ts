"use client";

import { useEffect, useRef, useState } from "react";

/**
 * Sticky IntersectionObserver hook. Returns `[ref, inView]` where `inView`
 * flips to true the first time the element enters the viewport and stays
 * true thereafter (so consumers can fetch once and not flicker on scroll-out).
 *
 * `rootMargin` lets us prefetch just before the element is visible.
 */
export function useInViewport<T extends Element>(
  rootMargin = "120px"
): [React.RefObject<T | null>, boolean] {
  const ref = useRef<T | null>(null);
  const [inView, setInView] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el || inView) return;
    if (typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) {
          setInView(true);
          io.disconnect();
        }
      },
      { rootMargin }
    );
    io.observe(el);
    return () => io.disconnect();
  }, [inView, rootMargin]);

  return [ref, inView];
}
