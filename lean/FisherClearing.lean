-- FisherClearing: Lean 4 formalization of "Prediction Markets Are Fisher Markets"
--
-- Key results:
--   Phase 1: Log-sum-exp convexity, softmax simplex membership, sandwich bounds
--   Phase 2: Fenchel conjugate of max = simplex indicator, of LSE = entropy
--   Phase 3: Reduced-form MM utility, LP recovery, welfare gap
--   Phase 4: Clearing program, price uniqueness

import FisherClearing.Convex.LogSumExp
import FisherClearing.Convex.Softmax
import FisherClearing.Duality.SandwichBound
import FisherClearing.Convex.FenchelConjugate
import FisherClearing.Duality.MintingSimplex
import FisherClearing.Duality.LmsrEntropy
import FisherClearing.ReducedForm.Utility
import FisherClearing.ReducedForm.LpRecovery
import FisherClearing.ReducedForm.WelfareGap
import FisherClearing.Clearing.Program
import FisherClearing.Clearing.PriceUniqueness
