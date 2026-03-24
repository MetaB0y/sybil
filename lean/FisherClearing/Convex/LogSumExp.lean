/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib

/-!
# Log-Sum-Exp Function

This file defines the log-sum-exp function with temperature parameter and proves its convexity.
Log-sum-exp (LSE) is a smooth approximation to the maximum function, fundamental to
prediction market clearing via the LMSR cost function.

## Main definitions

* `FisherClearing.sumExp b f`: The sum `∑ i, exp(f i / b)`, always positive.
* `FisherClearing.logSumExp b f`: The function `b * log(∑ i, exp(f i / b))`.

## Main results

* `FisherClearing.sumExp_pos`: The sum of exponentials is always positive.
* `FisherClearing.convexOn_logSumExp`: Log-sum-exp is convex as a function of `f`.

## References

* Boyd, Vandenberghe. *Convex Optimization*, Example 3.1.5
* Prediction Markets Are Fisher Markets, §4
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {b : ℝ}

/-! ### Sum of exponentials -/

/-- The sum of exponentials: `∑ i, exp(f i / b)`. This is the inner term of `logSumExp`.
    Always positive regardless of `b` and `f`. -/
noncomputable def sumExp (b : ℝ) (f : ι → ℝ) : ℝ :=
  ∑ i : ι, Real.exp (f i / b)

lemma sumExp_pos (b : ℝ) (f : ι → ℝ) : 0 < sumExp b f :=
  Finset.sum_pos (fun _ _ => Real.exp_pos _) Finset.univ_nonempty

lemma sumExp_ne_zero (b : ℝ) (f : ι → ℝ) : sumExp b f ≠ 0 :=
  ne_of_gt (sumExp_pos b f)

lemma sumExp_nonneg (b : ℝ) (f : ι → ℝ) : 0 ≤ sumExp b f :=
  le_of_lt (sumExp_pos b f)

/-- Each exponential term is at most the full sum. -/
lemma exp_div_le_sumExp (b : ℝ) (f : ι → ℝ) (k : ι) :
    Real.exp (f k / b) ≤ sumExp b f := by
  apply Finset.single_le_sum _ (Finset.mem_univ k)
  intro _ _
  exact le_of_lt (Real.exp_pos _)

/-! ### Log-sum-exp -/

/-- Log-sum-exp with temperature parameter `b`:
    `logSumExp b f = b * log(∑ i, exp(f i / b))`.
    For `b > 0`, this is a smooth convex approximation to `max_i f i`. -/
noncomputable def logSumExp (b : ℝ) (f : ι → ℝ) : ℝ :=
  b * Real.log (sumExp b f)

/-- logSumExp is convex as a function of `f : ι → ℝ`.

