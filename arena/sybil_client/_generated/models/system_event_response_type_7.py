from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_7_type import SystemEventResponseType7Type
from typing import cast






T = TypeVar("T", bound="SystemEventResponseType7")



@_attrs_define
class SystemEventResponseType7:
    """ 
        Attributes:
            affected_accounts (list[int]):
            market_id (int):
            payout_nanos (int): Resolution payout per YES share. Integer nanodollars;
                1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
            type_ (SystemEventResponseType7Type):
     """

    affected_accounts: list[int]
    market_id: int
    payout_nanos: int
    type_: SystemEventResponseType7Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        affected_accounts = self.affected_accounts



        market_id = self.market_id

        payout_nanos = self.payout_nanos

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "affected_accounts": affected_accounts,
            "market_id": market_id,
            "payout_nanos": payout_nanos,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        affected_accounts = cast(list[int], d.pop("affected_accounts"))


        market_id = d.pop("market_id")

        payout_nanos = d.pop("payout_nanos")

        type_ = SystemEventResponseType7Type(d.pop("type"))




        system_event_response_type_7 = cls(
            affected_accounts=affected_accounts,
            market_id=market_id,
            payout_nanos=payout_nanos,
            type_=type_,
        )


        system_event_response_type_7.additional_properties = d
        return system_event_response_type_7

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
