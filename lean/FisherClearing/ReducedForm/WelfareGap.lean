/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.ReducedForm.Utility

/-!
# Welfare Gap Bound (Proposition 6)

When a market maker's utility `U` exceeds its budget `B` by `Δ = U − B`,
the welfare gap between the affine envelope and the actual reduced-form utility is:

  `gap = Δ − B · log(1 + Δ/B) ≤ Δ² / (2B)`

This bounds how much welfare is "lost" to the logarithmic regime.

## Main results

* `FisherClearing.welfare_gap_def`: The gap is `Δ − B · log(1 + Δ/B)`.
* `FisherClearing.welfare_gap_le_sq`: Quadratic upper bound `Δ²/(2B)`.

## References

* Prediction Markets Are Fisher Markets, Proposition 6
-/

namespace FisherClearing

open Real

variable {B Δ : ℝ}

/-! ### Welfare gap -/

/-- The welfare gap when utility exceeds budget: the difference between the affine
    envelope and the actual reduced-form utility. -/
noncomputable def welfareGap (B Δ : ℝ) : ℝ :=
  Δ - B * Real.log (1 + Δ / B)

/-- The welfare gap equals the difference `(U + B log B - B) - ψ_B(U)` when `U = B + Δ`.
    This is the welfare "lost" to the logarithmic regime. -/
theorem welfareGap_eq_affine_minus_psiB (hB : 0 < B) (hΔ : 0 ≤ Δ) :
    welfareGap B Δ = (B + Δ + B * Real.log B - B) - psiB B (B + Δ) := by
  rcases eq_or_lt_of_le hΔ with rfl | hΔ_pos
  · -- Δ = 0: both sides simplify to 0
    simp [welfareGap, psiB, Real.log_one]
  · -- Δ > 0: U = B + Δ > B, so psiB is in the logarithmic regime
    rw [psiB_of_gt (show B < B + Δ by linarith)]
    unfold welfareGap
    -- Rewrite log(1 + Δ/B) = log((B+Δ)/B) = log(B+Δ) - log B
    have hBΔ_pos : (0 : ℝ) < B + Δ := by linarith
    have h_eq : 1 + Δ / B = (B + Δ) / B := by field_simp
    rw [h_eq, Real.log_div (ne_of_gt hBΔ_pos) (ne_of_gt hB)]
    ring

/-- The welfare gap is nonneg: `Δ ≥ B · log(1 + Δ/B)`.
    This follows from `log(1 + x) ≤ x` for all `x > -1`. -/
theorem welfareGap_nonneg (hB : 0 < B) (hΔ : 0 ≤ Δ) :
    0 ≤ welfareGap B Δ := by
  unfold welfareGap
  -- Need: 0 ≤ Δ - B * log(1 + Δ/B), i.e., B * log(1 + Δ/B) ≤ Δ
  -- Use: log(x) ≤ x - 1 for x > 0, with x = 1 + Δ/B
  suffices h : B * Real.log (1 + Δ / B) ≤ Δ by linarith
  have h1 : (0 : ℝ) < 1 + Δ / B := by positivity
  have h2 : Real.log (1 + Δ / B) ≤ Δ / B := by linarith [Real.log_le_sub_one_of_pos h1]
  nlinarith [mul_le_mul_of_nonneg_left h2 (le_of_lt hB),
             mul_div_cancel₀ Δ (ne_of_gt hB)]

/-- **Proposition 6** (Welfare gap bound): `Δ − B · log(1 + Δ/B) ≤ Δ²/(2B)`.

    **Proof sketch**: The inequality `x − log(1 + x) ≤ x²/2` for `x ≥ 0`
    follows from `log(1 + x) ≥ x − x²/2` (Taylor remainder bound).
    Apply with `x = Δ/B` and multiply by `B`. -/
theorem welfare_gap_le_sq (hB : 0 < B) (hΔ : 0 ≤ Δ) :
    welfareGap B Δ ≤ Δ ^ 2 / (2 * B) := by
  unfold welfareGap
  have hx : 0 ≤ Δ / B := div_nonneg hΔ (le_of_lt hB)
  have hlog := le_log_one_add_of_nonneg hx
  -- Reduce to: x - log(1+x) ≤ x²/2 where x = Δ/B
  suffices hsuff : Δ / B - log (1 + Δ / B) ≤ (Δ / B) ^ 2 / 2 by
    have h := mul_le_mul_of_nonneg_left hsuff (le_of_lt hB)
    have lhs_eq : B * (Δ / B - log (1 + Δ / B)) = Δ - B * log (1 + Δ / B) := by
      field_simp
    have rhs_eq : B * ((Δ / B) ^ 2 / 2) = Δ ^ 2 / (2 * B) := by
      field_simp
    linarith
  -- From hlog: log(1+x) ≥ 2x/(x+2), so x - log(1+x) ≤ x - 2x/(x+2) = x²/(x+2) ≤ x²/2
  have hden : (0 : ℝ) < Δ / B + 2 := by linarith
  have hstep1 : Δ / B - 2 * (Δ / B) / (Δ / B + 2) = (Δ / B) ^ 2 / (Δ / B + 2) := by
    field_simp; ring
  have hstep2 : Δ / B - log (1 + Δ / B) ≤ (Δ / B) ^ 2 / (Δ / B + 2) := by linarith
  have hstep3 : (Δ / B) ^ 2 / (Δ / B + 2) ≤ (Δ / B) ^ 2 / 2 := by
    rw [div_le_div_iff₀ hden (by norm_num : (0:ℝ) < 2)]
    nlinarith [sq_nonneg (Δ / B)]
  linarith

end FisherClearing
