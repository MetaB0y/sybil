from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.auto_resolution_action_dto import AutoResolutionActionDto
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="AutoResolutionEntryResponse")



@_attrs_define
class AutoResolutionEntryResponse:
    """ One entry on the automated-resolution review board (SYB-48).

        Attributes:
            action (AutoResolutionActionDto): Which confidence tier a proposed automated resolution (SYB-48) landed in.
                Mirrors the resolver-side confidence policy so the review board can render
                and gate each entry consistently.
            confidence (float): Model confidence in [0, 1].
            market_id (int):
            payout_nanos (int): Proposed YES payout per share. Integer nanodollars; 1_000_000_000 = $1.
                Payouts are per-share probabilities in [0, 1e9].
            proposed_at_ms (int): When the proposal was first recorded. Unix milliseconds.
            reasoning (str): Model's free-text justification.
            status (str): Display status derived at read time from the operator decision AND the
                market's live on-chain state: one of `pending`, `needs_review`,
                `escalated`, `approved`, `rejected`, `resolved`.
            decided_at_ms (int | None | Unset): When an operator approved/rejected, if they did. Unix milliseconds.
            eta_ms (int | None | Unset): Auto-finalize deadline for `propose` entries. Unix milliseconds.
            evidence_excerpts (list[str] | Unset): Short verbatim excerpts from the fetched source.
     """

    action: AutoResolutionActionDto
    confidence: float
    market_id: int
    payout_nanos: int
    proposed_at_ms: int
    reasoning: str
    status: str
    decided_at_ms: int | None | Unset = UNSET
    eta_ms: int | None | Unset = UNSET
    evidence_excerpts: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        confidence = self.confidence

        market_id = self.market_id

        payout_nanos = self.payout_nanos

        proposed_at_ms = self.proposed_at_ms

        reasoning = self.reasoning

        status = self.status

        decided_at_ms: int | None | Unset
        if isinstance(self.decided_at_ms, Unset):
            decided_at_ms = UNSET
        else:
            decided_at_ms = self.decided_at_ms

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
            "proposed_at_ms": proposed_at_ms,
            "reasoning": reasoning,
            "status": status,
        })
        if decided_at_ms is not UNSET:
            field_dict["decided_at_ms"] = decided_at_ms
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

        proposed_at_ms = d.pop("proposed_at_ms")

        reasoning = d.pop("reasoning")

        status = d.pop("status")

        def _parse_decided_at_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        decided_at_ms = _parse_decided_at_ms(d.pop("decided_at_ms", UNSET))


        def _parse_eta_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        eta_ms = _parse_eta_ms(d.pop("eta_ms", UNSET))


        evidence_excerpts = cast(list[str], d.pop("evidence_excerpts", UNSET))


        auto_resolution_entry_response = cls(
            action=action,
            confidence=confidence,
            market_id=market_id,
            payout_nanos=payout_nanos,
            proposed_at_ms=proposed_at_ms,
            reasoning=reasoning,
            status=status,
            decided_at_ms=decided_at_ms,
            eta_ms=eta_ms,
            evidence_excerpts=evidence_excerpts,
        )


        auto_resolution_entry_response.additional_properties = d
        return auto_resolution_entry_response

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
