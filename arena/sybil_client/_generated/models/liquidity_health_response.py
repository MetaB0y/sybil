from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.actor_identity_response import ActorIdentityResponse
  from ..models.market_liquidity_health_response import MarketLiquidityHealthResponse





T = TypeVar("T", bound="LiquidityHealthResponse")



@_attrs_define
class LiquidityHealthResponse:
    """ 
        Attributes:
            active_markets (int):
            expected_noise_actors (int):
            height (int):
            markets (list[MarketLiquidityHealthResponse]):
            markets_with_clearing_prices (int):
            markets_with_noise_fills (int):
            markets_with_three_noise_actors (int):
            markets_with_two_noise_actors (int):
            mm_coverage_bps (int):
            mm_markets_quoted (int):
            mm_markets_two_sided (int):
            mm_two_sided_coverage_bps (int):
            noise_coverage_bps (int):
            noise_crossing_coverage_bps (int):
            noise_markets_crossing_mm (int): Markets with a naturally MM-marketable noise order, measured
                post-submission rather than coordinated by the noise actor.
            noise_markets_selected (int):
            observed_noise_actors (int):
            rolling_mm_coverage_bps (int):
            rolling_mm_two_sided_coverage_bps (int):
            rolling_noise_coverage_bps (int):
            rolling_noise_crossing_coverage_bps (int):
            rolling_noise_fill_coverage_bps (int):
            rolling_window_blocks (int): Number of committed blocks used for rolling actor coverage.
            total_fills (int):
            total_rejections (int):
            total_volume_nanos (int): Filled notional across the block. Integer nanodollars; 1_000_000_000 = $1.
            universe_generation (int):
            actors (list[ActorIdentityResponse] | Unset):
     """

    active_markets: int
    expected_noise_actors: int
    height: int
    markets: list[MarketLiquidityHealthResponse]
    markets_with_clearing_prices: int
    markets_with_noise_fills: int
    markets_with_three_noise_actors: int
    markets_with_two_noise_actors: int
    mm_coverage_bps: int
    mm_markets_quoted: int
    mm_markets_two_sided: int
    mm_two_sided_coverage_bps: int
    noise_coverage_bps: int
    noise_crossing_coverage_bps: int
    noise_markets_crossing_mm: int
    noise_markets_selected: int
    observed_noise_actors: int
    rolling_mm_coverage_bps: int
    rolling_mm_two_sided_coverage_bps: int
    rolling_noise_coverage_bps: int
    rolling_noise_crossing_coverage_bps: int
    rolling_noise_fill_coverage_bps: int
    rolling_window_blocks: int
    total_fills: int
    total_rejections: int
    total_volume_nanos: int
    universe_generation: int
    actors: list[ActorIdentityResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.actor_identity_response import ActorIdentityResponse
        from ..models.market_liquidity_health_response import MarketLiquidityHealthResponse
        active_markets = self.active_markets

        expected_noise_actors = self.expected_noise_actors

        height = self.height

        markets = []
        for markets_item_data in self.markets:
            markets_item = markets_item_data.to_dict()
            markets.append(markets_item)



        markets_with_clearing_prices = self.markets_with_clearing_prices

        markets_with_noise_fills = self.markets_with_noise_fills

        markets_with_three_noise_actors = self.markets_with_three_noise_actors

        markets_with_two_noise_actors = self.markets_with_two_noise_actors

        mm_coverage_bps = self.mm_coverage_bps

        mm_markets_quoted = self.mm_markets_quoted

        mm_markets_two_sided = self.mm_markets_two_sided

        mm_two_sided_coverage_bps = self.mm_two_sided_coverage_bps

        noise_coverage_bps = self.noise_coverage_bps

        noise_crossing_coverage_bps = self.noise_crossing_coverage_bps

        noise_markets_crossing_mm = self.noise_markets_crossing_mm

        noise_markets_selected = self.noise_markets_selected

        observed_noise_actors = self.observed_noise_actors

        rolling_mm_coverage_bps = self.rolling_mm_coverage_bps

        rolling_mm_two_sided_coverage_bps = self.rolling_mm_two_sided_coverage_bps

        rolling_noise_coverage_bps = self.rolling_noise_coverage_bps

        rolling_noise_crossing_coverage_bps = self.rolling_noise_crossing_coverage_bps

        rolling_noise_fill_coverage_bps = self.rolling_noise_fill_coverage_bps

        rolling_window_blocks = self.rolling_window_blocks

        total_fills = self.total_fills

        total_rejections = self.total_rejections

        total_volume_nanos = self.total_volume_nanos

        universe_generation = self.universe_generation

        actors: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.actors, Unset):
            actors = []
            for actors_item_data in self.actors:
                actors_item = actors_item_data.to_dict()
                actors.append(actors_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "active_markets": active_markets,
            "expected_noise_actors": expected_noise_actors,
            "height": height,
            "markets": markets,
            "markets_with_clearing_prices": markets_with_clearing_prices,
            "markets_with_noise_fills": markets_with_noise_fills,
            "markets_with_three_noise_actors": markets_with_three_noise_actors,
            "markets_with_two_noise_actors": markets_with_two_noise_actors,
            "mm_coverage_bps": mm_coverage_bps,
            "mm_markets_quoted": mm_markets_quoted,
            "mm_markets_two_sided": mm_markets_two_sided,
            "mm_two_sided_coverage_bps": mm_two_sided_coverage_bps,
            "noise_coverage_bps": noise_coverage_bps,
            "noise_crossing_coverage_bps": noise_crossing_coverage_bps,
            "noise_markets_crossing_mm": noise_markets_crossing_mm,
            "noise_markets_selected": noise_markets_selected,
            "observed_noise_actors": observed_noise_actors,
            "rolling_mm_coverage_bps": rolling_mm_coverage_bps,
            "rolling_mm_two_sided_coverage_bps": rolling_mm_two_sided_coverage_bps,
            "rolling_noise_coverage_bps": rolling_noise_coverage_bps,
            "rolling_noise_crossing_coverage_bps": rolling_noise_crossing_coverage_bps,
            "rolling_noise_fill_coverage_bps": rolling_noise_fill_coverage_bps,
            "rolling_window_blocks": rolling_window_blocks,
            "total_fills": total_fills,
            "total_rejections": total_rejections,
            "total_volume_nanos": total_volume_nanos,
            "universe_generation": universe_generation,
        })
        if actors is not UNSET:
            field_dict["actors"] = actors

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.actor_identity_response import ActorIdentityResponse
        from ..models.market_liquidity_health_response import MarketLiquidityHealthResponse
        d = dict(src_dict)
        active_markets = d.pop("active_markets")

        expected_noise_actors = d.pop("expected_noise_actors")

        height = d.pop("height")

        markets = []
        _markets = d.pop("markets")
        for markets_item_data in (_markets):
            markets_item = MarketLiquidityHealthResponse.from_dict(markets_item_data)



            markets.append(markets_item)


        markets_with_clearing_prices = d.pop("markets_with_clearing_prices")

        markets_with_noise_fills = d.pop("markets_with_noise_fills")

        markets_with_three_noise_actors = d.pop("markets_with_three_noise_actors")

        markets_with_two_noise_actors = d.pop("markets_with_two_noise_actors")

        mm_coverage_bps = d.pop("mm_coverage_bps")

        mm_markets_quoted = d.pop("mm_markets_quoted")

        mm_markets_two_sided = d.pop("mm_markets_two_sided")

        mm_two_sided_coverage_bps = d.pop("mm_two_sided_coverage_bps")

        noise_coverage_bps = d.pop("noise_coverage_bps")

        noise_crossing_coverage_bps = d.pop("noise_crossing_coverage_bps")

        noise_markets_crossing_mm = d.pop("noise_markets_crossing_mm")

        noise_markets_selected = d.pop("noise_markets_selected")

        observed_noise_actors = d.pop("observed_noise_actors")

        rolling_mm_coverage_bps = d.pop("rolling_mm_coverage_bps")

        rolling_mm_two_sided_coverage_bps = d.pop("rolling_mm_two_sided_coverage_bps")

        rolling_noise_coverage_bps = d.pop("rolling_noise_coverage_bps")

        rolling_noise_crossing_coverage_bps = d.pop("rolling_noise_crossing_coverage_bps")

        rolling_noise_fill_coverage_bps = d.pop("rolling_noise_fill_coverage_bps")

        rolling_window_blocks = d.pop("rolling_window_blocks")

        total_fills = d.pop("total_fills")

        total_rejections = d.pop("total_rejections")

        total_volume_nanos = d.pop("total_volume_nanos")

        universe_generation = d.pop("universe_generation")

        _actors = d.pop("actors", UNSET)
        actors: list[ActorIdentityResponse] | Unset = UNSET
        if _actors is not UNSET:
            actors = []
            for actors_item_data in _actors:
                actors_item = ActorIdentityResponse.from_dict(actors_item_data)



                actors.append(actors_item)


        liquidity_health_response = cls(
            active_markets=active_markets,
            expected_noise_actors=expected_noise_actors,
            height=height,
            markets=markets,
            markets_with_clearing_prices=markets_with_clearing_prices,
            markets_with_noise_fills=markets_with_noise_fills,
            markets_with_three_noise_actors=markets_with_three_noise_actors,
            markets_with_two_noise_actors=markets_with_two_noise_actors,
            mm_coverage_bps=mm_coverage_bps,
            mm_markets_quoted=mm_markets_quoted,
            mm_markets_two_sided=mm_markets_two_sided,
            mm_two_sided_coverage_bps=mm_two_sided_coverage_bps,
            noise_coverage_bps=noise_coverage_bps,
            noise_crossing_coverage_bps=noise_crossing_coverage_bps,
            noise_markets_crossing_mm=noise_markets_crossing_mm,
            noise_markets_selected=noise_markets_selected,
            observed_noise_actors=observed_noise_actors,
            rolling_mm_coverage_bps=rolling_mm_coverage_bps,
            rolling_mm_two_sided_coverage_bps=rolling_mm_two_sided_coverage_bps,
            rolling_noise_coverage_bps=rolling_noise_coverage_bps,
            rolling_noise_crossing_coverage_bps=rolling_noise_crossing_coverage_bps,
            rolling_noise_fill_coverage_bps=rolling_noise_fill_coverage_bps,
            rolling_window_blocks=rolling_window_blocks,
            total_fills=total_fills,
            total_rejections=total_rejections,
            total_volume_nanos=total_volume_nanos,
            universe_generation=universe_generation,
            actors=actors,
        )


        liquidity_health_response.additional_properties = d
        return liquidity_health_response

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
