from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_14_type import SystemEventResponseType14Type






T = TypeVar("T", bound="SystemEventResponseType14")



@_attrs_define
class SystemEventResponseType14:
    """ 
        Attributes:
            account_id (int):
            amount_nanos (int): Amount moved into the account. Integer nanodollars; 1_000_000_000 = $1.
            sybil_account_key_hex (str):
            type_ (SystemEventResponseType14Type):
     """

    account_id: int
    amount_nanos: int
    sybil_account_key_hex: str
    type_: SystemEventResponseType14Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        amount_nanos = self.amount_nanos

        sybil_account_key_hex = self.sybil_account_key_hex

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "amount_nanos": amount_nanos,
            "sybil_account_key_hex": sybil_account_key_hex,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        amount_nanos = d.pop("amount_nanos")

        sybil_account_key_hex = d.pop("sybil_account_key_hex")

        type_ = SystemEventResponseType14Type(d.pop("type"))




        system_event_response_type_14 = cls(
            account_id=account_id,
            amount_nanos=amount_nanos,
            sybil_account_key_hex=sybil_account_key_hex,
            type_=type_,
        )


        system_event_response_type_14.additional_properties = d
        return system_event_response_type_14

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
