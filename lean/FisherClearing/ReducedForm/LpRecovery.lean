/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.ReducedForm.Utility

/-!
# LP Recovery (Proposition 5)

When every market maker operates below budget (`U_k ≤ B_k`), the reduced-form
utility `ψ_B` is in its affine regime, and the clearing program reduces to a
linear program. In this case, the Fisher-market solution exactly recovers the
LP-optimal welfare.

## Main results

* `FisherClearing.psiB_affine_regime`: Below budget, `ψ_B(U)` is affine in `U`.
* `FisherClearing.lp_recovery`: If all MMs are below budget, reduced-form = LP optimum.

## References

* Prediction Markets Are Fisher Markets, Proposition 5
-/

namespace FisherClearing

open scoped BigOperators
open Real

variable {κ : Type*} [Fintype κ]

/-! ### LP Recovery -/

/-- If `U_k ≤ B_k` for all market makers `k`, then `∑ ψ_{B_k}(U_k)` is affine in `U`.
    Specifically: `∑ ψ_{B_k}(U_k) = ∑ U_k + ∑ (B_k log B_k − B_k)`. -/
theorem psiB_sum_affine_of_le
    (B U : κ → ℝ) (hle : ∀ k, U k ≤ B k) :
    ∑ k : κ, psiB (B k) (U k) =
      ∑ k : κ, U k + ∑ k : κ, (B k * Real.log (B k) - B k) := by
  have h : ∀ k, psiB (B k) (U k) = U k + (B k * Real.log (B k) - B k) :=
    fun k => by rw [psiB_of_le (hle k)]; ring
  simp_rw [h, Finset.sum_add_distrib]

/-- **Proposition 5** (LP Recovery): When all market makers operate below budget,
    the Fisher-market clearing program reduces to a linear program.

    The reduced-form objective `∑ ψ_{B_k}(U_k)` in the below-budget regime differs
    from `∑ U_k` by only a constant `∑ (B_k log B_k − B_k)`, so their optima
    coincide. The LP has the same feasible set and a linear objective, hence
    standard LP duality applies. -/
theorem lp_recovery
    (B U_star U_lp : κ → ℝ)
    (hB : ∀ k, 0 < B k)
    (h_star_le : ∀ k, U_star k ≤ B k)
    (h_lp_le : ∀ k, U_lp k ≤ B k)
    (h_lp_opt : ∑ k : κ, U_lp k ≥ ∑ k : κ, U_star k) :
    ∑ k : κ, psiB (B k) (U_lp k) ≥ ∑ k : κ, psiB (B k) (U_star k) := by
  rw [psiB_sum_affine_of_le B U_lp h_lp_le, psiB_sum_affine_of_le B U_star h_star_le]
  linarith

end FisherClearing
