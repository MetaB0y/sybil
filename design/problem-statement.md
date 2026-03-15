# Welfare-Maximizing Matching in Frequent Batch Auctions

## Self-Contained Problem Statement

---

## 1. Setting

A **prediction market exchange** runs **Frequent Batch Auctions (FBA)**: orders accumulate over a time window (e.g., 1 second), then all orders in the batch are matched simultaneously at uniform clearing prices.

The exchange operates **binary markets**. Each market $m$ has two outcomes: YES (outcome 0) and NO (outcome 1). Exactly one outcome resolves to \$1; the other resolves to \$0.

Some markets are grouped into **market groups** representing mutually exclusive events. For example, an election with candidates A, B, C creates three binary markets (one per candidate), grouped because exactly one candidate wins.

### Participants

- **Traders**: submit limit orders (buy YES at up to 60c, sell NO at at least 30c, etc.)
- **Market makers (MMs)**: submit orders on both sides with a **capital budget constraint** limiting total risk exposure across their portfolio.

---

## 2. Orders

Each order $i$ has:

| Field | Type | Description |
|-------|------|-------------|
| market $m_i$ | MarketId | Which market |
| side $\sigma_i$ | {BuyYes, SellYes, BuyNo, SellNo} | Direction |
| limit $L_i$ | Nanos (u64) | Maximum price willing to pay (buyer) or minimum willing to accept (seller) |
| max fill $\bar{Q}_i$ | u64 | Maximum quantity to fill |
| MM id $k_i$ | Option\<MmId\> | If this order belongs to a market maker |

Nanos are fixed-point: $1.00 = 10^9$ nanos. No floating point anywhere.

### Conversion to Unified YES Space

Because buying NO at limit $L$ is economically equivalent to selling YES at limit $(\$1 - L)$, we can convert all orders to a unified YES representation:

| Original | Unified | Unified limit |
|----------|---------|---------------|
| BuyYes at $L$ | YES demand | $L$ |
| SellYes at $L$ | YES supply | $L$ |
| BuyNo at $L$ | YES supply | $\$1 - L$ |
| SellNo at $L$ | YES demand | $\$1 - L$ |

After conversion, each market has:
- **Demand set** $D_m$: orders wanting to buy YES
- **Supply set** $S_m$: orders willing to sell YES

---

## 3. Minting

The exchange can **mint** new shares to provide liquidity:

### Per-Market Minting

Create 1 YES share + 1 NO share for cost \$1. This is always available — it's the fundamental property of binary markets (the two shares always sum to \$1 at resolution).

Variable: $\text{mint}_m \geq 0$ for each market $m$.

### Group Minting

For a group $g$ of $K$ mutually exclusive markets, create 1 YES share on **every** market in the group for cost \$1. This works because exactly one market resolves YES, paying out exactly \$1.

Variable: $\text{gmint}_g \geq 0$ for each group $g$.

Group minting is $K$ times cheaper per YES share than per-market minting. This is the key structural advantage that MILP exploits and heuristic solvers miss.

---

## 4. Decision Variables

| Variable | Domain | Meaning |
|----------|--------|---------|
| $q_i$ | $[0, \bar{Q}_i]$ | Fill quantity for order $i$ |
| $\text{mint}_m$ | $[0, \infty)$ | Per-market minting on market $m$ |
| $\text{gmint}_g$ | $[0, \infty)$ | Group minting for group $g$ |

---

## 5. Objective: Welfare Maximization

**Total welfare** = sum of surplus across all filled orders.

For a buyer filled at clearing price $p$: surplus $= (L_i - p) \times q_i$

For a seller filled at clearing price $p$: surplus $= (p - L_i) \times q_i$

A standard result in auction theory: total welfare equals the area between supply and demand curves, which depends only on **which orders fill and how much**, not on the clearing price itself. The price determines the **split** of surplus between buyers and sellers, but not the total.

Therefore, total welfare can be written as a function of fill quantities alone:

$$W = \sum_{i \in \text{buyers}} L_i \cdot q_i \;-\; \sum_{j \in \text{sellers}} L_j \cdot q_j \;-\; \$1 \cdot \sum_m \text{mint}_m \;-\; \$1 \cdot \sum_g \text{gmint}_g$$

The minting cost terms capture the cost of creating new shares. At the welfare-maximizing clearing, this cost is exactly offset by the value of the shares to buyers, so it nets out — but it must appear in the objective to prevent unbounded minting.

**This objective is linear in the decision variables.**

---

## 6. Constraints

### 6.1. Position Balance (per market, per outcome)

For each market $m$ and each outcome $o \in \{0, 1\}$:

$$\sum_{i \in \text{buy}(m,o)} q_i \;\leq\; \sum_{j \in \text{sell}(m,o)} q_j \;+\; \text{mint}_m \;+\; \mathbb{1}[o = 0] \cdot \text{gmint}_{g(m)}$$

In words: total demand for outcome $o$ on market $m$ cannot exceed total supply plus minting. Group minting only contributes to YES (outcome 0) supply.

### 6.2. Quantity Bounds

$$0 \leq q_i \leq \bar{Q}_i \quad \forall i$$

### 6.3. Market Maker Budget (the hard constraint)

For each market maker $k$ with budget $B_k$:

$$\sum_{i \in \text{orders}(k)} \text{capital}_i(p_{m_i}, q_i) \;\leq\; B_k$$

Where capital depends on the **clearing price** $p$ and fill quantity:

| Side | Capital per unit |
|------|-----------------|
| BuyYes | $p_{\text{YES}} \cdot q$ |
| SellYes | $(1 - p_{\text{YES}}) \cdot q$ |
| BuyNo | $(1 - p_{\text{YES}}) \cdot q$ |
| SellNo | $p_{\text{YES}} \cdot q$ |

The clearing price $p_m$ is determined by the market clearing itself (it is the **dual variable** of the position balance constraint — see Section 7). This makes the budget constraint **bilinear**: it couples primal quantities $q_i$ with dual prices $p_m$.

### 6.4. Uniform Clearing Price (UCP)

All trades in a market execute at the same clearing price:

$$q_i > 0 \implies \text{surplus}_i(p_m) \geq 0$$

For buyers: $L_i \geq p_m$. For sellers: $p_m \geq L_i$.

---

## 7. The LP Structure

### Observation: Without MM budgets, the problem is a Linear Program.

Constraints 6.1, 6.2, and 6.4 together with the linear objective form an LP:

$$\boxed{\begin{aligned}
\max_{q, \text{mint}, \text{gmint}} \quad & \sum_{i \in \text{buyers}} L_i \cdot q_i - \sum_{j \in \text{sellers}} L_j \cdot q_j - \$1 \cdot \sum_m \text{mint}_m - \$1 \cdot \sum_g \text{gmint}_g \\[6pt]
\text{s.t.} \quad & \sum_{i \in \text{buy}(m,o)} q_i \leq \sum_{j \in \text{sell}(m,o)} q_j + \text{mint}_m + \mathbb{1}[o{=}0] \cdot \text{gmint}_{g(m)} & \forall m, o \\[4pt]
& 0 \leq q_i \leq \bar{Q}_i & \forall i \\[4pt]
& \text{mint}_m, \text{gmint}_g \geq 0 & \forall m, g
\end{aligned}}$$

**Size**: $O(N + M + G)$ variables, $O(N + M)$ constraints. For 10K orders, 100 markets, 10 groups: trivially solvable by modern LP solvers in <1ms.

### Dual Variables = Clearing Prices

The dual variable of the position balance constraint for market $m$, outcome $o$ is the **clearing price** $p_{m,o}$.

LP duality gives us, for free:

| Property | Dual condition | Meaning |
|----------|---------------|---------|
| **UCP** | Complementary slackness | $q_i > 0 \implies L_i \geq p_m$ (buyer) |
| **Price normalization** | $\text{mint}_m$ stationarity | $p_{m,\text{YES}} + p_{m,\text{NO}} \leq \$1$; equality when minting active |
| **Group consistency** | $\text{gmint}_g$ stationarity | $\sum_{m \in g} p_{m,\text{YES}} \leq \$1$; equality when group minting active |

All economic constraints emerge naturally from LP duality. No post-hoc enforcement needed.

---

