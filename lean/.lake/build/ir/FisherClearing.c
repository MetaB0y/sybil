// Lean compiler output
// Module: FisherClearing
// Imports: public import Init public import FisherClearing.Convex.LogSumExp public import FisherClearing.Convex.Softmax public import FisherClearing.Duality.SandwichBound public import FisherClearing.Convex.FenchelConjugate public import FisherClearing.Duality.MintingSimplex public import FisherClearing.Duality.LmsrEntropy public import FisherClearing.ReducedForm.Utility public import FisherClearing.ReducedForm.LpRecovery public import FisherClearing.ReducedForm.WelfareGap public import FisherClearing.Clearing.Program public import FisherClearing.Clearing.PriceUniqueness public import FisherClearing.Clearing.DeployedValue public import FisherClearing.Clearing.FullProgram public import FisherClearing.Clearing.KKT public import FisherClearing.Clearing.DemandConcavity public import FisherClearing.Clearing.PriceDuality public import FisherClearing.Clearing.ConicFormulation
#include <lean/lean.h>
#if defined(__clang__)
#pragma clang diagnostic ignored "-Wunused-parameter"
#pragma clang diagnostic ignored "-Wunused-label"
#elif defined(__GNUC__) && !defined(__CLANG__)
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wunused-label"
#pragma GCC diagnostic ignored "-Wunused-but-set-variable"
#endif
#ifdef __cplusplus
extern "C" {
#endif
lean_object* initialize_Init(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Convex_LogSumExp(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Convex_Softmax(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Duality_SandwichBound(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Convex_FenchelConjugate(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Duality_MintingSimplex(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Duality_LmsrEntropy(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_ReducedForm_Utility(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_ReducedForm_LpRecovery(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_ReducedForm_WelfareGap(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_Program(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_PriceUniqueness(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_DeployedValue(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_FullProgram(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_KKT(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_DemandConcavity(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_PriceDuality(uint8_t builtin);
lean_object* initialize_FisherClearing_FisherClearing_Clearing_ConicFormulation(uint8_t builtin);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_FisherClearing_FisherClearing(uint8_t builtin) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Convex_LogSumExp(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Convex_Softmax(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Duality_SandwichBound(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Convex_FenchelConjugate(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Duality_MintingSimplex(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Duality_LmsrEntropy(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_ReducedForm_Utility(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_ReducedForm_LpRecovery(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_ReducedForm_WelfareGap(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_Program(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_PriceUniqueness(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_DeployedValue(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_FullProgram(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_KKT(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_DemandConcavity(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_PriceDuality(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_FisherClearing_FisherClearing_Clearing_ConicFormulation(builtin);
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif
