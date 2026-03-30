---
tags: [concept, economics]
layer: core
status: current
last_verified: 2026-03-15
---

Sybil's solver maximizes [[Welfare Maximization|welfare]] (total consumer surplus), not volume (total shares traded). This is a deliberate design choice with real consequences: an order at exactly the clearing price — zero surplus — may not fill if higher-surplus orders consume all available liquidity. The solver would rather fill 100 shares generating $15 of welfare than fill 200 shares generating $14 of welfare.

This can feel counterintuitive. If someone is willing to sell at 50 cents and someone else is willing to buy at 50 cents, hasn't the exchange fulfilled its purpose by matching them? Yes, but if a third trader is willing to buy at 70 cents and there's only 100 shares of supply, the 70-cent buyer creates more total value. The 50-cent buyer generates zero surplus — the market would be just as well off without that trade. Welfare maximization says: give the scarce supply to whoever values it most.

The arguments cut both ways. Welfare maximization gives you allocative efficiency, incentive compatibility (bid your true value), and protection against spam orders at marginal prices. Volume maximization gives you more data for price discovery, more fee revenue, and happier users who see their orders filled. Academic literature (Vickrey 1961, Budish-Cramton-Shim 2015) generally favors welfare for auctions and prediction markets, while traditional exchanges often use FIFO for fairness and regulatory compliance. Sybil currently implements pure welfare maximization. A hybrid mode (fill zero-surplus orders when doing so sacrifices less than X% of welfare) remains a possible future option.

## Key Properties
- Welfare-first: high-surplus orders always fill before low-surplus orders
- Zero-surplus orders (limit = clearing price) may be left unfilled
- Volume potential can be sacrificed for marginal welfare gains
- Economically standard for auctions and prediction markets
- Volume maximization mode is not currently implemented but could be added

## Where This Lives
> `design/welfare-vs-volume.md` — full analysis with scenarios and academic references

## See Also
- [[Welfare Maximization]] — the objective function and why total welfare is price-independent
- [[The LP Core]] — the LP naturally prioritizes high-surplus orders
- [[Frequent Batch Auctions]] — FBAs make welfare maximization natural
