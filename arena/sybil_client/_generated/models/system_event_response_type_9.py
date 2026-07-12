from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_9_type import SystemEventResponseType9Type






T = TypeVar("T", bound="SystemEventResponseType9")



@_attrs_define
class SystemEventResponseType9:
    """ 
        Attributes:
            group_id (int):
            market_id (int):
            type_ (SystemEventResponseType9Type):
     """

    group_id: int
    market_id: int
    type_: SystemEventResponseType9Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        group_id = self.group_id

        market_id = self.market_id

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "group_id": group_id,
            "market_id": market_id,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        group_id = d.pop("group_id")

        market_id = d.pop("market_id")

        type_ = SystemEventResponseType9Type(d.pop("type"))




        system_event_response_type_9 = cls(
            group_id=group_id,
            market_id=market_id,
            type_=type_,
        )


        system_event_response_type_9.additional_properties = d
        return system_event_response_type_9

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
