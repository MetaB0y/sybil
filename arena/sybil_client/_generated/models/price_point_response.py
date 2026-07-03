from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="PricePointResponse")



@_attrs_define
class PricePointResponse:
    """ 
        Attributes:
            height (int):
            no_price_nanos (int):
            timestamp_ms (int):
            volume_nanos (int):
            yes_price_nanos (int):
     """

    height: int
    no_price_nanos: int
    timestamp_ms: int
    volume_nanos: int
    yes_price_nanos: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        height = self.height

        no_price_nanos = self.no_price_nanos

        timestamp_ms = self.timestamp_ms

        volume_nanos = self.volume_nanos

        yes_price_nanos = self.yes_price_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "height": height,
            "no_price_nanos": no_price_nanos,
            "timestamp_ms": timestamp_ms,
            "volume_nanos": volume_nanos,
            "yes_price_nanos": yes_price_nanos,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        height = d.pop("height")

        no_price_nanos = d.pop("no_price_nanos")

        timestamp_ms = d.pop("timestamp_ms")

        volume_nanos = d.pop("volume_nanos")

        yes_price_nanos = d.pop("yes_price_nanos")

        price_point_response = cls(
            height=height,
            no_price_nanos=no_price_nanos,
            timestamp_ms=timestamp_ms,
            volume_nanos=volume_nanos,
            yes_price_nanos=yes_price_nanos,
        )


        price_point_response.additional_properties = d
        return price_point_response

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
