from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.order_spec_type_2_type import OrderSpecType2Type






T = TypeVar("T", bound="OrderSpecType2")



@_attrs_define
class OrderSpecType2:
    """ Sell YES share-units on a single market (`1000` units = 1 share).

        Attributes:
            limit_price_nanos (int):
            market_id (int):
            quantity (int): Quantity in fixed-point share-units.
            type_ (OrderSpecType2Type):
     """

    limit_price_nanos: int
    market_id: int
    quantity: int
    type_: OrderSpecType2Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        limit_price_nanos = self.limit_price_nanos

        market_id = self.market_id

        quantity = self.quantity

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "limit_price_nanos": limit_price_nanos,
            "market_id": market_id,
            "quantity": quantity,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        limit_price_nanos = d.pop("limit_price_nanos")

        market_id = d.pop("market_id")

        quantity = d.pop("quantity")

        type_ = OrderSpecType2Type(d.pop("type"))




        order_spec_type_2 = cls(
            limit_price_nanos=limit_price_nanos,
            market_id=market_id,
            quantity=quantity,
            type_=type_,
        )


        order_spec_type_2.additional_properties = d
        return order_spec_type_2

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
