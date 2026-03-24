/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib

/-!
# Reduced-Form Market Maker Utility (Proposition 3)

This file defines the reduced-form utility function `ψ_B` that captures the market maker's
contribution to welfare in the clearing program. The function has two regimes:
- **Below budget** (`U ≤ B`): `ψ_B(U) = U + B log B − B` — affine, full welfare transfer
- **Above budget** (`U > B`): `ψ_B(U) = B log U` — logarithmic, diminishing returns

The function is:
- Concave on `(0, ∞)`
- C¹ at the join point `U = B` (value and derivative match)
- Has derivative `min(1, B/U)` for `U > 0`

## Main definitions

* `FisherClearing.psiB B U`: The reduced-form MM utility.

## Main results

* `FisherClearing.psiB_join`: Both branches agree at `U = B`.
* `FisherClearing.concaveOn_psiB`: `ψ_B` is concave on `(0, ∞)`.
* `FisherClearing.psiB_le_affine`: Affine envelope `ψ_B(U) ≤ U + B log B − B`.

## References

* Prediction Markets Are Fisher Markets, Proposition 3
-/

namespace FisherClearing

open Real

variable {B U : ℝ}

/-! ### Definition -/

/-- Reduced-form market maker utility:
    `ψ_B(U) = U + B log B − B` if `U ≤ B`, and `B log U` if `U > B`.
    Here `B > 0` is the MM's budget and `U > 0` is the MM's achieved utility. -/
noncomputable def psiB (B U : ℝ) : ℝ :=
  if U ≤ B then U + B * Real.log B - B else B * Real.log U

/-! ### Join continuity -/

/-- At `U = B`, both branches give `B log B`. -/
theorem psiB_join (hB : 0 < B) : psiB B B = B * Real.log B := by
  simp only [psiB, le_refl, ite_true]; ring

/-- Below budget, `ψ_B` is affine: `ψ_B(U) = U + B log B − B`. -/
theorem psiB_of_le (hU : U ≤ B) : psiB B U = U + B * Real.log B - B := by
  simp [psiB, hU]

/-- Above budget, `ψ_B` is logarithmic: `ψ_B(U) = B log U`. -/
theorem psiB_of_gt (hU : B < U) : psiB B U = B * Real.log U := by
  simp [psiB, not_le.mpr hU]

/-! ### Derivative -/

