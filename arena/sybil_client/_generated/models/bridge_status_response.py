from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="BridgeStatusResponse")



@_attrs_define
class BridgeStatusResponse:
    """ 
        Attributes:
            deposit_cursor (int):
            deposit_root_hex (str):
            next_withdrawal_id (int):
            withdrawal_count (int):
     """

    deposit_cursor: int
    deposit_root_hex: str
    next_withdrawal_id: int
    withdrawal_count: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        deposit_cursor = self.deposit_cursor

        deposit_root_hex = self.deposit_root_hex

        next_withdrawal_id = self.next_withdrawal_id

        withdrawal_count = self.withdrawal_count


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposit_cursor": deposit_cursor,
            "deposit_root_hex": deposit_root_hex,
            "next_withdrawal_id": next_withdrawal_id,
            "withdrawal_count": withdrawal_count,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        deposit_cursor = d.pop("deposit_cursor")

        deposit_root_hex = d.pop("deposit_root_hex")

        next_withdrawal_id = d.pop("next_withdrawal_id")

        withdrawal_count = d.pop("withdrawal_count")

        bridge_status_response = cls(
            deposit_cursor=deposit_cursor,
            deposit_root_hex=deposit_root_hex,
            next_withdrawal_id=next_withdrawal_id,
            withdrawal_count=withdrawal_count,
        )


        bridge_status_response.additional_properties = d
        return bridge_status_response

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
