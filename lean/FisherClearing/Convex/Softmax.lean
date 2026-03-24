/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.LogSumExp

/-!
# Softmax Function

This file defines the softmax function and proves it maps to the standard simplex.
Softmax is the gradient of log-sum-exp, and its output gives the clearing prices
in the LMSR market maker.

## Main definitions

* `FisherClearing.softmax b f k`: The function `exp(f k / b) / ∑ j, exp(f j / b)`.

## Main results

* `FisherClearing.softmax_nonneg`: Each softmax component is nonnegative.
* `FisherClearing.softmax_sum_eq_one`: Softmax components sum to 1.
* `FisherClearing.softmax_mem_stdSimplex`: Softmax output lies in the standard simplex.
* `FisherClearing.hasDerivAt_logSumExp_comp`: Softmax is the gradient of logSumExp.
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {b : ℝ}

/-! ### Softmax definition and basic properties -/

/-- Softmax with temperature `b`:
    `softmax b f k = exp(f k / b) / ∑ j, exp(f j / b)`.
    For `b > 0`, this gives a probability distribution over `ι`. -/
noncomputable def softmax (b : ℝ) (f : ι → ℝ) (k : ι) : ℝ :=
  Real.exp (f k / b) / sumExp b f

lemma softmax_nonneg (b : ℝ) (f : ι → ℝ) (k : ι) : 0 ≤ softmax b f k :=
  div_nonneg (le_of_lt (Real.exp_pos _)) (sumExp_nonneg b f)

lemma softmax_pos (b : ℝ) (f : ι → ℝ) (k : ι) : 0 < softmax b f k :=
  div_pos (Real.exp_pos _) (sumExp_pos b f)

lemma softmax_le_one (b : ℝ) (f : ι → ℝ) (k : ι) : softmax b f k ≤ 1 := by
  unfold softmax
  rw [div_le_one (sumExp_pos b f)]
  exact exp_div_le_sumExp b f k

/-! ### Softmax sums to one -/

/-- Softmax components sum to 1. -/
theorem softmax_sum_eq_one (b : ℝ) (f : ι → ℝ) :
    ∑ k : ι, softmax b f k = 1 := by
  simp only [softmax, ← Finset.sum_div]
  exact div_self (sumExp_ne_zero b f)

/-! ### Simplex membership -/

/-- Softmax output lies in the standard simplex `Δ = {p ≥ 0 | ∑ pᵢ = 1}`. -/
theorem softmax_mem_stdSimplex (b : ℝ) (f : ι → ℝ) :
    softmax b f ∈ stdSimplex ℝ ι :=
  ⟨fun k => softmax_nonneg b f k, softmax_sum_eq_one b f⟩

/-! ### Gradient relationship -/

/-- Softmax is the gradient of logSumExp: the partial derivative of `logSumExp b` with
    respect to the `k`-th component of `f` equals `softmax b f k`.

    **Proof sketch**: By the chain rule,
    `∂/∂fₖ [b · log(∑ exp(fᵢ/b))] = b · (1/∑ exp(fᵢ/b)) · exp(fₖ/b) · (1/b) = softmax_k`. -/
theorem hasDerivAt_logSumExp_comp [DecidableEq ι] (hb : 0 < b) (f : ι → ℝ) (k : ι) :
    HasDerivAt (fun t => logSumExp b (Function.update f k t))
      (softmax b f k) (f k) := by
  -- Decompose sumExp into the k-th exponential plus a constant
  set C := ∑ i ∈ Finset.univ.erase k, Real.exp (f i / b) with hC_def
  -- Key: sumExp (update f k t) = exp(t/b) + C for all t
  have hsplit : ∀ t, sumExp b (Function.update f k t) = Real.exp (t / b) + C := by
    intro t; unfold sumExp
    rw [← Finset.add_sum_erase _ _ (Finset.mem_univ k)]
    simp only [Function.update_self]; congr 1
    exact Finset.sum_congr rfl fun i hi => by
      rw [Function.update_of_ne (Finset.ne_of_mem_erase hi)]
  -- exp(f k / b) + C = sumExp b f
  have hsum_eq : Real.exp (f k / b) + C = sumExp b f := by
    have h := hsplit (f k); rw [Function.update_eq_self] at h; linarith
  have hpos : 0 < Real.exp (f k / b) + C := by linarith [sumExp_pos b f]
  -- Rewrite function to b * log(exp(t/b) + C)
  rw [show (fun t => logSumExp b (Function.update f k t)) =
      (fun t => b * Real.log (Real.exp (t / b) + C)) from
    funext fun t => by simp only [logSumExp, hsplit]]
  -- Chain rule step by step
  have h1 : HasDerivAt (fun t => Real.exp (t / b))
      (Real.exp (f k / b) * (1 / b)) (f k) := by
    have h := (Real.hasDerivAt_exp (f k / b)).comp (f k) ((hasDerivAt_id (f k)).div_const b)
    simp only [Function.comp_def, id] at h; exact h
  have h2 : HasDerivAt (fun t => Real.exp (t / b) + C)
      (Real.exp (f k / b) * (1 / b)) (f k) := by
    have h := h1.add (hasDerivAt_const (f k) C); rwa [add_zero] at h
  have h3 := h2.log (ne_of_gt hpos)
  have h4 := h3.const_mul b
  -- Simplify derivative to softmax
  have hderiv_eq : softmax b f k =
      b * (Real.exp (f k / b) * (1 / b) / (Real.exp (f k / b) + C)) := by
    simp only [softmax, ← hsum_eq]
    field_simp [ne_of_gt hb, ne_of_gt hpos]
  rw [hderiv_eq]; exact h4

end FisherClearing
