/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.LogSumExp

/-!
# Sandwich Bound (Proposition 1)

The log-sum-exp function is sandwiched between the maximum and the maximum plus
a logarithmic correction term:

  `max_i f i ≤ logSumExp b f ≤ max_i f i + b · log(|ι|)`

This characterizes logSumExp as a smooth approximation to the max function,
with approximation quality controlled by the temperature `b`.

## Main results

* `FisherClearing.le_logSumExp`: Lower bound `max f ≤ logSumExp b f`.
* `FisherClearing.logSumExp_le_sup_add`: Upper bound `logSumExp b f ≤ max f + b · log |ι|`.
* `FisherClearing.logSumExp_sandwich`: Combined sandwich inequality.
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {b : ℝ}

/-- The maximum of `f` over a finite nonempty type, using `Finset.sup'`. -/
noncomputable def maxFin (f : ι → ℝ) : ℝ :=
  Finset.univ.sup' Finset.univ_nonempty f

lemma le_maxFin (f : ι → ℝ) (k : ι) : f k ≤ maxFin f :=
  Finset.le_sup' f (Finset.mem_univ k)

/-! ### Lower bound: max ≤ logSumExp -/

/-- Each component of `f` is at most `logSumExp b f` (for `b > 0`). -/
theorem le_logSumExp_of_mem (hb : 0 < b) (f : ι → ℝ) (k : ι) :
    f k ≤ logSumExp b f := by
  unfold logSumExp
  rw [show f k = b * (f k / b) from (mul_div_cancel₀ (f k) (ne_of_gt hb)).symm]
  apply mul_le_mul_of_nonneg_left _ (le_of_lt hb)
  rw [← Real.log_exp (f k / b)]
  exact Real.log_le_log (Real.exp_pos _) (exp_div_le_sumExp b f k)

/-- **Lower sandwich bound**: `max_i f i ≤ logSumExp b f` for `b > 0`. -/
theorem le_logSumExp (hb : 0 < b) (f : ι → ℝ) :
    maxFin f ≤ logSumExp b f := by
  apply Finset.sup'_le
  intro k _
  exact le_logSumExp_of_mem hb f k

/-! ### Upper bound: logSumExp ≤ max + b · log n -/

/-- **Upper sandwich bound**: `logSumExp b f ≤ max_i f i + b · log |ι|` for `b > 0`.

    **Proof**: Each `exp(fᵢ/b) ≤ exp(max f / b)`, so
    `∑ exp(fᵢ/b) ≤ |ι| · exp(max f / b)`. Taking `b · log`:
    `logSumExp b f ≤ b · log(|ι| · exp(max f / b)) = max f + b · log |ι|`. -/
theorem logSumExp_le_sup_add (hb : 0 < b) (f : ι → ℝ) :
    logSumExp b f ≤ maxFin f + b * Real.log (Fintype.card ι) := by
  unfold logSumExp
  have hcard : (0 : ℝ) < Fintype.card ι := Nat.cast_pos.mpr Fintype.card_pos
  -- Step 1: bound sumExp from above by n * exp(max f / b)
  have hsub : sumExp b f ≤ (Fintype.card ι : ℝ) * Real.exp (maxFin f / b) := by
    unfold sumExp
    calc ∑ i : ι, Real.exp (f i / b)
        ≤ ∑ _i : ι, Real.exp (maxFin f / b) := by
          apply Finset.sum_le_sum; intro i _
          exact Real.exp_le_exp.mpr (div_le_div_of_nonneg_right (le_maxFin f i) (le_of_lt hb))
      _ = (Fintype.card ι : ℝ) * Real.exp (maxFin f / b) := by
          rw [Finset.sum_const, Finset.card_univ, nsmul_eq_mul]
  -- Step 2: take log of both sides (log is monotone)
  have hlog : Real.log (sumExp b f) ≤ Real.log (Fintype.card ι) + maxFin f / b := by
    calc Real.log (sumExp b f)
        ≤ Real.log ((Fintype.card ι : ℝ) * Real.exp (maxFin f / b)) :=
          Real.log_le_log (sumExp_pos b f) hsub
      _ = Real.log (Fintype.card ι) + Real.log (Real.exp (maxFin f / b)) :=
          Real.log_mul (ne_of_gt hcard) (ne_of_gt (Real.exp_pos _))
      _ = Real.log (Fintype.card ι) + maxFin f / b := by rw [Real.log_exp]
  -- Step 3: multiply by b > 0 and simplify
  calc b * Real.log (sumExp b f)
      ≤ b * (Real.log (Fintype.card ι) + maxFin f / b) :=
        mul_le_mul_of_nonneg_left hlog (le_of_lt hb)
    _ = maxFin f + b * Real.log (Fintype.card ι) := by
        field_simp; ring

/-- **Sandwich inequality** (Proposition 1):
    `max_i f i ≤ logSumExp b f ≤ max_i f i + b · log |ι|`. -/
theorem logSumExp_sandwich (hb : 0 < b) (f : ι → ℝ) :
    maxFin f ≤ logSumExp b f ∧
    logSumExp b f ≤ maxFin f + b * Real.log (Fintype.card ι) :=
  ⟨le_logSumExp hb f, logSumExp_le_sup_add hb f⟩

end FisherClearing