/-- The derivative of `ψ_B` below budget is 1. -/
theorem hasDerivAt_psiB_of_lt (hB : 0 < B) (hU : 0 < U) (hUB : U < B) :
    HasDerivAt (psiB B) 1 U := by
  -- Derivative of the affine branch is 1
  have hd : HasDerivAt (fun x : ℝ => x + (B * log B - B)) 1 U := by
    convert (hasDerivAt_id' U).add (hasDerivAt_const U (B * log B - B)) using 1
    norm_num
  -- psiB B agrees with this branch near U
  refine hd.congr_of_eventuallyEq ?_
  apply Filter.eventuallyEq_of_mem (Iio_mem_nhds hUB)
  intro x (hx : x < B)
  show psiB B x = x + (B * log B - B)
  rw [psiB_of_le (le_of_lt hx)]; ring

/-- The derivative of `ψ_B` above budget is `B/U`. -/
theorem hasDerivAt_psiB_of_gt (hB : 0 < B) (hU : B < U) :
    HasDerivAt (psiB B) (B / U) U := by
  -- psiB B agrees with (fun U => B * log U) near U, since B < U
  have heq : psiB B =ᶠ[nhds U] (fun U => B * log U) := by
    apply Filter.eventuallyEq_of_mem (Ioi_mem_nhds hU)
    intro x (hx : B < x)
    simp [psiB, not_le.mpr hx]
  have hU_pos : (0 : ℝ) < U := lt_trans hB hU
  -- deriv of B * log U is B * U⁻¹ = B / U
  rw [div_eq_mul_inv]
  exact ((hasDerivAt_log (ne_of_gt hU_pos)).const_mul B).congr_of_eventuallyEq heq

/-! ### Derivative at boundary -/

/-- At `U = B`, the derivative is 1 (left = affine, right = B/B = 1). -/
theorem hasDerivAt_psiB_at_eq (hB : 0 < B) :
    HasDerivAt (psiB B) 1 B := by
  -- Combine left and right derivatives
  rw [← hasDerivWithinAt_univ, ← Set.Iic_union_Ici]
  apply HasDerivWithinAt.union
  · -- Left: affine branch on Iic B
    have hd : HasDerivAt (fun x : ℝ => x + (B * log B - B)) 1 B := by
      convert (hasDerivAt_id' B).add (hasDerivAt_const B (B * log B - B)) using 1; norm_num
    exact hd.hasDerivWithinAt.congr (fun y hy => by rw [psiB_of_le hy]; ring)
      (by rw [psiB_of_le le_rfl]; ring)
  · -- Right: log branch on Ici B
    have hd : HasDerivAt (fun x : ℝ => B * log x) (B * B⁻¹) B :=
      (hasDerivAt_log (ne_of_gt hB)).const_mul B
    rw [mul_inv_cancel₀ (ne_of_gt hB)] at hd
    exact hd.hasDerivWithinAt.congr (fun y hy => by
      rcases eq_or_lt_of_le (Set.mem_Ici.mp hy) with rfl | hy'
      · rw [psiB_of_le le_rfl]; ring
      · exact psiB_of_gt hy') (by rw [psiB_of_le le_rfl]; ring)

/-- `HasDerivAt (psiB B)` at any point `U > 0`. -/
theorem hasDerivAt_psiB (hB : 0 < B) (hU : 0 < U) :
    HasDerivAt (psiB B) (if U < B then 1 else B / U) U := by
  by_cases hlt : U < B
  · simp only [if_pos hlt]; exact hasDerivAt_psiB_of_lt hB hU hlt
  · push_neg at hlt; simp only [if_neg (not_lt.mpr hlt)]
    rcases eq_or_lt_of_le hlt with rfl | hgt
    · rw [div_self (ne_of_gt hB)]; exact hasDerivAt_psiB_at_eq hB
    · exact hasDerivAt_psiB_of_gt hB hgt

/-! ### Concavity -/

/-- `ψ_B` is concave on `(0, ∞)`.

    **Proof**: The derivative `min(1, B/U)` is antitone on `(0, ∞)`,
    which implies concavity by `AntitoneOn.concaveOn_of_deriv`. -/
theorem concaveOn_psiB (hB : 0 < B) :
    ConcaveOn ℝ (Set.Ioi 0) (psiB B) := by
  apply AntitoneOn.concaveOn_of_deriv (convex_Ioi 0)
  · -- ContinuousOn: follows from differentiability
    exact fun x hx => (hasDerivAt_psiB hB hx).continuousAt.continuousWithinAt
  · -- DifferentiableOn on interior (Ioi 0)
    rw [interior_Ioi]
    exact fun x hx => (hasDerivAt_psiB hB hx).differentiableAt.differentiableWithinAt
  · -- AntitoneOn of deriv on interior (Ioi 0)
    rw [interior_Ioi]
    intro u₁ hu₁ u₂ hu₂ h12
    -- deriv (psiB B) = if U < B then 1 else B / U
    rw [(hasDerivAt_psiB hB hu₁).deriv, (hasDerivAt_psiB hB hu₂).deriv]
    -- Case split on u₁, u₂ vs B
    by_cases h1 : u₁ < B <;> by_cases h2 : u₂ < B
    · -- Both below B: deriv = 1, 1 ≤ 1
      simp [h1, h2]
    · -- u₁ < B ≤ u₂: deriv₂ = B/u₂ ≤ 1 = deriv₁
      simp only [if_pos h1, if_neg h2]
      push_neg at h2
      exact (div_le_one hu₂).mpr h2
    · -- u₂ < B ≤ u₁: impossible since u₁ ≤ u₂
      exact absurd (lt_of_le_of_lt h12 h2) (not_lt.mpr (not_lt.mp h1))
    · -- Both ≥ B: B/u₂ ≤ B/u₁
      simp only [if_neg h1, if_neg h2]
      exact (div_le_div_iff_of_pos_left hB hu₂ hu₁).mpr h12

/-! ### Affine envelope -/

/-- `ψ_B(U) ≤ U + B log B − B` for all `U > 0`.
    Equality holds iff `U ≤ B` (the affine regime). -/
theorem psiB_le_affine (hB : 0 < B) (hU : 0 < U) :
    psiB B U ≤ U + B * Real.log B - B := by
  by_cases h : U ≤ B
  · rw [psiB_of_le h]
  · push_neg at h
    rw [psiB_of_gt h]
    -- Need: B * log U ≤ U + B * log B - B
    -- Equivalently: B * (log U - log B) ≤ U - B
    -- Use: log(U/B) ≤ U/B - 1, then multiply by B
    have hUB_pos : 0 < U / B := div_pos (lt_trans hB h) hB
    have hlog : Real.log (U / B) ≤ U / B - 1 := Real.log_le_sub_one_of_pos hUB_pos
    have hlog' : Real.log U - Real.log B = Real.log (U / B) :=
      (Real.log_div (ne_of_gt (lt_trans hB h)) (ne_of_gt hB)).symm
    nlinarith [mul_le_mul_of_nonneg_left hlog (le_of_lt hB),
               mul_div_cancel₀ U (ne_of_gt hB)]

/-- The affine envelope is tight below budget. -/
theorem psiB_eq_affine_iff (hB : 0 < B) (hU : 0 < U) :
    psiB B U = U + B * Real.log B - B ↔ U ≤ B := by
  constructor
  · -- Forward: equality implies U ≤ B (strict ineq above budget)
    intro h
    by_contra hUB
    push_neg at hUB -- hUB : B < U
    rw [psiB_of_gt hUB] at h
    -- h : B * log U = U + B * log B - B, i.e., B * (log U - log B) = U - B
    have hUB_pos : (0 : ℝ) < U / B := div_pos (lt_trans hB hUB) hB
    have hlog_pos : 0 < Real.log (U / B) := Real.log_pos ((one_lt_div hB).mpr hUB)
    -- Strict bound: log(U/B) + 1 < exp(log(U/B)) = U/B
    have h_strict := add_one_lt_exp hlog_pos.ne'
    rw [Real.exp_log hUB_pos] at h_strict
    -- h_strict : log(U/B) + 1 < U/B, i.e., log(U/B) < U/B - 1
    -- So B * log(U/B) < B * (U/B - 1) = U - B
    have hlog_div : Real.log (U / B) = Real.log U - Real.log B :=
      Real.log_div (ne_of_gt (lt_trans hB hUB)) (ne_of_gt hB)
    have h_eq : B * (Real.log U - Real.log B) = U - B := by linarith
    rw [← hlog_div] at h_eq
    -- But strict: B * log(U/B) < U - B
    nlinarith [mul_lt_mul_of_pos_left h_strict hB, mul_div_cancel₀ U (ne_of_gt hB)]
  · -- Backward: U ≤ B implies equality
    exact psiB_of_le

end FisherClearing
