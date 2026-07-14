from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_16_type import SystemEventResponseType16Type






T = TypeVar("T", bound="SystemEventResponseType16")



@_attrs_define
class SystemEventResponseType16:
    """ 
        Attributes:
            account_id (int):
            market_id (int):
            quantity (int): Complete-set size. Integer share-units; 1000 = 1 share.
            type_ (SystemEventResponseType16Type):
     """

    account_id: int
    market_id: int
    quantity: int
    type_: SystemEventResponseType16Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        market_id = self.market_id

        quantity = self.quantity

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "market_id": market_id,
            "quantity": quantity,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        market_id = d.pop("market_id")

        quantity = d.pop("quantity")

        type_ = SystemEventResponseType16Type(d.pop("type"))




        system_event_response_type_16 = cls(
            account_id=account_id,
            market_id=market_id,
            quantity=quantity,
            type_=type_,
        )


        system_event_response_type_16.additional_properties = d
        return system_event_response_type_16

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
