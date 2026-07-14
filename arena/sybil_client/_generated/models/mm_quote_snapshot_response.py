from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.mm_quote_market_response import MmQuoteMarketResponse





T = TypeVar("T", bound="MmQuoteSnapshotResponse")



@_attrs_define
class MmQuoteSnapshotResponse:
    """ 
        Attributes:
            markets (list[MmQuoteMarketResponse]):
            observed_at_ms (int):
            target_height (int):
            universe_generation (int):
     """

    markets: list[MmQuoteMarketResponse]
    observed_at_ms: int
    target_height: int
    universe_generation: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.mm_quote_market_response import MmQuoteMarketResponse
        markets = []
        for markets_item_data in self.markets:
            markets_item = markets_item_data.to_dict()
            markets.append(markets_item)



        observed_at_ms = self.observed_at_ms

        target_height = self.target_height

        universe_generation = self.universe_generation


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "markets": markets,
            "observed_at_ms": observed_at_ms,
            "target_height": target_height,
            "universe_generation": universe_generation,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.mm_quote_market_response import MmQuoteMarketResponse
        d = dict(src_dict)
        markets = []
        _markets = d.pop("markets")
        for markets_item_data in (_markets):
            markets_item = MmQuoteMarketResponse.from_dict(markets_item_data)



            markets.append(markets_item)


        observed_at_ms = d.pop("observed_at_ms")

        target_height = d.pop("target_height")

        universe_generation = d.pop("universe_generation")

        mm_quote_snapshot_response = cls(
            markets=markets,
            observed_at_ms=observed_at_ms,
            target_height=target_height,
            universe_generation=universe_generation,
        )


        mm_quote_snapshot_response.additional_properties = d
        return mm_quote_snapshot_response

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
