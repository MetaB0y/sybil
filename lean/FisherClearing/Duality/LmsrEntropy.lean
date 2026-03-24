/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.LogSumExp
import FisherClearing.Convex.FenchelConjugate
import FisherClearing.Convex.Softmax

/-!
# Theorem 2: LMSR Cost ↔ Entropy Duality

The Fenchel conjugate of the LMSR cost function `C_b(D) = logSumExp b D` is
negative entropy scaled by the liquidity parameter `b`:

  `C_b*(p) = b · ∑ pₖ log pₖ`  if `p ∈ Δ`
  `C_b*(p) = +∞`               if `p ∉ Δ`

As `b → 0⁺`, the LMSR cost converges to the minting cost (max), and
the entropy term converges to the simplex indicator (Theorem 1).

## Main definitions

* `FisherClearing.negEntropy p`: Negative Shannon entropy `∑ pₖ log pₖ`.
* `FisherClearing.shannonEntropy p`: Shannon entropy `H(p) = -∑ pₖ log pₖ`.

## Main results

* `FisherClearing.fenchelConjugate_logSumExp_eq`: Theorem 2 — full characterization.
* `FisherClearing.negEntropy_nonpos_on_simplex`: Entropy is nonneg, so negEntropy ≤ 0.

## References

* Prediction Markets Are Fisher Markets, Theorem 2
* Hanson, "Logarithmic Market Scoring Rules" (2003)
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {b : ℝ} {p : ι → ℝ}

/-! ### Entropy definitions -/

/-- Negative Shannon entropy: `∑ k, pₖ · log pₖ`.
    This equals `−H(p)` where `H` is the Shannon entropy.
    Convention: `0 · log 0 = 0` (from `Real.log 0 = 0` in Mathlib). -/
noncomputable def negEntropy (p : ι → ℝ) : ℝ :=
  ∑ k : ι, p k * Real.log (p k)

/-- Shannon entropy: `H(p) = -∑ pₖ log pₖ = ∑ negMulLog(pₖ)`. -/
noncomputable def shannonEntropy (p : ι → ℝ) : ℝ :=
  -negEntropy p

lemma shannonEntropy_eq_sum_negMulLog (p : ι → ℝ) :
    shannonEntropy p = ∑ k : ι, negMulLog (p k) := by
  simp [shannonEntropy, negEntropy, negMulLog, Finset.sum_neg_distrib, mul_comm]

/-- On the standard simplex, negative entropy is nonpositive (entropy is nonneg).
    This follows from Jensen's inequality applied to the concave function `log`. -/
theorem negEntropy_nonpos_on_simplex (hp : p ∈ stdSimplex ℝ ι) :
    negEntropy p ≤ 0 := by
  unfold negEntropy
  apply Finset.sum_nonpos
  intro k _
  -- Each term p_k * log(p_k) ≤ 0 on the simplex
  rcases eq_or_lt_of_le (hp.1 k) with h0 | h_pos
  · -- p k = 0: 0 * log 0 = 0 ≤ 0
    rw [← h0]; simp
  · -- 0 < p k ≤ 1: log(p k) ≤ 0
    have hle1 : p k ≤ 1 := by
      have : p k ≤ ∑ j : ι, p j := by
        apply Finset.single_le_sum (fun j _ => hp.1 j) (Finset.mem_univ k)
      linarith [hp.2]
    exact mul_nonpos_of_nonneg_of_nonpos (le_of_lt h_pos)
      (Real.log_nonpos (le_of_lt h_pos) hle1)

