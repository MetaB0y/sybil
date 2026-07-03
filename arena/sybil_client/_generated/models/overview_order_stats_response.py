from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="OverviewOrderStatsResponse")



@_attrs_define
class OverviewOrderStatsResponse:
    """ 
        Attributes:
            matched (int | Unset):
            placed (int | Unset):
            placed_distinct (int | Unset): Distinct orders admitted (counted once per order at intake), all-time
                or rolling 24h. `placed` above stays per-batch participation for
                back-compat: a resting order counts once here but once per batch there.
            unmatched (int | Unset):
     """

    matched: int | Unset = UNSET
    placed: int | Unset = UNSET
    placed_distinct: int | Unset = UNSET
    unmatched: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        matched = self.matched

        placed = self.placed

        placed_distinct = self.placed_distinct

        unmatched = self.unmatched


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if matched is not UNSET:
            field_dict["matched"] = matched
        if placed is not UNSET:
            field_dict["placed"] = placed
        if placed_distinct is not UNSET:
            field_dict["placed_distinct"] = placed_distinct
        if unmatched is not UNSET:
            field_dict["unmatched"] = unmatched

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        matched = d.pop("matched", UNSET)

        placed = d.pop("placed", UNSET)

        placed_distinct = d.pop("placed_distinct", UNSET)

        unmatched = d.pop("unmatched", UNSET)

        overview_order_stats_response = cls(
            matched=matched,
            placed=placed,
            placed_distinct=placed_distinct,
            unmatched=unmatched,
        )


        overview_order_stats_response.additional_properties = d
        return overview_order_stats_response

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
