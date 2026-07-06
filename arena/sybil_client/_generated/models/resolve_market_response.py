from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ResolveMarketResponse")



@_attrs_define
class ResolveMarketResponse:
    """ 
        Attributes:
            market_id (int):
            payout_nanos (int): Resolution payout per YES share. Integer nanodollars;
                1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
            status (str):
            challenge_deadline_ms (int | None | Unset):
     """

    market_id: int
    payout_nanos: int
    status: str
    challenge_deadline_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        payout_nanos = self.payout_nanos

        status = self.status

        challenge_deadline_ms: int | None | Unset
        if isinstance(self.challenge_deadline_ms, Unset):
            challenge_deadline_ms = UNSET
        else:
            challenge_deadline_ms = self.challenge_deadline_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "payout_nanos": payout_nanos,
            "status": status,
        })
        if challenge_deadline_ms is not UNSET:
            field_dict["challenge_deadline_ms"] = challenge_deadline_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        payout_nanos = d.pop("payout_nanos")

        status = d.pop("status")

        def _parse_challenge_deadline_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        challenge_deadline_ms = _parse_challenge_deadline_ms(d.pop("challenge_deadline_ms", UNSET))


        resolve_market_response = cls(
            market_id=market_id,
            payout_nanos=payout_nanos,
            status=status,
            challenge_deadline_ms=challenge_deadline_ms,
        )


        resolve_market_response.additional_properties = d
        return resolve_market_response

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
