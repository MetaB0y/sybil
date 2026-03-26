/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.ReducedForm.Utility

/-!
# Deployed-Value Lift

The deployed-value variable `V_k = max(U_k, B_k)` is the key technical device for
proving Theorem 3. The lifted objective `B_k log V_k - V_k + U_k` is maximized over
`V_k ≥ U_k` and equals `ψ_B(U_k)` at the optimum.

## Main definitions

* `FisherClearing.deployedValue B U`: The optimal deployed value `max U B`.
* `FisherClearing.retainedCash B U`: Retained cash `max U B - U`.
* `FisherClearing.capitalScarcity B U`: Capital-scarcity factor `B / max U B`.
* `FisherClearing.liftedMM B V U`: The per-MM lifted term `B log V - V + U`.

## Main results

* `FisherClearing.liftedMM_eq_psiB`: At `V = deployedValue B U`, the lifted term equals `ψ_B(U)`.
* `FisherClearing.capitalScarcity_le_one`: The scarcity factor is at most 1.

## References

* Prediction Markets Are Fisher Markets, Proposition 4 and proof of Theorem 3
-/

namespace FisherClearing

open Real

variable {B U V : ℝ}

/-! ### Definitions -/

/-- Optimal deployed value: `V* = max(U, B)`. -/
noncomputable def deployedValue (B U : ℝ) : ℝ := max U B

/-- Retained cash: `s = V* - U = max(U, B) - U`. -/
noncomputable def retainedCash (B U : ℝ) : ℝ := max U B - U

/-- Capital-scarcity factor: `α = B / V* = B / max(U, B)`. -/
noncomputable def capitalScarcity (B U : ℝ) : ℝ := B / max U B

/-- Per-MM lifted objective term: `B log V - V + U`. -/
noncomputable def liftedMM (B V U : ℝ) : ℝ := B * log V - V + U

/-! ### Basic properties -/

theorem deployedValue_pos (hB : 0 < B) : 0 < deployedValue B U :=
  lt_of_lt_of_le hB (le_max_right U B)

theorem retainedCash_nonneg : 0 ≤ retainedCash B U := by
  simp only [retainedCash]; linarith [le_max_left U B]

theorem deployedValue_ge_U : U ≤ deployedValue B U := le_max_left U B

theorem deployedValue_ge_B : B ≤ deployedValue B U := le_max_right U B

/-! ### Capital scarcity -/

theorem capitalScarcity_pos (hB : 0 < B) : 0 < capitalScarcity B U := by
  exact div_pos hB (deployedValue_pos hB)

theorem capitalScarcity_le_one (hB : 0 < B) : capitalScarcity B U ≤ 1 := by
  simp only [capitalScarcity]
  exact div_le_one_iff.mpr (Or.inl ⟨deployedValue_pos hB, deployedValue_ge_B⟩)

theorem capitalScarcity_eq_one_iff (hB : 0 < B) (hU : 0 ≤ U) :
    capitalScarcity B U = 1 ↔ U ≤ B := by
  simp only [capitalScarcity, deployedValue]
  constructor
  · intro h; rw [div_eq_one_iff_eq (ne_of_gt (lt_of_lt_of_le hB (le_max_right U B)))] at h
    linarith [le_max_left U B]
  · intro h; rw [max_eq_right h, div_self (ne_of_gt hB)]

/-! ### Equivalence with ψ_B -/

/-- At the optimal deployed value `V = max(U, B)`, the lifted term equals `ψ_B(U)`. -/
theorem liftedMM_eq_psiB (hB : 0 < B) (hU : 0 < U) :
    liftedMM B (deployedValue B U) U = psiB B U := by
  simp only [liftedMM, deployedValue]
  by_cases h : U ≤ B
  · -- Below budget: V = B, so B log B - B + U
    rw [max_eq_right h, psiB_of_le h]; ring
  · -- Above budget: V = U, so B log U - U + U = B log U
    push_neg at h
    rw [max_eq_left h.le, psiB_of_gt h]; ring

/-- The lifted objective is at most `ψ_B(U)` for any `V ≥ U`. -/
theorem liftedMM_le_psiB (hB : 0 < B) (hU : 0 < U) (hVU : U ≤ V) :
    liftedMM B V U ≤ psiB B U := by
  have hV_pos : 0 < V := lt_of_lt_of_le hU hVU
  by_cases hUB : U ≤ B
  · -- Below budget: psiB B U = U + B*log B - B
    rw [psiB_of_le hUB]; simp only [liftedMM]
    -- Need: B*log V - V ≤ B*log B - B
    have hVB : 0 < V / B := div_pos hV_pos hB
    have h1 := log_le_sub_one_of_pos hVB
    have h2 := log_div (ne_of_gt hV_pos) (ne_of_gt hB)
    have h3 : B * log (V / B) ≤ B * (V / B - 1) := mul_le_mul_of_nonneg_left h1 hB.le
    have h4 : B * (V / B - 1) = V - B := by field_simp
    have h5 : B * (log V - log B) ≤ V - B := by
      calc B * (log V - log B) = B * log (V / B) := by rw [h2]
        _ ≤ V - B := by linarith
    nlinarith
  · -- Above budget: psiB B U = B * log U
    push_neg at hUB
    rw [psiB_of_gt hUB]; simp only [liftedMM]
    -- Need: B*log V - V + U ≤ B*log U, i.e., B*(log V - log U) ≤ V - U
    have hVU' : 0 < V / U := div_pos hV_pos hU
    have h1 := log_le_sub_one_of_pos hVU'
    have h2 := log_div (ne_of_gt hV_pos) (ne_of_gt hU)
    have h3 : B * log (V / U) ≤ B * (V / U - 1) := mul_le_mul_of_nonneg_left h1 hB.le
    have h4 : U * (V / U - 1) = V - U := by field_simp
    -- B*(V/U - 1) ≤ U*(V/U - 1) = V - U since B < U
    have h5 : B * (V / U - 1) ≤ U * (V / U - 1) := by
      apply mul_le_mul_of_nonneg_right hUB.le
      have : 1 ≤ V / U := by rwa [le_div_iff₀ hU, one_mul]
      linarith
    have h6 : U * (V / U - 1) = V - U := by field_simp
    have h7 : B * (log V - log U) ≤ V - U := by
      calc B * (log V - log U) = B * log (V / U) := by rw [h2]
        _ ≤ V - U := by linarith
    nlinarith

end FisherClearing
