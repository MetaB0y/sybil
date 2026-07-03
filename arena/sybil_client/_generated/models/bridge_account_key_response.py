from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="BridgeAccountKeyResponse")



@_attrs_define
class BridgeAccountKeyResponse:
    """ 
        Attributes:
            account_id (int):
            sybil_account_key_hex (str):
     """

    account_id: int
    sybil_account_key_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        sybil_account_key_hex = self.sybil_account_key_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "sybil_account_key_hex": sybil_account_key_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        sybil_account_key_hex = d.pop("sybil_account_key_hex")

        bridge_account_key_response = cls(
            account_id=account_id,
            sybil_account_key_hex=sybil_account_key_hex,
        )


        bridge_account_key_response.additional_properties = d
        return bridge_account_key_response

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
