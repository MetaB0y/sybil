from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast






T = TypeVar("T", bound="SignedOrderData")



@_attrs_define
class SignedOrderData:
    """ 
        Attributes:
            limit_price_nanos (str): Limit price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            market_ids (list[int]): Market IDs this order spans.
            max_fill (int): Maximum fill quantity. Integer share-units; 1000 units = 1 share.
            payoffs (list[int]): Payoff vector.
     """

    limit_price_nanos: str
    market_ids: list[int]
    max_fill: int
    payoffs: list[int]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        limit_price_nanos = self.limit_price_nanos

        market_ids = self.market_ids



        max_fill = self.max_fill

        payoffs = self.payoffs




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "limit_price_nanos": limit_price_nanos,
            "market_ids": market_ids,
            "max_fill": max_fill,
            "payoffs": payoffs,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        limit_price_nanos = d.pop("limit_price_nanos")

        market_ids = cast(list[int], d.pop("market_ids"))


        max_fill = d.pop("max_fill")

        payoffs = cast(list[int], d.pop("payoffs"))


        signed_order_data = cls(
            limit_price_nanos=limit_price_nanos,
            market_ids=market_ids,
            max_fill=max_fill,
            payoffs=payoffs,
        )


        signed_order_data.additional_properties = d
        return signed_order_data

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
