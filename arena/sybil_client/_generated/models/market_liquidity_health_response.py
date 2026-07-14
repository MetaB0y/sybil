from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MarketLiquidityHealthResponse")



@_attrs_define
class MarketLiquidityHealthResponse:
    """ 
        Attributes:
            clearing_price_present (bool):
            fill_volume_nanos (int): Filled notional on this market. Integer nanodollars; 1_000_000_000 = $1.
            market_id (int):
            mm_orders (int):
            noise_actor_count (int):
            noise_crossing_orders (int): Noise orders that happened to be marketable against the accepted MM
                shape. Computed post-submission; noise actors cannot read that shape.
            noise_orders (int):
            other_non_mm_orders (int):
            mm_skip_reason (None | str | Unset):
     """

    clearing_price_present: bool
    fill_volume_nanos: int
    market_id: int
    mm_orders: int
    noise_actor_count: int
    noise_crossing_orders: int
    noise_orders: int
    other_non_mm_orders: int
    mm_skip_reason: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        clearing_price_present = self.clearing_price_present

        fill_volume_nanos = self.fill_volume_nanos

        market_id = self.market_id

        mm_orders = self.mm_orders

        noise_actor_count = self.noise_actor_count

        noise_crossing_orders = self.noise_crossing_orders

        noise_orders = self.noise_orders

        other_non_mm_orders = self.other_non_mm_orders

        mm_skip_reason: None | str | Unset
        if isinstance(self.mm_skip_reason, Unset):
            mm_skip_reason = UNSET
        else:
            mm_skip_reason = self.mm_skip_reason


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "clearing_price_present": clearing_price_present,
            "fill_volume_nanos": fill_volume_nanos,
            "market_id": market_id,
            "mm_orders": mm_orders,
            "noise_actor_count": noise_actor_count,
            "noise_crossing_orders": noise_crossing_orders,
            "noise_orders": noise_orders,
            "other_non_mm_orders": other_non_mm_orders,
        })
        if mm_skip_reason is not UNSET:
            field_dict["mm_skip_reason"] = mm_skip_reason

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        clearing_price_present = d.pop("clearing_price_present")

        fill_volume_nanos = d.pop("fill_volume_nanos")

        market_id = d.pop("market_id")

        mm_orders = d.pop("mm_orders")

        noise_actor_count = d.pop("noise_actor_count")

        noise_crossing_orders = d.pop("noise_crossing_orders")

        noise_orders = d.pop("noise_orders")

        other_non_mm_orders = d.pop("other_non_mm_orders")

        def _parse_mm_skip_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mm_skip_reason = _parse_mm_skip_reason(d.pop("mm_skip_reason", UNSET))


        market_liquidity_health_response = cls(
            clearing_price_present=clearing_price_present,
            fill_volume_nanos=fill_volume_nanos,
            market_id=market_id,
            mm_orders=mm_orders,
            noise_actor_count=noise_actor_count,
            noise_crossing_orders=noise_crossing_orders,
            noise_orders=noise_orders,
            other_non_mm_orders=other_non_mm_orders,
            mm_skip_reason=mm_skip_reason,
        )


        market_liquidity_health_response.additional_properties = d
        return market_liquidity_health_response

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
