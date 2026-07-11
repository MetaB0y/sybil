from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="AdmitTimingViewResponse")



@_attrs_define
class AdmitTimingViewResponse:
    """ 
        Attributes:
            account_id (int):
            admit_height (int):
            admit_timestamp_ms (int): Admission timestamp in Unix epoch milliseconds.
            is_mm (bool):
            is_new (bool):
            order_id (int):
     """

    account_id: int
    admit_height: int
    admit_timestamp_ms: int
    is_mm: bool
    is_new: bool
    order_id: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        admit_height = self.admit_height

        admit_timestamp_ms = self.admit_timestamp_ms

        is_mm = self.is_mm

        is_new = self.is_new

        order_id = self.order_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "admit_height": admit_height,
            "admit_timestamp_ms": admit_timestamp_ms,
            "is_mm": is_mm,
            "is_new": is_new,
            "order_id": order_id,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        admit_height = d.pop("admit_height")

        admit_timestamp_ms = d.pop("admit_timestamp_ms")

        is_mm = d.pop("is_mm")

        is_new = d.pop("is_new")

        order_id = d.pop("order_id")

        admit_timing_view_response = cls(
            account_id=account_id,
            admit_height=admit_height,
            admit_timestamp_ms=admit_timestamp_ms,
            is_mm=is_mm,
            is_new=is_new,
            order_id=order_id,
        )


        admit_timing_view_response.additional_properties = d
        return admit_timing_view_response

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
