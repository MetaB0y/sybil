from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="ResolveMarketResponse")



@_attrs_define
class ResolveMarketResponse:
    """ 
        Attributes:
            market_id (int):
            payout_nanos (str): Resolution payout per YES share. Integer nanodollars;
                1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
            status (str):
     """

    market_id: int
    payout_nanos: str
    status: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        payout_nanos = self.payout_nanos

        status = self.status


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "payout_nanos": payout_nanos,
            "status": status,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        payout_nanos = d.pop("payout_nanos")

        status = d.pop("status")

        resolve_market_response = cls(
            market_id=market_id,
            payout_nanos=payout_nanos,
            status=status,
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
