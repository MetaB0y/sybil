/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.FenchelConjugate

/-!
# Theorem 1: Minting–Simplex Duality

The Fenchel conjugate of the minting cost `C(D) = max_k D_k` is the indicator
function of the standard simplex:

  `C*(p) = 0`  if `p ∈ Δ`
  `C*(p) = +∞` if `p ∉ Δ`

This is the foundational duality result connecting the minting operation
(creating one unit of every outcome) to probability distributions.

## Main results

* `FisherClearing.fenchelConjugate_mintingCost_simplex`: On the simplex, the conjugate is 0.
* `FisherClearing.fenchelConjugate_mintingCost_not_simplex`: Off the simplex, the conjugate is ⊤.
* `FisherClearing.fenchelConjugate_mintingCost_eq`: Combined characterization.

## References

* Prediction Markets Are Fisher Markets, Theorem 1
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {p : ι → ℝ}

/-- The minting cost function: `C(D) = max_k D_k`. -/
noncomputable def mintingCost (D : ι → ℝ) : ℝ :=
  Finset.univ.sup' Finset.univ_nonempty D

/-- **Theorem 1, ≤ direction**: For `p` on the simplex, `⟨p, D⟩ ≤ max_k D_k`,
    so `C*(p) ≤ 0`.

    **Proof**: If `p ∈ Δ` (nonneg, sum = 1), then
    `∑ pₖ Dₖ ≤ ∑ pₖ · max D = max D · ∑ pₖ = max D`.
    Hence `⟨p, D⟩ - C(D) ≤ 0` for all `D`, so `sup ≤ 0`. -/
