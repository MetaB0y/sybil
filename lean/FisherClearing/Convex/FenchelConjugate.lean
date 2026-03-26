/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib

/-!
# Fenchel Conjugate

This file defines the Fenchel (convex) conjugate for functions on finite-dimensional
real vector spaces `ι → ℝ`. The conjugate is valued in `EReal` to handle the case
where the supremum is `+∞`.

The Fenchel conjugate is the key duality tool connecting:
- The minting cost (max function) to the simplex indicator (Theorem 1)
- The LMSR cost (logSumExp) to negative entropy (Theorem 2)

## Main definitions

* `FisherClearing.fenchelConjugate f p`: The Fenchel conjugate `f*(p) = sup_x {⟨p,x⟩ - f(x)}`.

## References

* Rockafellar, *Convex Analysis*, §12
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι]

/-- The Fenchel (convex) conjugate of `f : (ι → ℝ) → ℝ` at dual variable `p : ι → ℝ`:
    `f*(p) = sup_x {⟨p, x⟩ - f(x)}`.
    Valued in `EReal` since the supremum may be `+∞`. -/
noncomputable def fenchelConjugate (f : (ι → ℝ) → ℝ) (p : ι → ℝ) : EReal :=
  ⨆ (x : ι → ℝ), (((∑ k : ι, p k * x k) - f x : ℝ) : EReal)

/-- The inner product `⟨p, x⟩ - f(x)` is bounded above by `f*(p)`. -/
lemma le_fenchelConjugate (f : (ι → ℝ) → ℝ) (p : ι → ℝ) (x : ι → ℝ) :
    (((∑ k : ι, p k * x k) - f x : ℝ) : EReal) ≤ fenchelConjugate f p :=
  le_iSup (fun x => (((∑ k : ι, p k * x k) - f x : ℝ) : EReal)) x

