from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_6_type import SystemEventResponseType6Type






T = TypeVar("T", bound="SystemEventResponseType6")



@_attrs_define
class SystemEventResponseType6:
    """ 
        Attributes:
            height (int):
            type_ (SystemEventResponseType6Type):
     """

    height: int
    type_: SystemEventResponseType6Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        height = self.height

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "height": height,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        height = d.pop("height")

        type_ = SystemEventResponseType6Type(d.pop("type"))




        system_event_response_type_6 = cls(
            height=height,
            type_=type_,
        )


        system_event_response_type_6.additional_properties = d
        return system_event_response_type_6

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
