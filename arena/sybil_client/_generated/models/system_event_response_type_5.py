from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_5_type import SystemEventResponseType5Type






T = TypeVar("T", bound="SystemEventResponseType5")



@_attrs_define
class SystemEventResponseType5:
    """ 
        Attributes:
            account_id (int):
            amount_nanos (int): Finalized withdrawal amount. Integer nanodollars; 1_000_000_000 = $1.
            type_ (SystemEventResponseType5Type):
            withdrawal_id (int):
     """

    account_id: int
    amount_nanos: int
    type_: SystemEventResponseType5Type
    withdrawal_id: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        amount_nanos = self.amount_nanos

        type_ = self.type_.value

        withdrawal_id = self.withdrawal_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "amount_nanos": amount_nanos,
            "type": type_,
            "withdrawal_id": withdrawal_id,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        amount_nanos = d.pop("amount_nanos")

        type_ = SystemEventResponseType5Type(d.pop("type"))




        withdrawal_id = d.pop("withdrawal_id")

        system_event_response_type_5 = cls(
            account_id=account_id,
            amount_nanos=amount_nanos,
            type_=type_,
            withdrawal_id=withdrawal_id,
        )


        system_event_response_type_5.additional_properties = d
        return system_event_response_type_5

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