/-- Fenchel–Young inequality: `⟨p, x⟩ ≤ f(x) + f*(p)` when `f*(p) < ⊤`. -/
theorem fenchel_young (f : (ι → ℝ) → ℝ) (p : ι → ℝ) (x : ι → ℝ)
    (hfin : fenchelConjugate f p ≠ ⊤) :
    (∑ k : ι, p k * x k : ℝ) ≤ f x + (fenchelConjugate f p).toReal := by
  have h1 := le_fenchelConjugate f p x
  -- fenchelConjugate is not ⊥ (it's ≥ a real number)
  have hbot : fenchelConjugate f p ≠ ⊥ := by
    intro heq; rw [heq] at h1; exact not_le.mpr (EReal.bot_lt_coe _) h1
  -- Rewrite conjugate as a real cast
  rw [(EReal.coe_toReal hfin hbot).symm] at h1
  -- Extract real inequality from EReal inequality
  have h2 : ∑ k : ι, p k * x k - f x ≤ (fenchelConjugate f p).toReal := by exact_mod_cast h1
  linarith

/-! ### Convexity of the Fenchel conjugate -/

/-- The Fenchel conjugate of any `ℝ`-valued function is convex wherever it is finite.

    **Proof**: `f*(θp₁ + (1-θ)p₂) = sup_x {⟨θp₁+(1-θ)p₂, x⟩ - f(x)}`
    `= sup_x {θ(⟨p₁,x⟩ - f(x)) + (1-θ)(⟨p₂,x⟩ - f(x)) + (θ+1-θ-1)f(x)}`
    `= sup_x {θ(⟨p₁,x⟩ - f(x)) + (1-θ)(⟨p₂,x⟩ - f(x))}`
    `≤ θ · sup_x{⟨p₁,x⟩ - f(x)} + (1-θ) · sup_x{⟨p₂,x⟩ - f(x)}`. -/
theorem convexOn_fenchelConjugate_real (f : (ι → ℝ) → ℝ)
    (S : Set (ι → ℝ)) (hS : Convex ℝ S)
    (hfin : ∀ p ∈ S, fenchelConjugate f p ≠ ⊤) :
    ConvexOn ℝ S (fun p => (fenchelConjugate f p).toReal) := by
  constructor
  · exact hS
  · intro p₁ hp₁ p₂ hp₂ a b ha hb hab
    have hfin₁ := hfin p₁ hp₁
    have hfin₂ := hfin p₂ hp₂
    have hbot₁ : fenchelConjugate f p₁ ≠ ⊥ := ne_of_gt
      (lt_of_lt_of_le (EReal.bot_lt_coe _) (le_fenchelConjugate f p₁ 0))
    have hbot₂ : fenchelConjugate f p₂ ≠ ⊥ := ne_of_gt
      (lt_of_lt_of_le (EReal.bot_lt_coe _) (le_fenchelConjugate f p₂ 0))
    have hpS : a • p₁ + b • p₂ ∈ S := hS hp₁ hp₂ ha hb hab
    have hfinm := hfin _ hpS
    have hbotm : fenchelConjugate f (a • p₁ + b • p₂) ≠ ⊥ := ne_of_gt
      (lt_of_lt_of_le (EReal.bot_lt_coe _) (le_fenchelConjugate f _ 0))
    -- Key: for each x, the inner product decomposes
    set R := a * (fenchelConjugate f p₁).toReal + b * (fenchelConjugate f p₂).toReal
    -- Step 1: For each x, ⟨a•p₁+b•p₂, x⟩ - f(x) ≤ R
    have hpw : ∀ x : ι → ℝ,
        ∑ k, (a • p₁ + b • p₂) k * x k - f x ≤ R := by
      intro x
      -- Decompose inner product
      -- Expand inner product: ⟨a•p₁+b•p₂, x⟩ = a•⟨p₁,x⟩ + b•⟨p₂,x⟩
      have hip : ∑ k, (a • p₁ + b • p₂) k * x k =
          a * ∑ k, p₁ k * x k + b * ∑ k, p₂ k * x k := by
        calc ∑ k, (a • p₁ + b • p₂) k * x k
            = ∑ k, (a * (p₁ k * x k) + b * (p₂ k * x k)) := by
                congr 1; ext k; simp [Pi.add_apply, Pi.smul_apply, smul_eq_mul]; ring
          _ = a * ∑ k, p₁ k * x k + b * ∑ k, p₂ k * x k := by
                rw [Finset.sum_add_distrib, ← Finset.mul_sum, ← Finset.mul_sum]
      -- Use a + b = 1 to split f(x) = a•f(x) + b•f(x)
      have hd : ∑ k, (a • p₁ + b • p₂) k * x k - f x =
          a * (∑ k, p₁ k * x k - f x) + b * (∑ k, p₂ k * x k - f x) := by
        have h1 : (a + b) * f x = f x := by rw [hab]; ring
        linarith [hip]
      rw [hd]
      have h1 : ∑ k, p₁ k * x k - f x ≤ (fenchelConjugate f p₁).toReal := by
        have := le_fenchelConjugate f p₁ x
        rw [(EReal.coe_toReal hfin₁ hbot₁).symm] at this; exact_mod_cast this
      have h2 : ∑ k, p₂ k * x k - f x ≤ (fenchelConjugate f p₂).toReal := by
        have := le_fenchelConjugate f p₂ x
        rw [(EReal.coe_toReal hfin₂ hbot₂).symm] at this; exact_mod_cast this
      nlinarith
    -- Step 2: f*(a•p₁+b•p₂) ≤ ↑R in EReal
    have hle : fenchelConjugate f (a • p₁ + b • p₂) ≤ ↑R := by
      apply iSup_le; intro x; exact_mod_cast hpw x
    -- Step 3: Convert EReal inequality to ℝ inequality
    simp only [smul_eq_mul]
    have hR : (fenchelConjugate f (a • p₁ + b • p₂)).toReal ≤ R :=
      EReal.toReal_le_toReal hle hbotm (EReal.coe_ne_top R)
    exact hR

end FisherClearing
