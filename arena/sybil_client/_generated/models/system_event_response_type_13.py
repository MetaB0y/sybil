from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_13_type import SystemEventResponseType13Type






T = TypeVar("T", bound="SystemEventResponseType13")



@_attrs_define
class SystemEventResponseType13:
    """ 
        Attributes:
            amount_nanos (str): Amount parked in the system ledger. Integer nanodollars; 1_000_000_000 = $1.
            deposit_id (int):
            deposit_root_hex (str):
            sybil_account_key_hex (str):
            type_ (SystemEventResponseType13Type):
     """

    amount_nanos: str
    deposit_id: int
    deposit_root_hex: str
    sybil_account_key_hex: str
    type_: SystemEventResponseType13Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        amount_nanos = self.amount_nanos

        deposit_id = self.deposit_id

        deposit_root_hex = self.deposit_root_hex

        sybil_account_key_hex = self.sybil_account_key_hex

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "amount_nanos": amount_nanos,
            "deposit_id": deposit_id,
            "deposit_root_hex": deposit_root_hex,
            "sybil_account_key_hex": sybil_account_key_hex,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        amount_nanos = d.pop("amount_nanos")

        deposit_id = d.pop("deposit_id")

        deposit_root_hex = d.pop("deposit_root_hex")

        sybil_account_key_hex = d.pop("sybil_account_key_hex")

        type_ = SystemEventResponseType13Type(d.pop("type"))




        system_event_response_type_13 = cls(
            amount_nanos=amount_nanos,
            deposit_id=deposit_id,
            deposit_root_hex=deposit_root_hex,
            sybil_account_key_hex=sybil_account_key_hex,
            type_=type_,
        )


        system_event_response_type_13.additional_properties = d
        return system_event_response_type_13

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
