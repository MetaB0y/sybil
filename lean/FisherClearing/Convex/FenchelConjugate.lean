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

end FisherClearing
