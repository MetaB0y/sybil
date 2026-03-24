/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.LogSumExp
import FisherClearing.Convex.Softmax
import FisherClearing.ReducedForm.Utility

/-!
# Clearing Program

This file states the prediction market clearing program as a concave maximization
problem. The program combines:
- Limit order surplus: `∑ⱼ (vⱼ − ⟨q, payoffⱼ⟩) · xⱼ`
- Market maker utility: `∑ₖ ψ_{Bₖ}(Uₖ)`

over fill fractions `x ∈ [0,1]ⁿ`, clearing prices `q ∈ Δ`, and MM utilities `U > 0`.

## Main definitions

* `FisherClearing.ClearingInstance`: Data for a clearing problem.
* `FisherClearing.clearingObjective`: The objective function.
* `FisherClearing.isFeasible`: Feasibility predicate.

## References

* Prediction Markets Are Fisher Markets, §5
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]  -- market outcomes
variable {J : Type*} [Fintype J]                -- limit orders
variable {K : Type*} [Fintype K]                -- market makers

/-- Data for a clearing problem instance. -/
structure ClearingInstance (ι J K : Type*) where
  /-- Limit prices (valuations) for each order. -/
  limitPrice : J → ℝ
  /-- Payoff vector for each order: `payoff j k` is payout of order `j` in state `k`. -/
  payoff : J → ι → ℝ
  /-- Market maker budgets (liquidity parameters). -/
  budget : K → ℝ
  /-- All budgets are positive. -/
  budget_pos : ∀ k, 0 < budget k

/-- A clearing solution specifies fill fractions, prices, and MM utilities. -/
structure ClearingSolution (ι J K : Type*) where
  /-- Fill fraction for each limit order, in `[0, 1]`. -/
  fill : J → ℝ
  /-- Clearing prices (probability distribution over outcomes). -/
  price : ι → ℝ
  /-- Market maker utilities. -/
  mmUtil : K → ℝ

/-- The surplus of order `j` at prices `q`: `v_j − ⟨q, payoff_j⟩`. -/
noncomputable def orderSurplus (inst : ClearingInstance ι J K) (q : ι → ℝ) (j : J) : ℝ :=
  inst.limitPrice j - ∑ s : ι, q s * inst.payoff j s

/-- The clearing objective: total welfare from limit orders plus MM utility.
    `W = ∑ⱼ surplus_j · x_j + ∑ₖ ψ_{Bₖ}(Uₖ)` -/
noncomputable def clearingObjective
    (inst : ClearingInstance ι J K) (sol : ClearingSolution ι J K) : ℝ :=
  ∑ j : J, orderSurplus inst sol.price j * sol.fill j +
  ∑ k : K, psiB (inst.budget k) (sol.mmUtil k)

/-- A clearing solution is feasible if fills are in [0,1], prices are on the simplex,
    and MM utilities are positive. -/
def isFeasible (inst : ClearingInstance ι J K) (sol : ClearingSolution ι J K) : Prop :=
  (∀ j, 0 ≤ sol.fill j ∧ sol.fill j ≤ 1) ∧
  sol.price ∈ stdSimplex ℝ ι ∧
  (∀ k, 0 < sol.mmUtil k)

/-- The clearing program: maximize welfare over feasible solutions. -/
noncomputable def optimalWelfare (inst : ClearingInstance ι J K) : ℝ :=
  sSup { clearingObjective inst sol | (sol : ClearingSolution ι J K) (_ : isFeasible inst sol) }

/-- The MM contribution to welfare is concave in each MM's utility.
    This follows from concavity of each `ψ_B` on `(0, ∞)`. -/
theorem concaveOn_psiB_component (inst : ClearingInstance ι J K) (k : K) :
    ConcaveOn ℝ (Set.Ioi 0) (psiB (inst.budget k)) := by
  exact concaveOn_psiB (inst.budget_pos k)

end FisherClearing
