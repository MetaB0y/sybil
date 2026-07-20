from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="ExecutionQualityResponse")



@_attrs_define
class ExecutionQualityResponse:
    """ Product execution and liquidity utilization for one time window.

    The rolling window is admission-cohort based: a carried order's first fill
    is credited to its original admission hour, so each numerator is bounded by
    its corresponding denominator.

        Attributes:
            maker_quotes_hit (int | Unset): Worked MM quote orders that received at least one positive fill.
            maker_quotes_worked (int | Unset): One-block operator MM quote orders worked.
            trader_orders_admitted (int | Unset): Fresh non-MM orders admitted once each.
            trader_orders_first_filled (int | Unset): Admitted trader orders that received at least one positive fill.
     """

    maker_quotes_hit: int | Unset = UNSET
    maker_quotes_worked: int | Unset = UNSET
    trader_orders_admitted: int | Unset = UNSET
    trader_orders_first_filled: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        maker_quotes_hit = self.maker_quotes_hit

        maker_quotes_worked = self.maker_quotes_worked

        trader_orders_admitted = self.trader_orders_admitted

        trader_orders_first_filled = self.trader_orders_first_filled


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if maker_quotes_hit is not UNSET:
            field_dict["maker_quotes_hit"] = maker_quotes_hit
        if maker_quotes_worked is not UNSET:
            field_dict["maker_quotes_worked"] = maker_quotes_worked
        if trader_orders_admitted is not UNSET:
            field_dict["trader_orders_admitted"] = trader_orders_admitted
        if trader_orders_first_filled is not UNSET:
            field_dict["trader_orders_first_filled"] = trader_orders_first_filled

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        maker_quotes_hit = d.pop("maker_quotes_hit", UNSET)

        maker_quotes_worked = d.pop("maker_quotes_worked", UNSET)

        trader_orders_admitted = d.pop("trader_orders_admitted", UNSET)

        trader_orders_first_filled = d.pop("trader_orders_first_filled", UNSET)

        execution_quality_response = cls(
            maker_quotes_hit=maker_quotes_hit,
            maker_quotes_worked=maker_quotes_worked,
            trader_orders_admitted=trader_orders_admitted,
            trader_orders_first_filled=trader_orders_first_filled,
        )


        execution_quality_response.additional_properties = d
        return execution_quality_response

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
