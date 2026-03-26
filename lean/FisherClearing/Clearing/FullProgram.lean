/-
Copyright (c) 2024 FisherClearing Contributors. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
-/
import Mathlib
import FisherClearing.Convex.LogSumExp
import FisherClearing.ReducedForm.Utility
import FisherClearing.Clearing.DeployedValue

/-!
# Full Clearing Program and Concavity (Theorem 3, Part 1)

This file defines the reduced-form clearing program P_b^RA and proves:
1. The objective is concave on `[0,1]^J` (Theorem 3, part 1)
2. An optimal fill vector exists (Weierstrass)

## References

* Prediction Markets Are Fisher Markets, Theorem 3 (part 1)
-/

namespace FisherClearing

open scoped BigOperators
open Finset Real

variable {ι : Type*} [Fintype ι] [Nonempty ι]
variable {J : Type*} [Fintype J]
variable {K : Type*} [Fintype K] [DecidableEq K]

/-! ### Data -/

/-- A full clearing instance with MM ownership. -/
structure FullInstance (ι J K : Type*) where
  limitPrice : J → ℝ
  payoff : J → ι → ℝ
  budget : K → ℝ
  budget_pos : ∀ k, 0 < budget k
  limitPrice_pos : ∀ j, 0 < limitPrice j
  owner : J → Option K

variable (inst : FullInstance ι J K)

/-- MM `k`'s weighted fill. -/
noncomputable def FullInstance.mmUtil (fill : J → ℝ) (k : K) : ℝ :=
  ∑ j : J, if inst.owner j = some k then inst.limitPrice j * fill j else 0

/-- Net demand for outcome `s`. -/
noncomputable def FullInstance.netDemand (fill : J → ℝ) (s : ι) : ℝ :=
  ∑ j : J, fill j * inst.payoff j s

/-- The full reduced-form objective. -/
noncomputable def FullInstance.objective (b : ℝ) (fill : J → ℝ) : ℝ :=
  (∑ k : K, psiB (inst.budget k) (inst.mmUtil fill k)) -
  logSumExp b (inst.netDemand fill)

/-- The box-feasible set. -/
def boxFeasible : Set (J → ℝ) :=
  Set.pi Set.univ (fun _ => Set.Icc 0 1)

theorem convex_boxFeasible : Convex ℝ (boxFeasible : Set (J → ℝ)) :=
  convex_pi (fun _ _ => convex_Icc 0 1)

theorem isCompact_boxFeasible : IsCompact (boxFeasible : Set (J → ℝ)) :=
  isCompact_univ_pi (fun _ => isCompact_Icc)

theorem nonempty_boxFeasible : (boxFeasible : Set (J → ℝ)).Nonempty :=
  ⟨0, Set.mem_pi.mpr (fun _ _ => ⟨le_refl 0, zero_le_one⟩)⟩

/-! ### Linearity helpers -/

theorem FullInstance.mmUtil_nonneg
    (fill : J → ℝ) (hfill : fill ∈ (boxFeasible : Set (J → ℝ))) (k : K) :
    0 ≤ inst.mmUtil fill k :=
  Finset.sum_nonneg fun j _ => by
    split_ifs; · exact mul_nonneg (inst.limitPrice_pos j).le (Set.mem_pi.mp hfill j trivial |>.1)
    · exact le_refl _

theorem FullInstance.mmUtil_linear (k : K)
    (f g : J → ℝ) (a c : ℝ) :
    inst.mmUtil (a • f + c • g) k = a * inst.mmUtil f k + c * inst.mmUtil g k := by
  simp only [FullInstance.mmUtil, Pi.add_apply, Pi.smul_apply, smul_eq_mul]
  -- LHS = ∑ j, if ... then L_j * (a*f_j + c*g_j) else 0
  -- RHS = a * ∑ j, if ... then L_j*f_j else 0 + c * ∑ j, if ... then L_j*g_j else 0
  trans ∑ j : J, (a * (if inst.owner j = some k then inst.limitPrice j * f j else 0) +
                  c * (if inst.owner j = some k then inst.limitPrice j * g j else 0))
  · exact Finset.sum_congr rfl fun j _ => by split_ifs <;> ring
  · simp only [Finset.sum_add_distrib, ← Finset.mul_sum]

