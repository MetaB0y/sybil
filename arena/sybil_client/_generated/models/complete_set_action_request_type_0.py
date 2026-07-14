from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.complete_set_action_request_type_0_action import CompleteSetActionRequestType0Action






T = TypeVar("T", bound="CompleteSetActionRequestType0")



@_attrs_define
class CompleteSetActionRequestType0:
    """ 
        Attributes:
            action (CompleteSetActionRequestType0Action):
            market_id (int):
            quantity (int): Complete-set size. Integer share-units; 1000 = 1 share.
     """

    action: CompleteSetActionRequestType0Action
    market_id: int
    quantity: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        market_id = self.market_id

        quantity = self.quantity


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "action": action,
            "market_id": market_id,
            "quantity": quantity,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        action = CompleteSetActionRequestType0Action(d.pop("action"))




        market_id = d.pop("market_id")

        quantity = d.pop("quantity")

        complete_set_action_request_type_0 = cls(
            action=action,
            market_id=market_id,
            quantity=quantity,
        )


        complete_set_action_request_type_0.additional_properties = d
        return complete_set_action_request_type_0

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
