from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="OrderAdmissionPolicyResponse")



@_attrs_define
class OrderAdmissionPolicyResponse:
    """ Public constraints needed to construct orders that admission can accept.

        Attributes:
            min_order_notional_nanos (str): Minimum ceil(limit_price * quantity / 1000) for ordinary non-MM orders.
                Integer nanodollars; 1_000_000_000 = $1.
            share_scale (int): Integer quantity units per user-facing share.
     """

    min_order_notional_nanos: str
    share_scale: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        min_order_notional_nanos = self.min_order_notional_nanos

        share_scale = self.share_scale


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "min_order_notional_nanos": min_order_notional_nanos,
            "share_scale": share_scale,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        min_order_notional_nanos = d.pop("min_order_notional_nanos")

        share_scale = d.pop("share_scale")

        order_admission_policy_response = cls(
            min_order_notional_nanos=min_order_notional_nanos,
            share_scale=share_scale,
        )


        order_admission_policy_response.additional_properties = d
        return order_admission_policy_response

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
