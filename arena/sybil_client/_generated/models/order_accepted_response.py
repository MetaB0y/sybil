from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast






T = TypeVar("T", bound="OrderAcceptedResponse")



@_attrs_define
class OrderAcceptedResponse:
    """ 
        Attributes:
            accepted (bool):
            order_ids (list[int]): Sequencer-assigned IDs for the admitted orders, in request order.
     """

    accepted: bool
    order_ids: list[int]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        accepted = self.accepted

        order_ids = self.order_ids




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "accepted": accepted,
            "order_ids": order_ids,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        accepted = d.pop("accepted")

        order_ids = cast(list[int], d.pop("order_ids"))


        order_accepted_response = cls(
            accepted=accepted,
            order_ids=order_ids,
        )


        order_accepted_response.additional_properties = d
        return order_accepted_response

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
