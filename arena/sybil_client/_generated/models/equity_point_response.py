from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="EquityPointResponse")



@_attrs_define
class EquityPointResponse:
    """ 
        Attributes:
            deposited_nanos (str): Deposited amount at this point. Integer nanodollars; 1_000_000_000 = $1.
            height (int):
            portfolio_value_nanos (str): Portfolio value at this point. Integer nanodollars; 1_000_000_000 = $1.
            timestamp_ms (int):
     """

    deposited_nanos: str
    height: int
    portfolio_value_nanos: str
    timestamp_ms: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        deposited_nanos = self.deposited_nanos

        height = self.height

        portfolio_value_nanos = self.portfolio_value_nanos

        timestamp_ms = self.timestamp_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposited_nanos": deposited_nanos,
            "height": height,
            "portfolio_value_nanos": portfolio_value_nanos,
            "timestamp_ms": timestamp_ms,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        deposited_nanos = d.pop("deposited_nanos")

        height = d.pop("height")

        portfolio_value_nanos = d.pop("portfolio_value_nanos")

        timestamp_ms = d.pop("timestamp_ms")

        equity_point_response = cls(
            deposited_nanos=deposited_nanos,
            height=height,
            portfolio_value_nanos=portfolio_value_nanos,
            timestamp_ms=timestamp_ms,
        )


        equity_point_response.additional_properties = d
        return equity_point_response

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
