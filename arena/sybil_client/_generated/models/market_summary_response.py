from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MarketSummaryResponse")



@_attrs_define
class MarketSummaryResponse:
    """ Minimal market data for high-throughput dashboards (drops strings & metadata).

        Attributes:
            market_id (int):
            name (str):
            status (str):
            volume_nanos (int): All-time traded notional. Integer nanodollars; 1_000_000_000 = $1.
            liquidity_avg10_nanos (int | Unset): Liquidity depth score. Integer nanodollars; 1_000_000_000 = $1.
                Mirrors `MarketResponse`.
            liquidity_band_nanos (int | Unset): Liquidity price-band width. Integer nanodollars; 1_000_000_000 = $1.
                Mirrors `MarketResponse`.
            no_price_24h_ago_nanos (int | None | Unset): NO mark ~24h ago. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            no_price_nanos (int | None | Unset): Current NO mark, complementary to the YES mark. Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
            orders_matched_total (int | Unset):
            orders_placed_total (int | Unset): All-time placed/matched/unmatched (mirrors `MarketResponse`).
            orders_unmatched_total (int | Unset):
            reference_price_nanos (int | None | Unset): Reference price from external system (e.g., Polymarket), display
                only.
                Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            trader_count (int | Unset): All-time unique trader count (mirrors `MarketResponse.trader_count`).
            volume_24h_nanos (int | Unset): Rolling 24h trading volume. Integer nanodollars; 1_000_000_000 = $1.
                Mirrors
                `MarketResponse.volume_24h_nanos`).
            yes_price_24h_ago_nanos (int | None | Unset): YES mark ~24h ago. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            yes_price_nanos (int | None | Unset): Current YES mark: traded clearing price when filled, otherwise the
                committed book midpoint or carried mark. Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
     """

    market_id: int
    name: str
    status: str
    volume_nanos: int
    liquidity_avg10_nanos: int | Unset = UNSET
    liquidity_band_nanos: int | Unset = UNSET
    no_price_24h_ago_nanos: int | None | Unset = UNSET
    no_price_nanos: int | None | Unset = UNSET
    orders_matched_total: int | Unset = UNSET
    orders_placed_total: int | Unset = UNSET
    orders_unmatched_total: int | Unset = UNSET
    reference_price_nanos: int | None | Unset = UNSET
    trader_count: int | Unset = UNSET
    volume_24h_nanos: int | Unset = UNSET
    yes_price_24h_ago_nanos: int | None | Unset = UNSET
    yes_price_nanos: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        name = self.name

        status = self.status

        volume_nanos = self.volume_nanos

        liquidity_avg10_nanos = self.liquidity_avg10_nanos

        liquidity_band_nanos = self.liquidity_band_nanos

        no_price_24h_ago_nanos: int | None | Unset
        if isinstance(self.no_price_24h_ago_nanos, Unset):
            no_price_24h_ago_nanos = UNSET
        else:
            no_price_24h_ago_nanos = self.no_price_24h_ago_nanos

        no_price_nanos: int | None | Unset
        if isinstance(self.no_price_nanos, Unset):
            no_price_nanos = UNSET
        else:
            no_price_nanos = self.no_price_nanos

        orders_matched_total = self.orders_matched_total

        orders_placed_total = self.orders_placed_total

        orders_unmatched_total = self.orders_unmatched_total

        reference_price_nanos: int | None | Unset
        if isinstance(self.reference_price_nanos, Unset):
            reference_price_nanos = UNSET
        else:
            reference_price_nanos = self.reference_price_nanos

        trader_count = self.trader_count

        volume_24h_nanos = self.volume_24h_nanos

        yes_price_24h_ago_nanos: int | None | Unset
        if isinstance(self.yes_price_24h_ago_nanos, Unset):
            yes_price_24h_ago_nanos = UNSET
        else:
            yes_price_24h_ago_nanos = self.yes_price_24h_ago_nanos

        yes_price_nanos: int | None | Unset
        if isinstance(self.yes_price_nanos, Unset):
            yes_price_nanos = UNSET
        else:
            yes_price_nanos = self.yes_price_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "name": name,
            "status": status,
            "volume_nanos": volume_nanos,
        })
        if liquidity_avg10_nanos is not UNSET:
            field_dict["liquidity_avg10_nanos"] = liquidity_avg10_nanos
        if liquidity_band_nanos is not UNSET:
            field_dict["liquidity_band_nanos"] = liquidity_band_nanos
        if no_price_24h_ago_nanos is not UNSET:
            field_dict["no_price_24h_ago_nanos"] = no_price_24h_ago_nanos
        if no_price_nanos is not UNSET:
            field_dict["no_price_nanos"] = no_price_nanos
        if orders_matched_total is not UNSET:
            field_dict["orders_matched_total"] = orders_matched_total
        if orders_placed_total is not UNSET:
            field_dict["orders_placed_total"] = orders_placed_total
        if orders_unmatched_total is not UNSET:
            field_dict["orders_unmatched_total"] = orders_unmatched_total
        if reference_price_nanos is not UNSET:
            field_dict["reference_price_nanos"] = reference_price_nanos
        if trader_count is not UNSET:
            field_dict["trader_count"] = trader_count
        if volume_24h_nanos is not UNSET:
            field_dict["volume_24h_nanos"] = volume_24h_nanos
        if yes_price_24h_ago_nanos is not UNSET:
            field_dict["yes_price_24h_ago_nanos"] = yes_price_24h_ago_nanos
        if yes_price_nanos is not UNSET:
            field_dict["yes_price_nanos"] = yes_price_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        name = d.pop("name")

        status = d.pop("status")

        volume_nanos = d.pop("volume_nanos")

        liquidity_avg10_nanos = d.pop("liquidity_avg10_nanos", UNSET)

        liquidity_band_nanos = d.pop("liquidity_band_nanos", UNSET)

        def _parse_no_price_24h_ago_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        no_price_24h_ago_nanos = _parse_no_price_24h_ago_nanos(d.pop("no_price_24h_ago_nanos", UNSET))


        def _parse_no_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        no_price_nanos = _parse_no_price_nanos(d.pop("no_price_nanos", UNSET))


        orders_matched_total = d.pop("orders_matched_total", UNSET)

        orders_placed_total = d.pop("orders_placed_total", UNSET)

        orders_unmatched_total = d.pop("orders_unmatched_total", UNSET)

        def _parse_reference_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        reference_price_nanos = _parse_reference_price_nanos(d.pop("reference_price_nanos", UNSET))


        trader_count = d.pop("trader_count", UNSET)

        volume_24h_nanos = d.pop("volume_24h_nanos", UNSET)

        def _parse_yes_price_24h_ago_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_price_24h_ago_nanos = _parse_yes_price_24h_ago_nanos(d.pop("yes_price_24h_ago_nanos", UNSET))


        def _parse_yes_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_price_nanos = _parse_yes_price_nanos(d.pop("yes_price_nanos", UNSET))


        market_summary_response = cls(
            market_id=market_id,
            name=name,
            status=status,
            volume_nanos=volume_nanos,
            liquidity_avg10_nanos=liquidity_avg10_nanos,
            liquidity_band_nanos=liquidity_band_nanos,
            no_price_24h_ago_nanos=no_price_24h_ago_nanos,
            no_price_nanos=no_price_nanos,
            orders_matched_total=orders_matched_total,
            orders_placed_total=orders_placed_total,
            orders_unmatched_total=orders_unmatched_total,
            reference_price_nanos=reference_price_nanos,
            trader_count=trader_count,
            volume_24h_nanos=volume_24h_nanos,
            yes_price_24h_ago_nanos=yes_price_24h_ago_nanos,
            yes_price_nanos=yes_price_nanos,
        )


        market_summary_response.additional_properties = d
        return market_summary_response

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
