from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="PublicBridgeBlockResponse")



@_attrs_define
class PublicBridgeBlockResponse:
    """ 
        Attributes:
            deposit_count (int):
            deposit_root_hex (str):
     """

    deposit_count: int
    deposit_root_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        deposit_count = self.deposit_count

        deposit_root_hex = self.deposit_root_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposit_count": deposit_count,
            "deposit_root_hex": deposit_root_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        deposit_count = d.pop("deposit_count")

        deposit_root_hex = d.pop("deposit_root_hex")

        public_bridge_block_response = cls(
            deposit_count=deposit_count,
            deposit_root_hex=deposit_root_hex,
        )


        public_bridge_block_response.additional_properties = d
        return public_bridge_block_response

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
