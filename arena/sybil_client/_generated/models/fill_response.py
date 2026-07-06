from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="FillResponse")



@_attrs_define
class FillResponse:
    """ 
        Attributes:
            fill_price_nanos (int): Fill price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            fill_qty (int): Fill quantity. Integer share-units; 1000 units = 1 share.
            order_id (int):
            account_id (int | Unset):
     """

    fill_price_nanos: int
    fill_qty: int
    order_id: int
    account_id: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        fill_price_nanos = self.fill_price_nanos

        fill_qty = self.fill_qty

        order_id = self.order_id

        account_id = self.account_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "fill_price_nanos": fill_price_nanos,
            "fill_qty": fill_qty,
            "order_id": order_id,
        })
        if account_id is not UNSET:
            field_dict["account_id"] = account_id

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        fill_price_nanos = d.pop("fill_price_nanos")

        fill_qty = d.pop("fill_qty")

        order_id = d.pop("order_id")

        account_id = d.pop("account_id", UNSET)

        fill_response = cls(
            fill_price_nanos=fill_price_nanos,
            fill_qty=fill_qty,
            order_id=order_id,
            account_id=account_id,
        )


        fill_response.additional_properties = d
        return fill_response

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