/-- Negative entropy is zero iff `p` is a vertex of the simplex (Dirac distribution). -/
theorem negEntropy_eq_zero_iff_vertex [DecidableEq ι] (hp : p ∈ stdSimplex ℝ ι) :
    negEntropy p = 0 ↔ ∃ k : ι, p = fun i => if i = k then 1 else 0 := by
  constructor
  · -- Forward: negEntropy = 0 → vertex
    intro h0
    unfold negEntropy at h0
    -- Each term p k * log (p k) ≤ 0, and they sum to 0
    have hall : ∀ k, p k * log (p k) = 0 := by
      have hle : ∀ i ∈ Finset.univ, p i * log (p i) ≤ 0 := by
        intro k _
        rcases eq_or_lt_of_le (hp.1 k) with h0' | h_pos
        · rw [← h0']; simp
        · exact mul_nonpos_of_nonneg_of_nonpos (le_of_lt h_pos)
            (log_nonpos (le_of_lt h_pos)
              (by linarith [Finset.single_le_sum (fun j _ => hp.1 j) (Finset.mem_univ k), hp.2]))
      exact fun k => (Finset.sum_eq_zero_iff_of_nonpos hle).mp h0 k (Finset.mem_univ k)
    -- p k * log(p k) = 0 means p k = 0 or p k = 1
    have h01 : ∀ k, p k = 0 ∨ p k = 1 := by
      intro k
      rcases eq_or_lt_of_le (hp.1 k) with h | h
      · exact Or.inl h.symm
      · -- 0 < p k, so log(p k) = 0
        have hlog : log (p k) = 0 := by
          rcases mul_eq_zero.mp (hall k) with hp0 | hlog
          · linarith
          · exact hlog
        rw [log_eq_zero] at hlog
        rcases hlog with h1 | h2 | h3
        · linarith  -- p k = 0 contradicts 0 < p k
        · exact Or.inr h2
        · linarith  -- p k = -1 contradicts 0 < p k
    -- Each p k ∈ {0, 1} and ∑ p = 1, so exactly one is 1
    have ⟨k₀, hk₀⟩ : ∃ k, p k = 1 := by
      by_contra hall0; push_neg at hall0
      have hzero : ∀ k, p k = 0 := fun k => (h01 k).resolve_right (hall0 k)
      have : ∑ k : ι, p k = 0 := Finset.sum_eq_zero (fun k _ => hzero k)
      linarith [hp.2]
    refine ⟨k₀, funext fun i => ?_⟩
    split_ifs with h
    · rw [h]; exact hk₀
    · -- i ≠ k₀ and p i ∈ {0, 1}; if p i = 1 then ∑ p ≥ 2, contradiction
      rcases h01 i with h0' | h1'
      · exact h0'
      · exfalso
        have hge : p k₀ + p i ≤ ∑ j : ι, p j := by
          rw [← Finset.add_sum_erase _ _ (Finset.mem_univ k₀)]
          gcongr
          exact Finset.single_le_sum (fun j _ => hp.1 j)
            (Finset.mem_erase.mpr ⟨h, Finset.mem_univ i⟩)
        linarith [hk₀, h1', hp.2]
  · -- Backward: vertex → negEntropy = 0
    rintro ⟨k₀, rfl⟩
    unfold negEntropy
    apply Finset.sum_eq_zero
    intro i _
    simp only
    split_ifs <;> simp

/-! ### Gibbs inequality -/

/-- Gibbs inequality: for `p` on the simplex and `q` a positive distribution summing to 1,
    `∑ pₖ log qₖ ≤ ∑ pₖ log pₖ`.
    Proof via the pointwise `log x ≤ x - 1` applied to `x = qₖ/pₖ`. -/
private lemma sum_mul_log_le_negEntropy (hp : p ∈ stdSimplex ℝ ι)
    (q : ι → ℝ) (hq_pos : ∀ k, 0 < q k) (hq_sum : ∑ k : ι, q k = 1) :
    ∑ k : ι, p k * Real.log (q k) ≤ negEntropy p := by
  unfold negEntropy
  -- Decompose: ∑ pk log qk = ∑ pk log pk + ∑ (pk log qk - pk log pk)
  suffices h : ∑ k : ι, (p k * log (q k) - p k * log (p k)) ≤ 0 by
    have hsplit : ∑ k : ι, p k * log (q k) =
        ∑ k : ι, p k * log (p k) + ∑ k : ι, (p k * log (q k) - p k * log (p k)) := by
      rw [← Finset.sum_add_distrib]; congr 1; ext k; ring
    linarith
  calc ∑ k : ι, (p k * log (q k) - p k * log (p k))
      ≤ ∑ k : ι, (q k - p k) := Finset.sum_le_sum fun k _ => by
        rcases eq_or_lt_of_le (hp.1 k) with hpk | hpk
        · -- p k = 0: 0 * log(qk) - 0 * log(0) = 0 ≤ qk
          simp [← hpk, (hq_pos k).le]
        · -- p k > 0: use log(qk/pk) ≤ qk/pk - 1
          calc p k * log (q k) - p k * log (p k)
              = p k * (log (q k) - log (p k)) := by ring
            _ = p k * log (q k / p k) := by
                rw [log_div (ne_of_gt (hq_pos k)) (ne_of_gt hpk)]
            _ ≤ p k * (q k / p k - 1) :=
                mul_le_mul_of_nonneg_left
                  (log_le_sub_one_of_pos (div_pos (hq_pos k) hpk)) hpk.le
            _ = q k - p k := by field_simp
    _ = 0 := by
        simp only [sub_eq_add_neg]
        rw [Finset.sum_add_distrib, Finset.sum_neg_distrib, hq_sum, hp.2, add_neg_cancel]

/-! ### Theorem 2: Conjugate of logSumExp -/

/-- **Theorem 2, on-simplex**: For `p ∈ Δ`, the Fenchel conjugate of `logSumExp b` is
    `b · negEntropy(p) = b · ∑ pₖ log pₖ`.

    **Proof**: Upper bound via Gibbs inequality (`log x ≤ x − 1`).
    Lower bound by exhibiting `Dₖ = b · log pₖ` (interior) or approximating (boundary). -/
theorem fenchelConjugate_logSumExp_simplex (hb : 0 < b)
    (hp : p ∈ stdSimplex ℝ ι) :
    fenchelConjugate (logSumExp b) p = ↑(b * negEntropy p) := by
  apply iSup_eq_of_forall_le_of_forall_lt_exists_gt
  · -- Upper bound: ∀ D, ⟨p,D⟩ - C(D) ≤ b * negEntropy(p)
    intro D
    exact_mod_cast show ∑ k : ι, p k * D k - logSumExp b D ≤ b * negEntropy p from by
      set q := softmax b D
      have hq_pos : ∀ k, 0 < q k := fun k => softmax_pos b D k
      have hq_sum : ∑ k : ι, q k = 1 := softmax_sum_eq_one b D
      have halg : ∑ k : ι, p k * D k - logSumExp b D =
          b * ∑ k : ι, p k * log (q k) := by
        simp only [logSumExp, q, softmax]
        set S := sumExp b D
        have hS : (0 : ℝ) < S := sumExp_pos b D
        have hlog : ∀ k, log (exp (D k / b) / S) = D k / b - log S := fun k =>
          by rw [log_div (ne_of_gt (exp_pos _)) (ne_of_gt hS), log_exp]
        simp_rw [hlog]
        rw [Finset.mul_sum]
        simp_rw [show ∀ k, b * (p k * (D k / b - log S)) =
            p k * D k - p k * (b * log S) from fun k => by field_simp]
        symm
        have hprod : ∑ k : ι, p k * (b * log S) = b * log S := by
          simp_rw [show ∀ k, p k * (b * log S) = (b * log S) * p k from
            fun k => mul_comm _ _]
          rw [← Finset.mul_sum, hp.2, mul_one]
        simp only [sub_eq_add_neg]
        rw [Finset.sum_add_distrib, Finset.sum_neg_distrib, hprod]
      rw [halg]
      exact mul_le_mul_of_nonneg_left
        (sum_mul_log_le_negEntropy hp _ hq_pos hq_sum) hb.le
  · -- Approximation: ∀ w < target, ∃ D with w < g(D).
    -- Uses D k = b * log(p k + ε) which works uniformly for interior and boundary.
    intro w hw
    rcases eq_or_ne w ⊥ with rfl | hbot
    · exact ⟨0, EReal.bot_lt_coe _⟩
    · have htop : w ≠ ⊤ := ne_top_of_lt hw
      set w' := w.toReal
      have hw_eq : w = ↑w' := (EReal.coe_toReal htop hbot).symm
      rw [hw_eq] at hw ⊢
      have hw' : w' < b * negEntropy p := by exact_mod_cast hw
      -- Set up parameters
      set δ := b * negEntropy p - w'
      have hδ : 0 < δ := by linarith
      set nn : ℝ := ↑(Fintype.card ι)
      have hnn : 0 < nn := Nat.cast_pos.mpr Fintype.card_pos
      set ε := δ / (2 * b * nn)
      have hε : 0 < ε := div_pos hδ (by positivity)
      have hpε : ∀ k, 0 < p k + ε := fun k => add_pos_of_nonneg_of_pos (hp.1 k) hε
      -- Exhibit D k = b * log(p k + ε)
      refine ⟨fun k => b * log (p k + ε), ?_⟩
      suffices hreal : w' < ∑ k : ι, p k * (b * log (p k + ε)) -
          logSumExp b (fun k => b * log (p k + ε)) by exact_mod_cast hreal
      -- ∑ pk * log(pk + ε) ≥ negEntropy p (log is monotone, zero terms vanish)
      have h_ip : b * negEntropy p ≤ ∑ k : ι, p k * (b * log (p k + ε)) := by
        rw [show ∑ k, p k * (b * log (p k + ε)) = b * ∑ k, p k * log (p k + ε) from by
          rw [Finset.mul_sum]; exact Finset.sum_congr rfl fun k _ => by ring]
        exact mul_le_mul_of_nonneg_left (by
          unfold negEntropy; exact Finset.sum_le_sum fun k _ => by
            rcases eq_or_lt_of_le (hp.1 k) with hpk | hpk
            · rw [← hpk]; simp
            · exact mul_le_mul_of_nonneg_left
                (log_le_log hpk (le_add_of_nonneg_right hε.le)) hpk.le) hb.le
      -- sumExp = 1 + nn * ε
      have hSE : sumExp b (fun k => b * log (p k + ε)) = 1 + nn * ε := by
        simp only [sumExp]
        simp_rw [show ∀ k, b * log (p k + ε) / b = log (p k + ε) from
          fun k => mul_div_cancel_left₀ _ (ne_of_gt hb)]
        simp_rw [exp_log (hpε _)]
        rw [show ∑ k : ι, (p k + ε) = ∑ k, p k + ∑ k : ι, ε from Finset.sum_add_distrib,
            hp.2, Finset.sum_const, Finset.card_univ, nsmul_eq_mul]
      -- logSumExp ≤ b * nn * ε (from log(1+x) ≤ x)
      have hlog : log (1 + nn * ε) ≤ nn * ε := by
        linarith [log_le_sub_one_of_pos (show (0 : ℝ) < 1 + nn * ε from by positivity)]
      have hLSE : logSumExp b (fun k => b * log (p k + ε)) ≤ b * (nn * ε) := by
        simp only [logSumExp, hSE]
        exact mul_le_mul_of_nonneg_left hlog hb.le
      -- (2*b*nn) * ε = δ, so b*(nn*ε) = δ/2, so logSumExp cost < δ
      have h2bnn : (0 : ℝ) < 2 * b * nn := by positivity
      have hbnε : (2 * b * nn) * ε = δ := by
        rw [mul_comm]; exact div_mul_cancel₀ δ (ne_of_gt h2bnn)
      nlinarith [show δ = b * negEntropy p - w' from rfl]

/-- **Theorem 2, off-simplex**: For `p ∉ Δ`, the conjugate is `+∞`. -/
theorem fenchelConjugate_logSumExp_not_simplex (hb : 0 < b)
    (hp : p ∉ stdSimplex ℝ ι) :
    fenchelConjugate (logSumExp b) p = ⊤ := by
  classical
  -- Suffices: for any M : ℝ, ∃ D with g(D) ≥ M. Then iSup = ⊤.
  suffices h : ∀ M : ℝ, ∃ D : ι → ℝ, M ≤ ∑ k : ι, p k * D k - logSumExp b D by
    by_contra hne
    have hbot : fenchelConjugate (logSumExp b) p ≠ ⊥ :=
      ne_of_gt (lt_of_lt_of_le (EReal.bot_lt_coe _) (le_fenchelConjugate _ p 0))
    set r := (fenchelConjugate (logSumExp b) p).toReal
    have hreal : fenchelConjugate (logSumExp b) p = ↑r :=
      (EReal.coe_toReal hne hbot).symm
    obtain ⟨D, hD⟩ := h (r + 1)
    have h1 : (↑(r + 1) : EReal) ≤ fenchelConjugate (logSumExp b) p :=
      le_trans (by exact_mod_cast hD) (le_fenchelConjugate _ p D)
    have h2 : (r + 1 : ℝ) ≤ r := by exact_mod_cast hreal ▸ h1
    linarith
  -- Now prove: ∀ M, ∃ D, M ≤ g(D)
  -- p ∉ Δ means: (∃ k, p k < 0) ∨ (∑ p k ≠ 1)
  simp only [stdSimplex, Set.mem_setOf_eq, not_and_or, not_forall] at hp
  rcases hp with ⟨k₀, hk₀⟩ | hsum
  · -- Case 1: ∃ k₀ with p k₀ < 0. Use D k = t·δ_{k,k₀}; as t → -∞, g(D) → +∞.
    push_neg at hk₀
    intro M
    set nc : ℝ := ↑(Fintype.card ι)
    have hnc : 0 < nc := Nat.cast_pos.mpr Fintype.card_pos
    set t := min 0 ((M + b * log nc) / p k₀)
    refine ⟨fun k => if k = k₀ then t else 0, ?_⟩
    -- Inner product = p k₀ * t
    have hIP : ∑ k : ι, p k * (if k = k₀ then t else 0) = p k₀ * t := by
      simp only [mul_ite, mul_zero, Finset.sum_ite_eq', Finset.mem_univ, if_true]
    -- sumExp ≤ nc (all exp terms ≤ 1 since D k ≤ 0)
    have hSE : sumExp b (fun k => if k = k₀ then t else 0) ≤ nc := by
      unfold sumExp
      calc ∑ k, exp ((if k = k₀ then t else 0) / b)
          ≤ ∑ _ : ι, (1 : ℝ) := Finset.sum_le_sum fun k _ => by
            rw [← exp_zero]; exact exp_le_exp.mpr
              (div_nonpos_of_nonpos_of_nonneg
                (by split_ifs <;> linarith [min_le_left 0 ((M + b * log nc) / p k₀)]) hb.le)
        _ = nc := by rw [Finset.sum_const, Finset.card_univ, nsmul_eq_mul, mul_one]
    -- logSumExp ≤ b * log nc
    have hLSE : logSumExp b (fun k => if k = k₀ then t else 0) ≤ b * log nc := by
      unfold logSumExp
      exact mul_le_mul_of_nonneg_left (log_le_log (sumExp_pos _ _) hSE) hb.le
    -- p k₀ * t ≥ M + b * log nc (since p k₀ < 0 and t ≤ .../p k₀)
    have hpt : M + b * log nc ≤ p k₀ * t := by
      have h := (le_div_iff_of_neg hk₀).mp
        (min_le_right 0 ((M + b * log nc) / p k₀))
      linarith [mul_comm t (p k₀)]
    linarith [hIP, hLSE, hpt]
  · -- Case 2: ∑ p k ≠ 1. Use constant D k = t for all k.
    intro M
    set s := ∑ k : ι, p k
    have hs : s ≠ 1 := hsum
    set n : ℝ := ↑(Fintype.card ι) with hn_def
    have hn : 0 < n := Nat.cast_pos.mpr Fintype.card_pos
    set t := (M + b * log n) / (s - 1) with ht_def
    refine ⟨fun _ => t, ?_⟩
    have hSE : sumExp b (fun (_ : ι) => t) = n * exp (t / b) := by
      simp only [sumExp, Finset.sum_const, Finset.card_univ, nsmul_eq_mul, hn_def]
    have hLSE : logSumExp b (fun (_ : ι) => t) = b * log n + t := by
      simp only [logSumExp, hSE]
      rw [log_mul (ne_of_gt hn) (ne_of_gt (exp_pos _)), log_exp, mul_add,
          mul_div_cancel₀ t (ne_of_gt hb)]
    have hIP : ∑ k : ι, p k * t = t * s := by
      rw [show ∑ k : ι, p k * t = t * ∑ k : ι, p k from by
        rw [Finset.mul_sum]; exact (Finset.sum_congr rfl fun k _ => by ring).symm]
    -- g(D) = t * s - (b * log n + t) = t * (s - 1) - b * log n = M
    rw [hIP, hLSE]
    have hsub : s - 1 ≠ 0 := sub_ne_zero.mpr hs
    have : t * (s - 1) = M + b * log n := by
      rw [ht_def]; field_simp
    linarith

/-- **Theorem 2** (LMSR–Entropy Duality, combined):
    `(logSumExp b)*(p) = b · ∑ pₖ log pₖ` on `Δ`, and `+∞` off `Δ`. -/
theorem fenchelConjugate_logSumExp_eq (hb : 0 < b) (p : ι → ℝ) :
    (p ∈ stdSimplex ℝ ι → fenchelConjugate (logSumExp b) p = ↑(b * negEntropy p)) ∧
    (p ∉ stdSimplex ℝ ι → fenchelConjugate (logSumExp b) p = ⊤) :=
  ⟨fenchelConjugate_logSumExp_simplex hb, fenchelConjugate_logSumExp_not_simplex hb⟩

end FisherClearing
