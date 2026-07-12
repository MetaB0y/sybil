from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_10_type import SystemEventResponseType10Type






T = TypeVar("T", bound="SystemEventResponseType10")



@_attrs_define
class SystemEventResponseType10:
    """ 
        Attributes:
            account_id (int):
            auth_scheme (int):
            capability_mask (int):
            public_key_hex (str):
            type_ (SystemEventResponseType10Type):
     """

    account_id: int
    auth_scheme: int
    capability_mask: int
    public_key_hex: str
    type_: SystemEventResponseType10Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        auth_scheme = self.auth_scheme

        capability_mask = self.capability_mask

        public_key_hex = self.public_key_hex

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "auth_scheme": auth_scheme,
            "capability_mask": capability_mask,
            "public_key_hex": public_key_hex,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        auth_scheme = d.pop("auth_scheme")

        capability_mask = d.pop("capability_mask")

        public_key_hex = d.pop("public_key_hex")

        type_ = SystemEventResponseType10Type(d.pop("type"))




        system_event_response_type_10 = cls(
            account_id=account_id,
            auth_scheme=auth_scheme,
            capability_mask=capability_mask,
            public_key_hex=public_key_hex,
            type_=type_,
        )


        system_event_response_type_10.additional_properties = d
        return system_event_response_type_10

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
