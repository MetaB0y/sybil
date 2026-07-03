from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="PositionDeltaResponse")



@_attrs_define
class PositionDeltaResponse:
    """ 
        Attributes:
            delta (int):
            market_id (int):
            outcome (str):
     """

    delta: int
    market_id: int
    outcome: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        delta = self.delta

        market_id = self.market_id

        outcome = self.outcome


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "delta": delta,
            "market_id": market_id,
            "outcome": outcome,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        delta = d.pop("delta")

        market_id = d.pop("market_id")

        outcome = d.pop("outcome")

        position_delta_response = cls(
            delta=delta,
            market_id=market_id,
            outcome=outcome,
        )


        position_delta_response.additional_properties = d
        return position_delta_response

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