theorem fenchelConjugate_mintingCost_le_zero
    (hp : p ∈ stdSimplex ℝ ι) :
    fenchelConjugate mintingCost p ≤ 0 := by
  unfold fenchelConjugate mintingCost
  apply iSup_le; intro x
  -- For p on simplex: ∑ p_k * x_k ≤ sup' x, so the difference ≤ 0
  have hle : ∑ k : ι, p k * x k ≤ Finset.univ.sup' Finset.univ_nonempty x := by
    calc ∑ k : ι, p k * x k
        ≤ ∑ k : ι, p k * Finset.univ.sup' Finset.univ_nonempty x := by
          apply Finset.sum_le_sum; intro k _
          exact mul_le_mul_of_nonneg_left (Finset.le_sup' x (Finset.mem_univ k)) (hp.1 k)
      _ = Finset.univ.sup' Finset.univ_nonempty x := by
          rw [← Finset.sum_mul, hp.2, one_mul]
  exact_mod_cast (show (∑ k : ι, p k * x k) -
    Finset.univ.sup' Finset.univ_nonempty x ≤ (0 : ℝ) by linarith)

/-- **Theorem 1, ≥ direction on simplex**: For `p ∈ Δ`, taking `D = 0` gives
    `⟨p, 0⟩ - C(0) = 0 - 0 = 0`, so `C*(p) ≥ 0`. -/
theorem zero_le_fenchelConjugate_mintingCost
    (_hp : p ∈ stdSimplex ℝ ι) :
    (0 : EReal) ≤ fenchelConjugate mintingCost p := by
  unfold fenchelConjugate mintingCost
  apply le_iSup_of_le (fun _ : ι => (0 : ℝ))
  -- At x = 0: ∑ p_k * 0 - sup'(0) = 0 - 0 = 0
  norm_cast
  simp

/-- On the simplex, the conjugate of the minting cost is exactly 0. -/
theorem fenchelConjugate_mintingCost_simplex
    (hp : p ∈ stdSimplex ℝ ι) :
    fenchelConjugate mintingCost p = 0 :=
  le_antisymm
    (fenchelConjugate_mintingCost_le_zero hp)
    (zero_le_fenchelConjugate_mintingCost hp)

/-- **Theorem 1, off-simplex**: For `p ∉ Δ`, the conjugate is `+∞`.

    **Proof sketch**: If `∑ pₖ ≠ 1`, scale `D = t · 𝟏` to send the objective to ±∞.
    If some `pₖ < 0`, set `Dₖ → -∞` to send `⟨p,D⟩ - max D → +∞`. -/
theorem fenchelConjugate_mintingCost_not_simplex
    (hp : p ∉ stdSimplex ℝ ι) :
    fenchelConjugate mintingCost p = ⊤ := by
  classical
  unfold fenchelConjugate mintingCost
  -- Suffices: the real-valued objective is unbounded above
  suffices key : ∀ r : ℝ, ∃ x : ι → ℝ,
      r < ∑ k : ι, p k * x k - Finset.univ.sup' Finset.univ_nonempty x by
    rw [iSup_eq_top]
    intro b hb
    rcases eq_or_ne b ⊥ with rfl | hbot
    · obtain ⟨x, _⟩ := key 0; exact ⟨x, EReal.bot_lt_coe _⟩
    · obtain ⟨x, hx⟩ := key b.toReal
      refine ⟨x, ?_⟩
      rw [show b = ↑b.toReal from (EReal.coe_toReal (ne_of_lt hb) hbot).symm]
      exact_mod_cast hx
  -- Prove: for all r : ℝ, ∃ x with objective > r
  intro r
  by_cases hsum : ∑ k : ι, p k = 1
  · -- ∑ p = 1, so p ∉ simplex because ∃ k with p k < 0
    have ⟨k₀, hk₀⟩ : ∃ k, p k < 0 := by
      by_contra hall; push_neg at hall; exact hp ⟨hall, hsum⟩
    -- Since ∑ p = 1 > 0 but p k₀ < 0, there must be j ≠ k₀
    have ⟨j₀, hj₀⟩ : ∃ j : ι, j ≠ k₀ := by
      by_contra hall; push_neg at hall
      have : ∑ k : ι, p k = p k₀ :=
        Finset.sum_eq_single_of_mem k₀ (Finset.mem_univ k₀)
          (fun j _ hj => absurd (hall j) hj)
      linarith
    -- Witness: set D_k₀ = -t, D_j = 0 elsewhere, with t = (|r|+1)/(-p k₀)
    set t := (|r| + 1) / (-p k₀) with ht_def
    have ht_pos : 0 < t := div_pos (by positivity) (neg_pos.mpr hk₀)
    refine ⟨Function.update (fun _ => (0 : ℝ)) k₀ (-t), ?_⟩
    -- sup' = 0: all values ≤ 0, and j₀ has value 0
    have hsup : Finset.univ.sup' Finset.univ_nonempty
        (Function.update (fun _ => (0 : ℝ)) k₀ (-t)) = 0 := le_antisymm
      (by apply Finset.sup'_le; intro k _
          simp only [Function.update_apply]
          split_ifs <;> linarith)
      (by calc (0 : ℝ) = Function.update (fun _ => (0 : ℝ)) k₀ (-t) j₀ := by
                simp [hj₀]
              _ ≤ _ := Finset.le_sup' _ (Finset.mem_univ j₀))
    -- ∑ p_k * x_k = p k₀ * (-t) (other terms vanish)
    have hobj : ∑ k : ι, p k * Function.update (fun _ => (0 : ℝ)) k₀ (-t) k =
        p k₀ * (-t) := by
      calc ∑ k : ι, p k * Function.update (fun _ => (0 : ℝ)) k₀ (-t) k
          = p k₀ * Function.update (fun _ => (0 : ℝ)) k₀ (-t) k₀ :=
            Finset.sum_eq_single_of_mem k₀ (Finset.mem_univ k₀)
              (fun j _ hj => by simp [hj])
        _ = p k₀ * (-t) := by simp
    rw [hobj, hsup, sub_zero]
    -- p k₀ * (-t) = |r| + 1 > r
    have hval : p k₀ * (-t) = |r| + 1 := by
      rw [ht_def]; field_simp [ne_of_lt hk₀]
    linarith [le_abs_self r]
  · -- ∑ p ≠ 1: constant vector c = (|r|+1)/(∑p - 1)
    have hs_ne : (∑ k : ι, p k) - 1 ≠ 0 := sub_ne_zero.mpr hsum
    set c := (|r| + 1) / ((∑ k : ι, p k) - 1) with hc_def
    refine ⟨fun _ => c, ?_⟩
    have hsup : Finset.univ.sup' Finset.univ_nonempty (fun (_ : ι) => c) = c :=
      Finset.sup'_const Finset.univ_nonempty c
    have hsum_val : ∑ k : ι, p k * c = (∑ k : ι, p k) * c := by rw [← Finset.sum_mul]
    rw [hsum_val, hsup]
    -- (∑ p) * c - c = c * (∑ p - 1) = |r| + 1 > r
    have hval : (∑ k : ι, p k) * c - c = |r| + 1 := by
      rw [hc_def]; field_simp [hs_ne]
    linarith [le_abs_self r]

/-- **Theorem 1** (Minting–Simplex Duality, combined):
    The Fenchel conjugate of `C(D) = max_k D_k` is the indicator of the simplex. -/
theorem fenchelConjugate_mintingCost_eq (p : ι → ℝ) :
    (p ∈ stdSimplex ℝ ι → fenchelConjugate mintingCost p = 0) ∧
    (p ∉ stdSimplex ℝ ι → fenchelConjugate mintingCost p = ⊤) :=
  ⟨fenchelConjugate_mintingCost_simplex, fenchelConjugate_mintingCost_not_simplex⟩

end FisherClearing