**Proof sketch** (via Hölder's inequality):
For `θ ∈ [0,1]` and vectors `x, y : ι → ℝ`, set `aᵢ = exp(xᵢ/b)`, `bᵢ = exp(yᵢ/b)`.
By Hölder with exponents `1/θ` and `1/(1-θ)`:
  `∑ aᵢᶿ bᵢ¹⁻ᶿ ≤ (∑ aᵢ)ᶿ · (∑ bᵢ)¹⁻ᶿ`
The left side equals `∑ exp((θxᵢ + (1-θ)yᵢ)/b)`.
Taking `b · log` of both sides (monotone for `b > 0`):
  `logSumExp b (θx + (1-θ)y) ≤ θ · logSumExp b x + (1-θ) · logSumExp b y`. -/
theorem convexOn_logSumExp (hb : 0 < b) :
    ConvexOn ℝ Set.univ (fun f : ι → ℝ => logSumExp b f) := by
  constructor
  · exact convex_univ
  · intro x _ y _ a c ha hc hac
    simp only [logSumExp]
    set Sx := sumExp b x
    set Sy := sumExp b y
    have hSx : 0 < Sx := sumExp_pos b x
    have hSy : 0 < Sy := sumExp_pos b y
    have hSxa_ne : Sx ^ a ≠ 0 := (rpow_pos_of_pos hSx a).ne'
    have hSyc_ne : Sy ^ c ≠ 0 := (rpow_pos_of_pos hSy c).ne'
    -- Step 1: exp((a*xi + c*yi)/b) = exp(xi/b)^a * exp(yi/b)^c
    have hterm : ∀ i, exp ((a * x i + c * y i) / b) =
        exp (x i / b) ^ a * exp (y i / b) ^ c := by
      intro i
      rw [show (a * x i + c * y i) / b = x i / b * a + y i / b * c from by ring,
          exp_add, exp_mul, exp_mul]
    have hS_rw : sumExp b (a • x + c • y) =
        ∑ i : ι, exp (x i / b) ^ a * exp (y i / b) ^ c := by
      unfold sumExp
      simp only [Pi.add_apply, Pi.smul_apply, smul_eq_mul]
      exact Finset.sum_congr rfl fun i _ => hterm i
    -- Step 2: Normalized sums = 1
    have hp_sum : ∑ i : ι, exp (x i / b) / Sx = 1 := by
      rw [← Finset.sum_div]; exact div_self (ne_of_gt hSx)
    have hq_sum : ∑ i : ι, exp (y i / b) / Sy = 1 := by
      rw [← Finset.sum_div]; exact div_self (ne_of_gt hSy)
    -- Step 3: Factoring — each term = Sx^a * Sy^c * (normalized)^a * (normalized)^c
    have hfact : ∀ i, exp (x i / b) ^ a * exp (y i / b) ^ c =
        Sx ^ a * Sy ^ c * ((exp (x i / b) / Sx) ^ a * (exp (y i / b) / Sy) ^ c) := by
      intro i; symm
      rw [div_rpow (exp_pos _).le hSx.le, div_rpow (exp_pos _).le hSy.le,
          div_mul_div_comm, mul_comm (Sx ^ a * Sy ^ c),
          div_mul_cancel₀ _ (mul_ne_zero hSxa_ne hSyc_ne)]
    -- Step 4: Bound normalized sum ≤ 1 via AM-GM
    have hbound : ∑ i : ι, (exp (x i / b) / Sx) ^ a * (exp (y i / b) / Sy) ^ c ≤ 1 :=
      calc ∑ i : ι, (exp (x i / b) / Sx) ^ a * (exp (y i / b) / Sy) ^ c
          ≤ ∑ i : ι, (a * (exp (x i / b) / Sx) + c * (exp (y i / b) / Sy)) :=
            Finset.sum_le_sum fun i _ =>
              Real.geom_mean_le_arith_mean2_weighted ha hc
                (div_nonneg (exp_pos _).le hSx.le) (div_nonneg (exp_pos _).le hSy.le) hac
        _ = a + c := by
            rw [Finset.sum_add_distrib, ← Finset.mul_sum, ← Finset.mul_sum,
                hp_sum, hq_sum, mul_one, mul_one]
        _ = 1 := hac
    -- Step 5: Key inequality — sumExp(a•x + c•y) ≤ Sx^a * Sy^c
    have hHolder : sumExp b (a • x + c • y) ≤ Sx ^ a * Sy ^ c := by
      rw [hS_rw]; simp_rw [hfact, ← Finset.mul_sum]
      exact mul_le_of_le_one_right
        (mul_nonneg (rpow_nonneg hSx.le a) (rpow_nonneg hSy.le c)) hbound
    -- Step 6: Take b * log of both sides
    have hS_pos : 0 < sumExp b (a • x + c • y) := sumExp_pos b _
    calc b * log (sumExp b (a • x + c • y))
        ≤ b * log (Sx ^ a * Sy ^ c) := by
          exact mul_le_mul_of_nonneg_left (log_le_log hS_pos hHolder) hb.le
      _ = b * (a * log Sx + c * log Sy) := by
          rw [log_mul (rpow_pos_of_pos hSx a).ne' (rpow_pos_of_pos hSy c).ne',
              log_rpow hSx, log_rpow hSy]
      _ = a * (b * log Sx) + c * (b * log Sy) := by ring

end FisherClearing
