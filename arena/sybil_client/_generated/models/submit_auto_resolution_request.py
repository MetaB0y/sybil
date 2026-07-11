from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.auto_resolution_action_dto import AutoResolutionActionDto
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="SubmitAutoResolutionRequest")



@_attrs_define
class SubmitAutoResolutionRequest:
    """ Body of `POST /v1/admin/auto-resolutions` (SYB-48). The auto-resolution
    resolver (sybil-polymarket) submits one of these per market it has
    evaluated with an LLM. This route NEVER settles a market: it only records a
    reviewable proposal. Finalization always flows back through the existing
    signed `POST /v1/markets/{id}/resolve` money path.

        Attributes:
            action (AutoResolutionActionDto): Which confidence tier a proposed automated resolution (SYB-48) landed in.
                Mirrors the resolver-side confidence policy so the review board can render
                and gate each entry consistently.
            confidence (float): Model confidence in [0, 1].
            market_id (int): Market the proposal is for.
            payout_nanos (int): Proposed YES payout per share. Integer nanodollars; 1_000_000_000 = $1.
                Payouts are per-share probabilities in [0, 1e9]. Example: 1000000000.
            reasoning (str): Model's free-text justification. Stored verbatim for review.
            eta_ms (int | None | Unset): Wall-clock deadline after which a `propose` entry may auto-finalize.
                Unix milliseconds. Required for `propose`; ignored otherwise.
            evidence_excerpts (list[str] | Unset): Short verbatim excerpts from the fetched source the model relied on.
     """

    action: AutoResolutionActionDto
    confidence: float
    market_id: int
    payout_nanos: int
    reasoning: str
    eta_ms: int | None | Unset = UNSET
    evidence_excerpts: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        confidence = self.confidence

        market_id = self.market_id

        payout_nanos = self.payout_nanos

        reasoning = self.reasoning

        eta_ms: int | None | Unset
        if isinstance(self.eta_ms, Unset):
            eta_ms = UNSET
        else:
            eta_ms = self.eta_ms

        evidence_excerpts: list[str] | Unset = UNSET
        if not isinstance(self.evidence_excerpts, Unset):
            evidence_excerpts = self.evidence_excerpts




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "action": action,
            "confidence": confidence,
            "market_id": market_id,
            "payout_nanos": payout_nanos,
            "reasoning": reasoning,
        })
        if eta_ms is not UNSET:
            field_dict["eta_ms"] = eta_ms
        if evidence_excerpts is not UNSET:
            field_dict["evidence_excerpts"] = evidence_excerpts

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        action = AutoResolutionActionDto(d.pop("action"))




        confidence = d.pop("confidence")

        market_id = d.pop("market_id")

        payout_nanos = d.pop("payout_nanos")

        reasoning = d.pop("reasoning")

        def _parse_eta_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        eta_ms = _parse_eta_ms(d.pop("eta_ms", UNSET))


        evidence_excerpts = cast(list[str], d.pop("evidence_excerpts", UNSET))


        submit_auto_resolution_request = cls(
            action=action,
            confidence=confidence,
            market_id=market_id,
            payout_nanos=payout_nanos,
            reasoning=reasoning,
            eta_ms=eta_ms,
            evidence_excerpts=evidence_excerpts,
        )


        submit_auto_resolution_request.additional_properties = d
        return submit_auto_resolution_request

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