## 8. The Full Problem (LP + Bilinear Budget)

Adding MM budget constraints to the LP:

$$\boxed{\begin{aligned}
\max \quad & W(q, \text{mint}, \text{gmint}) \quad \text{(linear)} \\[6pt]
\text{s.t.} \quad & \text{LP constraints (Section 7)} \\[4pt]
& \sum_{i \in \text{orders}(k)} p_{m_i} \cdot q_i \cdot c_i \leq B_k & \forall k \in \text{MMs}
\end{aligned}}$$

where $c_i$ is a coefficient depending on order side ($+1$ for BuyYes/SellNo, $-1$ for SellYes/BuyNo after appropriate transformation) and $p_{m_i}$ is the dual variable of the balance constraint.

**This is the only source of non-convexity.** The MM budget constraint couples primal variables ($q_i$) with dual variables ($p_m$) through a bilinear product.

### Problem Classification

| Without MM budgets | With MM budgets |
|-------------------|----------------|
| Linear Program | LP + bilinear side constraints |
| Polynomial time | NP-hard in general |
| Unique optimal (up to degeneracy) | Multiple local optima possible |
| Solved exactly by simplex/IPM | Requires specialized methods |

---

## 9. Bundle / Multi-Market Orders

Some orders bet on compound events across multiple markets. A bundle order "buy A-YES and B-YES" pays \$1 only if both A and B are YES.

### Payoff Vector Representation

A bundle spanning $K$ markets has a payoff vector over $2^K$ atomic states:

$$\text{payoff}_s \in \{-1, 0, +1\} \quad \text{for each state } s \in \{0, 1\}^K$$

Example — "buy A-YES and B-YES": payoff = [+1, 0, 0, 0] for states [AB, Ab, aB, ab].

### Marginal Decomposition

For each market $m$ the order spans, its **marginal payoff** is:

$$\delta_{i,m} = \mathbb{E}[\text{payoff} \mid m = \text{YES}] - \mathbb{E}[\text{payoff} \mid m = \text{NO}]$$

The bundle contributes $|\delta_{i,m}|$ units of YES demand (if $\delta > 0$) or supply (if $\delta < 0$) to market $m$'s position balance.

### LP Extension

**Approximate**: Use marginal decomposition in the per-market LP. Loses correlation structure but handles 85%+ of orders correctly.

**Exact**: Use per-state balance constraints (Arrow-Debreu formulation). For each compound state $s$ that appears in some order's payoff vector:

$$\sum_{i: \text{payoff}_{i,s} > 0} \text{payoff}_{i,s} \cdot q_i \leq \sum_{j: \text{payoff}_{j,s} < 0} |\text{payoff}_{j,s}| \cdot q_j + \text{state\_supply}_s$$

The number of constraints is $O(\sum_i 2^{K_i})$ where $K_i$ is the number of markets order $i$ spans. For typical bundles (2-3 markets), this is $O(N)$.

---

## 10. Problem Size (Typical)

| Parameter | Small | Medium | Large |
|-----------|-------|--------|-------|
| Markets | 10 | 30 | 200 |
| Orders | 300 | 3,000 | 100,000 |
| Market groups | 3 | 10 | 50 |
| MMs | 2 | 5 | 10 |
| MM orders | 50 | 500 | 5,000 |
| Bundles | 45 | 450 | 15,000 |

The LP (without MM budgets) is trivial at all sizes. The bilinear MM budget constraint is the computational bottleneck, but the number of MMs is always small (2-10).

---

## 11. Summary

The welfare-maximizing matching problem in FBA has a clean mathematical structure:

1. **Without MM budgets**: the problem is an **LP**. Prices, UCP, and group consistency all emerge from LP duality.

2. **With MM budgets**: the problem gains **bilinear side constraints** (price $\times$ quantity $\leq$ budget). This is the sole source of non-convexity and NP-hardness.

3. **The number of bilinear constraints is small** (one per MM, typically 2-10). This makes the problem amenable to specialized decomposition methods that solve the LP efficiently and handle the few bilinear constraints iteratively.

Any solution method should exploit this structure: solve the LP core exactly, and handle the MM budget coupling as a perturbation.
