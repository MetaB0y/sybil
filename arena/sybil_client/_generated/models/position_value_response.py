from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="PositionValueResponse")



@_attrs_define
class PositionValueResponse:
    """ 
        Attributes:
            current_price_nanos (int):
            market_id (int):
            outcome (str):
            quantity (int): Signed position quantity in fixed-point share-units (`1000` = 1 share).
            value_nanos (int):
            avg_entry_price_nanos (int | Unset): Weighted-average entry price for this side of the market (C1). `0`
                for positions opened before C1 landed (`#[serde(default)]` forward
                compat). Same units as `current_price_nanos`.
     """

    current_price_nanos: int
    market_id: int
    outcome: str
    quantity: int
    value_nanos: int
    avg_entry_price_nanos: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        current_price_nanos = self.current_price_nanos

        market_id = self.market_id

        outcome = self.outcome

        quantity = self.quantity

        value_nanos = self.value_nanos

        avg_entry_price_nanos = self.avg_entry_price_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "current_price_nanos": current_price_nanos,
            "market_id": market_id,
            "outcome": outcome,
            "quantity": quantity,
            "value_nanos": value_nanos,
        })
        if avg_entry_price_nanos is not UNSET:
            field_dict["avg_entry_price_nanos"] = avg_entry_price_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        current_price_nanos = d.pop("current_price_nanos")

        market_id = d.pop("market_id")

        outcome = d.pop("outcome")

        quantity = d.pop("quantity")

        value_nanos = d.pop("value_nanos")

        avg_entry_price_nanos = d.pop("avg_entry_price_nanos", UNSET)

        position_value_response = cls(
            current_price_nanos=current_price_nanos,
            market_id=market_id,
            outcome=outcome,
            quantity=quantity,
            value_nanos=value_nanos,
            avg_entry_price_nanos=avg_entry_price_nanos,
        )


        position_value_response.additional_properties = d
        return position_value_response

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