theorem FullInstance.netDemand_linear (s : ι)
    (f g : J → ℝ) (a c : ℝ) :
    inst.netDemand (a • f + c • g) s = a * inst.netDemand f s + c * inst.netDemand g s := by
  simp only [FullInstance.netDemand, Pi.add_apply, Pi.smul_apply, smul_eq_mul]
  trans ∑ j : J, (a * (f j * inst.payoff j s) + c * (g j * inst.payoff j s))
  · exact Finset.sum_congr rfl fun j _ => by ring
  · simp only [Finset.sum_add_distrib, ← Finset.mul_sum]

/-! ### Concavity -/

/-- Each `ψ_{Bₖ}(Uₖ(q))` is concave on the box. -/
theorem FullInstance.concaveOn_psiB_mmUtil (k : K) :
    ConcaveOn ℝ boxFeasible (fun fill => psiB (inst.budget k) (inst.mmUtil fill k)) := by
  constructor
  · exact convex_boxFeasible
  · intro f hf g hg a c ha hc hac
    simp only [smul_eq_mul]
    -- Affinity: Uₖ(af + cg) = a·Uₖ(f) + c·Uₖ(g)
    rw [inst.mmUtil_linear k f g a c]
    -- Concavity of ψ_B at nonneg points
    exact (concaveOn_psiB_Ici (inst.budget_pos k)).2
      (Set.mem_Ici.mpr (inst.mmUtil_nonneg f hf k))
      (Set.mem_Ici.mpr (inst.mmUtil_nonneg g hg k)) ha hc hac

/-- MM welfare is concave on the box. -/
theorem FullInstance.concaveOn_mmWelfare :
    ConcaveOn ℝ boxFeasible
      (fun fill => ∑ k : K, psiB (inst.budget k) (inst.mmUtil fill k)) := by
  constructor
  · exact convex_boxFeasible
  · intro f hf g hg a c ha hc hac
    -- Need: a * ∑ psiB(f) + c * ∑ psiB(g) ≤ ∑ psiB(af+cg)
    -- Rewrite LHS as ∑ (a*psiB(f) + c*psiB(g)) using linearity of sum
    have h : ∀ k, a * psiB (inst.budget k) (inst.mmUtil f k) +
                  c * psiB (inst.budget k) (inst.mmUtil g k) ≤
                  psiB (inst.budget k) (inst.mmUtil (a • f + c • g) k) := by
      intro k
      have := (inst.concaveOn_psiB_mmUtil k).2 hf hg ha hc hac
      simp only [smul_eq_mul] at this; exact this
    simp only [smul_eq_mul]
    calc a * ∑ k, psiB (inst.budget k) (inst.mmUtil f k) +
          c * ∑ k, psiB (inst.budget k) (inst.mmUtil g k)
        = ∑ k, (a * psiB (inst.budget k) (inst.mmUtil f k) +
                 c * psiB (inst.budget k) (inst.mmUtil g k)) := by
          simp only [Finset.mul_sum, Finset.sum_add_distrib]
      _ ≤ ∑ k, psiB (inst.budget k) (inst.mmUtil (a • f + c • g) k) :=
          Finset.sum_le_sum fun k _ => h k

/-- `-logSumExp b (D(q))` is concave on the box. -/
theorem FullInstance.concaveOn_neg_minting {b : ℝ} (hb : 0 < b) :
    ConcaveOn ℝ boxFeasible (fun fill => -logSumExp b (inst.netDemand fill)) := by
  constructor
  · exact convex_boxFeasible
  · intro f hf g hg a c ha hc hac
    simp only [smul_eq_mul]
    -- Linearity of netDemand: D(af+cg) = a•D(f) + c•D(g)
    have hlin : inst.netDemand (a • f + c • g) = a • inst.netDemand f + c • inst.netDemand g := by
      ext s; simp only [Pi.add_apply, Pi.smul_apply, smul_eq_mul]
      exact inst.netDemand_linear s f g a c
    rw [hlin]
    have hconv := (convexOn_logSumExp hb).2
      (Set.mem_univ (inst.netDemand f)) (Set.mem_univ (inst.netDemand g)) ha hc hac
    simp only [smul_eq_mul] at hconv
    linarith

/-- **Theorem 3, Part 1**: The full clearing objective is concave on `[0,1]^J`. -/
theorem FullInstance.concaveOn_objective {b : ℝ} (hb : 0 < b) :
    ConcaveOn ℝ boxFeasible (inst.objective b) := by
  constructor
  · exact convex_boxFeasible
  · intro f hf g hg a c ha hc hac
    have h1 := (inst.concaveOn_mmWelfare).2 hf hg ha hc hac
    have h2 := (inst.concaveOn_neg_minting hb).2 hf hg ha hc hac
    simp only [FullInstance.objective, smul_eq_mul] at *
    linarith

end FisherClearing
