/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Clearing.Program
import FisherClearing.Duality.LmsrEntropy

/-!
# Theorem 4: Price Uniqueness

The clearing prices `q*` in the optimal solution are **unique**, even though
fill fractions and MM utilities may not be. This follows from the strict
convexity of the entropy regularizer on the simplex.

## Main results

* `FisherClearing.strictConvexOn_negEntropy_simplex`: `−∑ pₖ log pₖ` is strictly convex on `Δ`.
* `FisherClearing.price_unique`: Optimal clearing prices are unique.

## Proof strategy

The clearing program's dual involves minimizing a function of prices `q` that includes
the entropy term `−b · ∑ qₖ log qₖ` (from the LMSR conjugate). Since:
1. `−∑ qₖ log qₖ` is strictly convex on the simplex (from strict concavity of `negMulLog`)
2. The simplex is compact (`isCompact_stdSimplex`)
3. The remaining terms are convex in `q`

The sum is strictly convex, so its minimizer (= optimal prices) is unique.

## References

* Prediction Markets Are Fisher Markets, Theorem 4
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {b : ℝ}

/-! ### Strict convexity of negative entropy -/

/-- Negative entropy `q ↦ -∑ negMulLog(qₖ)` is strictly convex on the standard simplex.

    **Proof**: Each `negMulLog` is strictly concave on `[0, ∞)` (by Mathlib's
    `strictConcaveOn_negMulLog`), so `∑ negMulLog(qₖ)` is strictly concave on the simplex,
    and its negation is strictly convex. -/
theorem strictConvexOn_negEntropy_simplex :
    StrictConvexOn ℝ (stdSimplex ℝ ι) (fun q : ι → ℝ => -shannonEntropy q) := by
  -- -shannonEntropy q = ∑ k, q k * log (q k) — sum of strictly convex coordinate functions
  simp only [show (fun q : ι → ℝ => -shannonEntropy q) =
    (fun q => ∑ k : ι, q k * log (q k)) from by ext q; simp [shannonEntropy, negEntropy]]
  refine ⟨convex_stdSimplex ℝ ι, fun q₁ hq₁ q₂ hq₂ hne a c ha hc hac => ?_⟩
  -- Rewrite RHS as a single sum
  simp only [Pi.add_apply, Pi.smul_apply, smul_eq_mul]
  rw [Finset.mul_sum, Finset.mul_sum, ← Finset.sum_add_distrib]
  -- Apply sum_lt_sum: strict at some coordinate, ≤ elsewhere
  obtain ⟨k₀, hk₀⟩ := Function.ne_iff.mp hne
  apply Finset.sum_lt_sum
  · -- Convexity at every coordinate
    intro k _
    have h := strictConvexOn_mul_log.convexOn.2 (Set.mem_Ici.mpr (hq₁.1 k))
      (Set.mem_Ici.mpr (hq₂.1 k)) ha.le hc.le hac
    simp only [smul_eq_mul] at h; linarith
  · -- Strict convexity at k₀ where q₁ k₀ ≠ q₂ k₀
    exact ⟨k₀, Finset.mem_univ k₀, by
      have h := strictConvexOn_mul_log.2 (Set.mem_Ici.mpr (hq₁.1 k₀))
        (Set.mem_Ici.mpr (hq₂.1 k₀)) hk₀ ha hc hac
      simp only [smul_eq_mul] at h; linarith⟩

/-- The entropy-regularized dual objective for clearing is strictly convex in prices. -/
theorem strictConvexOn_dual_prices (hb : 0 < b)
    (g : (ι → ℝ) → ℝ) (hg : ConvexOn ℝ (stdSimplex ℝ ι) g) :
    StrictConvexOn ℝ (stdSimplex ℝ ι)
      (fun q : ι → ℝ => g q + b * (-shannonEntropy q)) := by
  -- b * strictly_convex is strictly convex
  have hsce : StrictConvexOn ℝ (stdSimplex ℝ ι) (fun q => b * (-shannonEntropy q)) := by
    refine ⟨strictConvexOn_negEntropy_simplex.1, fun x hx y hy hne a c ha hc hac => ?_⟩
    have h := strictConvexOn_negEntropy_simplex.2 hx hy hne ha hc hac
    simp only [Pi.add_apply, Pi.smul_apply, smul_eq_mul] at h ⊢
    nlinarith
  -- convex + strictly_convex = strictly_convex
  exact hg.add_strictConvexOn hsce

/-! ### Existence and uniqueness of optimal prices -/

/-- An optimal price vector exists (the simplex is compact, the objective is continuous). -/
theorem exists_optimal_price
    (g : (ι → ℝ) → ℝ) (hg : ContinuousOn g (stdSimplex ℝ ι)) :
    ∃ q ∈ stdSimplex ℝ ι, ∀ q' ∈ stdSimplex ℝ ι, g q ≤ g q' := by
  -- Compact simplex + continuous function → minimum exists
  classical
  have hne : (stdSimplex ℝ ι).Nonempty := by
    obtain ⟨i₀⟩ := ‹Nonempty ι›
    exact ⟨fun i => if i = i₀ then 1 else 0,
      fun i => by dsimp; split_ifs <;> norm_num,
      by simp [Finset.sum_ite_eq', Finset.mem_univ]⟩
  obtain ⟨q, hq, hmin⟩ := (isCompact_stdSimplex ι).exists_isMinOn hne hg
  exact ⟨q, hq, fun q' hq' => hmin hq'⟩

/-- **Theorem 4** (Price Uniqueness): The optimal clearing prices are unique.

    **Proof sketch**: The dual objective `D(q) = g(q) − b · H(q)` is strictly convex
    on the compact simplex. A strictly convex continuous function on a compact convex
    set has at most one minimizer. Combined with existence, the minimizer is unique. -/
theorem price_unique (hb : 0 < b)
    (g : (ι → ℝ) → ℝ)
    (hg_conv : ConvexOn ℝ (stdSimplex ℝ ι) g)
    (hg_cont : ContinuousOn g (stdSimplex ℝ ι))
    (q₁ q₂ : ι → ℝ)
    (hq₁ : q₁ ∈ stdSimplex ℝ ι) (hq₂ : q₂ ∈ stdSimplex ℝ ι)
    (hopt₁ : ∀ q ∈ stdSimplex ℝ ι,
      g q₁ + b * (-shannonEntropy q₁) ≤ g q + b * (-shannonEntropy q))
    (hopt₂ : ∀ q ∈ stdSimplex ℝ ι,
      g q₂ + b * (-shannonEntropy q₂) ≤ g q + b * (-shannonEntropy q)) :
    q₁ = q₂ := by
  -- Strictly convex function has at most one minimizer
  exact (strictConvexOn_dual_prices hb g hg_conv).eq_of_isMinOn
    (fun q hq => hopt₁ q hq) (fun q hq => hopt₂ q hq) hq₁ hq₂

end FisherClearing
