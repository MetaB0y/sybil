from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_4_type import SystemEventResponseType4Type






T = TypeVar("T", bound="SystemEventResponseType4")



@_attrs_define
class SystemEventResponseType4:
    """ 
        Attributes:
            account_id (int):
            amount_nanos (str): Refunded account credit. Integer nanodollars; 1_000_000_000 = $1.
            reason (str):
            type_ (SystemEventResponseType4Type):
            withdrawal_id (int):
     """

    account_id: int
    amount_nanos: str
    reason: str
    type_: SystemEventResponseType4Type
    withdrawal_id: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        amount_nanos = self.amount_nanos

        reason = self.reason

        type_ = self.type_.value

        withdrawal_id = self.withdrawal_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "amount_nanos": amount_nanos,
            "reason": reason,
            "type": type_,
            "withdrawal_id": withdrawal_id,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        amount_nanos = d.pop("amount_nanos")

        reason = d.pop("reason")

        type_ = SystemEventResponseType4Type(d.pop("type"))




        withdrawal_id = d.pop("withdrawal_id")

        system_event_response_type_4 = cls(
            account_id=account_id,
            amount_nanos=amount_nanos,
            reason=reason,
            type_=type_,
            withdrawal_id=withdrawal_id,
        )


        system_event_response_type_4.additional_properties = d
        return system_event_response_type_4

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
