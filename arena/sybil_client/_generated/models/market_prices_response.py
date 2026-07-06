from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.market_prices_response_prices import MarketPricesResponsePrices





T = TypeVar("T", bound="MarketPricesResponse")



@_attrs_define
class MarketPricesResponse:
    """ 
        Attributes:
            prices (MarketPricesResponsePrices): Market price map. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
     """

    prices: MarketPricesResponsePrices
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.market_prices_response_prices import MarketPricesResponsePrices
        prices = self.prices.to_dict()


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "prices": prices,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.market_prices_response_prices import MarketPricesResponsePrices
        d = dict(src_dict)
        prices = MarketPricesResponsePrices.from_dict(d.pop("prices"))




        market_prices_response = cls(
            prices=prices,
        )


        market_prices_response.additional_properties = d
        return market_prices_response

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
