from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.block_market_stats import BlockMarketStats





T = TypeVar("T", bound="PublicBlockResponseByMarket")



@_attrs_define
class PublicBlockResponseByMarket:
    """ 
     """

    additional_properties: dict[str, BlockMarketStats] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.block_market_stats import BlockMarketStats
        
        field_dict: dict[str, Any] = {}
        for prop_name, prop in self.additional_properties.items():
            field_dict[prop_name] = prop.to_dict()


        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.block_market_stats import BlockMarketStats
        d = dict(src_dict)
        public_block_response_by_market = cls(
        )


        additional_properties = {}
        for prop_name, prop_dict in d.items():
            additional_property = BlockMarketStats.from_dict(prop_dict)



            additional_properties[prop_name] = additional_property

        public_block_response_by_market.additional_properties = additional_properties
        return public_block_response_by_market

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> BlockMarketStats:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: BlockMarketStats) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
