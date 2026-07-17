from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.set_reference_prices_request_prices_nanos import SetReferencePricesRequestPricesNanos





T = TypeVar("T", bound="SetReferencePricesRequest")



@_attrs_define
class SetReferencePricesRequest:
    """ 
        Attributes:
            prices_nanos (SetReferencePricesRequestPricesNanos): Map of market_id -> reference price. Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
                Zero explicitly evicts the current reference for that market.
     """

    prices_nanos: SetReferencePricesRequestPricesNanos





    def to_dict(self) -> dict[str, Any]:
        from ..models.set_reference_prices_request_prices_nanos import SetReferencePricesRequestPricesNanos
        prices_nanos = self.prices_nanos.to_dict()


        field_dict: dict[str, Any] = {}

        field_dict.update({
            "prices_nanos": prices_nanos,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.set_reference_prices_request_prices_nanos import SetReferencePricesRequestPricesNanos
        d = dict(src_dict)
        prices_nanos = SetReferencePricesRequestPricesNanos.from_dict(d.pop("prices_nanos"))




        set_reference_prices_request = cls(
            prices_nanos=prices_nanos,
        )

        return set_reference_prices_request

